#!/bin/sh
# Discovery for rust_project: find every Rust crate plz knows about, in this
# repo and in the subrepos it pulls in, and describe them for rust-analyzer.
#
# The rule prepends a prelude setting NAME, TOOL_LABEL, TARGETS, EXCLUDES,
# SUBREPOS, LOCK_LABEL, THIRD_PARTY_DIR, SYSROOT, SYSROOT_SRC and OUT_FILE.
# Values are never substituted into the shell text, so no path or label can be
# read as syntax.
set -eu

# Two ways in. Run by hand it writes a file; run by rust-analyzer through
# `workspace.discoverConfig` it speaks the discover protocol on stdout - JSON
# objects, one per line - so the editor asks for the project itself and nobody
# has to remember to regenerate anything. Same shape as go-rules' package
# driver for gopls.
DISCOVER=0
if [ "${1:-}" = "--discover" ]; then
    DISCOVER=1
    shift
fi

cd "`plz query reporoot`"

# Progress, in whichever form the caller understands. Messages are kept free of
# quotes and backslashes so that this needs no JSON escaping.
say() {
    if [ "$DISCOVER" = 1 ]; then
        printf '{"kind":"progress","message":"%s"}\n' "$1"
    else
        echo "$NAME: $1"
    fi
}

# JSON has no opinion about shell quoting, so anything that reaches a message
# is escaped: plz's own errors arrive here and contain quotes and newlines.
esc() {
    printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g' | tr '\n\r\t' '   '
}

# A failure has to reach rust-analyzer as protocol too: exiting non-zero with
# nothing on stdout tells it only that something went wrong.
die() {
    if [ "$DISCOVER" = 1 ]; then
        printf '{"kind":"error","error":"%s","source":null}\n' "`esc \"$1\"`"
        exit 0
    fi
    echo "$NAME: $1" >&2
    exit 1
}

WORK=`mktemp -d`
trap 'rm -rf "$WORK"' EXIT
: > "$WORK/skipped"

# Labels the caller asked not to see, matched on the front of the label.
drop_excluded() {
    if [ -z "$EXCLUDES" ]; then
        echo "$1"
        return 0
    fi
    echo "$1" | while IFS= read -r drop_label; do
        if [ -z "$drop_label" ]; then continue; fi
        drop_keep=1
        for drop_ex in $EXCLUDES; do
            case "$drop_label" in "$drop_ex"*) drop_keep=0 ;; esac
        done
        if [ "$drop_keep" = 1 ]; then echo "$drop_label"; fi
    done
}

# A sweep can fail for reasons that have nothing to do with the crates in it.
# A package may reference a plugin this repo does not have, and plugin names
# are one global namespace, so a subrepo declaring a plugin the host declares
# too aborts the parse outright. Neither is a reason to lose the rest of the
# subrepo: a failing sweep descends a level and skips only the packages that
# cannot be parsed. A subrepo with no such package costs a single query.
#
# Breadth-first over files rather than a recursive function: POSIX sh has no
# local variables, so a recursive sweep overwrites the path it is walking.
sweep() {
    sweep_prefix=$1
    sweep_root=$2
    sweep_maxdepth=$3
    sweep_start=${4:-}
    printf '%s\n' "$sweep_start" > "$WORK/level"
    sweep_depth=0
    sweep_budget=200
    while [ -s "$WORK/level" ]; do
        : > "$WORK/next"
        while IFS= read -r sweep_path; do
            if [ "$sweep_budget" -le 0 ]; then break; fi
            sweep_budget=`expr $sweep_budget - 1`
            if [ -n "$sweep_path" ]; then
                sweep_pat="$sweep_prefix$sweep_path/..."
            else
                sweep_pat="$sweep_prefix..."
            fi
            if sweep_out=`plz query alltargets "$sweep_pat" --include rust_ide --hidden 2>/dev/null`; then
                if [ -n "$sweep_out" ]; then echo "$sweep_out" >> "$WORK/found"; fi
                continue
            fi
            sweep_dir=$sweep_root
            if [ -n "$sweep_path" ]; then sweep_dir="$sweep_root/$sweep_path"; fi
            if [ "$sweep_depth" -ge "$sweep_maxdepth" ] || [ -z "$sweep_root" ] || [ ! -d "$sweep_dir" ]; then
                echo "$sweep_pat" >> "$WORK/skipped"
                continue
            fi
            sweep_went=0
            for sweep_child in "$sweep_dir"/*/; do
                if [ ! -d "$sweep_child" ]; then continue; fi
                sweep_name=`basename "$sweep_child"`
                # A subrepo checkout has its own plz-out; that is build output.
                if [ "$sweep_name" = "plz-out" ]; then continue; fi
                if [ -n "$sweep_path" ]; then
                    echo "$sweep_path/$sweep_name" >> "$WORK/next"
                else
                    echo "$sweep_name" >> "$WORK/next"
                fi
                sweep_went=1
            done
            if [ "$sweep_went" = 0 ]; then echo "$sweep_pat" >> "$WORK/skipped"; fi
        done < "$WORK/level"
        mv "$WORK/next" "$WORK/level"
        sweep_depth=`expr $sweep_depth + 1`
        if [ "$sweep_depth" -gt "$sweep_maxdepth" ]; then break; fi
    done
}

# This repo's own crates, swept the same way a subrepo is. A package that will
# not parse - one referencing a plugin this repo does not have, or a crate
# declaration that is missing - would otherwise take the whole repo with it,
# and the only way round that was to name subtrees in the BUILD file and hope
# they stayed right.
: > "$WORK/found"
for target in $TARGETS; do
    # `//src/...` and `//...` differ only in where they start from.
    target_path=${target#//}
    target_path=${target_path%...}
    target_path=${target_path%/}
    sweep "//" "." 4 "$target_path"
done
FRAGS=`drop_excluded "\`cat \"$WORK/found\"\`"`
if [ -z "$FRAGS" ]; then
    die "no Rust crates found under $TARGETS"
fi
say "describing `echo $FRAGS | wc -w` crates"
# The toolchain too: resolving where it lands is not the same as it being
# there, and a sysroot_src that was never built is a std with no sources.
plz build $TOOL_LABEL $LOCK_LABEL $SYSROOT_TARGET $SYSROOT_SRC_TARGET $FRAGS >/dev/null
TOOL=`plz query outputs $TOOL_LABEL`
# The toolchain is wherever whoever declared it put it, so ask rather than
# assume. An explicit sysroot in the rule wins and skips this.
if [ -z "$SYSROOT" ] && [ -n "$SYSROOT_TARGET" ]; then
    SYSROOT=`plz query outputs "$SYSROOT_TARGET" 2>/dev/null | head -1` || SYSROOT=""
fi
if [ -z "$SYSROOT_SRC" ] && [ -n "$SYSROOT_SRC_TARGET" ]; then
    SYSROOT_SRC=`plz query outputs "$SYSROOT_SRC_TARGET" 2>/dev/null | head -1` || SYSROOT_SRC=""
fi
if [ -z "$SYSROOT" ]; then
    echo "$NAME: could not locate the toolchain, so rust-analyzer will have no std" >&2
fi
LOCK_ARG=""
if [ -n "$LOCK_LABEL" ]; then
    LOCK_ARG="--lock `plz query outputs $LOCK_LABEL`"
fi
FILES=`plz query outputs $FRAGS`

# Subrepos. Plugin ones plz already lists, so they need no naming; anything
# brought in another way was named in `subrepos`. Third-party crates come from
# the lock and emit no fragments, so a sweep cannot describe one twice.
plz query config 2>/dev/null | awk '
    /^\[/ { name = "" }
    /^\[plugin "/ { name = $0; sub(/^\[plugin "/, "", name); sub(/"\]$/, "", name) }
    /^target = / { if (name != "") { print name, $3; name = "" } }
' > "$WORK/subrepos" || : > "$WORK/subrepos"
for sub in $SUBREPOS; do echo "$sub" >> "$WORK/subrepos"; done

: > "$WORK/subargs"
while read -r sub target; do
    if [ -z "$sub" ]; then continue; fi
    # Where the subrepo is checked out, which is the prefix its paths need and
    # what a descent walks.
    root=""
    if [ -n "${target:-}" ]; then
        root=`plz query outputs "$target" 2>/dev/null | head -1` || root=""
    fi
    # Without knowing where the subrepo is, its paths cannot be rebased and
    # every crate in it would point at a file that is not there.
    if [ -z "$root" ]; then
        echo "$NAME: cannot locate the $sub subrepo, so its crates are not described" >&2
        continue
    fi
    # A plugin subrepo that is not a Rust one - the cc, shell or proto rules -
    # has nothing to find, and descending it on a failed parse is pure cost.
    # One find answers it.
    if [ -n "$root" ]; then
        if [ -z "`find "$root" -name '*.rs' -print -quit 2>/dev/null`" ]; then
            continue
        fi
    fi
    : > "$WORK/found"
    sweep "///$sub//" "$root" 3
    drop_excluded "`cat "$WORK/found"`" > "$WORK/frags"
    while read -r frag; do
        if [ -z "$frag" ]; then continue; fi
        # A subrepo resolving back to this repo - what a plugin repo declaring
        # itself for its own tests does - returns host labels. Describing those
        # again is every crate here twice.
        case "$frag" in ///*) ;; *) continue ;; esac
        plz build "$frag" >/dev/null 2>&1 || continue
        json=`plz query outputs "$frag" 2>/dev/null | head -1` || continue
        # Everything the fragment names is relative to the subrepo, and $root
        # is where that subrepo is. plz reports only *this* repo's files as a
        # target's inputs, so asking it where the source is does not work
        # across the boundary - but where the subrepo is checked out does.
        if [ -n "$json" ]; then
            echo "--subrepo-crate $root=$json" >> "$WORK/subargs"
        fi
    done < "$WORK/frags"
done < "$WORK/subrepos"

SUBARGS=`tr '\n' ' ' < "$WORK/subargs"`
SUBN=`wc -l < "$WORK/subargs"`

while read -r s; do
    echo "$NAME: could not parse $s, so any crates in it are not described" >&2
done < "$WORK/skipped"

# Build whatever the project is about to point at.
#
# Naming a path is not the same as it existing, and the difference is silent
# every time: rust-analyzer degrades and reports something unrelated. This was
# a list of artifact kinds, and the list was wrong four times - the sysroot,
# its sources, the lock, the proc-macro dylibs - so it is not a list any more.
# The tool says what it will point at; anything missing gets built, whatever
# kind of thing it is.
if [ -n "$LOCK_LABEL" ]; then
    LOCK_PKG=${LOCK_LABEL%:*}
    # shellcheck disable=SC2086
    "$TOOL" ide $LOCK_ARG --third-party-dir $THIRD_PARTY_DIR --sysroot $SYSROOT \
        --sysroot-src $SYSROOT_SRC --first-party $FILES $SUBARGS \
        --emit-inputs "$WORK/inputs" --output "$WORK/scratch.json" >/dev/null 2>&1 || \
        : > "$WORK/inputs"
    : > "$WORK/missing"
    while IFS="	" read -r subrepo file; do
        if [ -n "$file" ] && [ ! -e "$file" ]; then
            echo "$LOCK_PKG:$subrepo" >> "$WORK/missing"
        fi
    done < "$WORK/inputs"
    if [ -s "$WORK/missing" ]; then
        sort -u "$WORK/missing" > "$WORK/missing.u"
        say "building `wc -l < "$WORK/missing.u"` crates the project points at"
        # One that will not build is not fatal: everything else still resolves.
        # shellcheck disable=SC2046
        plz build $(tr '\n' ' ' < "$WORK/missing.u") >/dev/null 2>&1 || true
        # Still absent after building means it is never coming - a crate whose
        # recorded root module is not the file it ships, which is a lock to
        # fix rather than a build to retry. Said once, rather than retried
        # silently on every run for the life of the repo.
        while IFS="	" read -r subrepo file; do
            if [ -n "$file" ] && [ ! -e "$file" ]; then
                echo "$NAME: $subrepo points at $file, which building it did not produce" >&2
            fi
        done < "$WORK/inputs"
    fi
fi

if [ "$DISCOVER" = 1 ]; then
    OUT_ARG="--discover --buildfile $BUILDFILE"
else
    OUT_ARG="--output $OUT_FILE"
fi
# shellcheck disable=SC2086
"$TOOL" ide $LOCK_ARG --third-party-dir $THIRD_PARTY_DIR --sysroot $SYSROOT \
    --sysroot-src $SYSROOT_SRC --first-party $FILES $SUBARGS $OUT_ARG

if [ "$DISCOVER" = 0 ]; then
    echo "$NAME: wrote $OUT_FILE (`echo $FILES | wc -w` first-party crates, $SUBN from subrepos)"
fi
