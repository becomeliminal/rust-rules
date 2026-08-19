#!/bin/sh
# Discovery for rust_project: find every Rust crate plz knows about, in this
# repo and in the subrepos it pulls in, and describe them for rust-analyzer.
#
# The rule prepends a prelude setting NAME, TOOL_LABEL, TARGETS, EXCLUDES,
# SUBREPOS, LOCK_LABEL, THIRD_PARTY_DIR, SYSROOT, SYSROOT_SRC and OUT_FILE.
# Values are never substituted into the shell text, so no path or label can be
# read as syntax.
set -eu
cd "`plz query reporoot`"

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
    sweep_sub=$1
    sweep_root=$2
    sweep_maxdepth=$3
    printf '%s\n' "" > "$WORK/level"
    sweep_depth=0
    sweep_budget=200
    while [ -s "$WORK/level" ]; do
        : > "$WORK/next"
        while IFS= read -r sweep_path; do
            if [ "$sweep_budget" -le 0 ]; then break; fi
            sweep_budget=`expr $sweep_budget - 1`
            if [ -n "$sweep_path" ]; then
                sweep_pat="///$sweep_sub//$sweep_path/..."
            else
                sweep_pat="///$sweep_sub//..."
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

# This repo's own crates.
FRAGS=`plz query alltargets $TARGETS --include rust_ide --hidden || true`
FRAGS=`drop_excluded "$FRAGS"`
if [ -z "$FRAGS" ]; then
    echo "$NAME: no Rust crates found under $TARGETS" >&2
    exit 1
fi
plz build $TOOL_LABEL $LOCK_LABEL $FRAGS >/dev/null
TOOL=`plz query outputs $TOOL_LABEL`
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
    # A plugin subrepo that is not a Rust one - the cc, shell or proto rules -
    # has nothing to find, and descending it on a failed parse is pure cost.
    # One find answers it.
    if [ -n "$root" ]; then
        if [ -z "`find "$root" -name '*.rs' -print -quit 2>/dev/null`" ]; then
            continue
        fi
    fi
    : > "$WORK/found"
    sweep "$sub" "$root" 3
    drop_excluded "`cat "$WORK/found"`" > "$WORK/frags"
    while read -r frag; do
        if [ -z "$frag" ]; then continue; fi
        # A subrepo resolving back to this repo - what a plugin repo declaring
        # itself for its own tests does - returns host labels. Describing those
        # again is every crate here twice.
        case "$frag" in ///*) ;; *) continue ;; esac
        plz build "$frag" >/dev/null 2>&1 || continue
        # plz reports a subrepo target's inputs relative to *this* repo, which
        # is the rebasing; the fragment's own root is relative to the subrepo.
        real=`plz query input "$frag" 2>/dev/null | head -1` || continue
        json=`plz query outputs "$frag" 2>/dev/null | head -1` || continue
        if [ -n "$real" ] && [ -n "$json" ]; then
            echo "--subrepo-crate $real=$json" >> "$WORK/subargs"
        fi
    done < "$WORK/frags"
done < "$WORK/subrepos"

SUBARGS=`tr '\n' ' ' < "$WORK/subargs"`
SUBN=`wc -l < "$WORK/subargs"`

while read -r s; do
    echo "$NAME: could not parse $s, so any crates in it are not described" >&2
done < "$WORK/skipped"

# shellcheck disable=SC2086
"$TOOL" ide $LOCK_ARG --third-party-dir $THIRD_PARTY_DIR --sysroot $SYSROOT \
    --sysroot-src $SYSROOT_SRC --first-party $FILES $SUBARGS --output $OUT_FILE
echo "$NAME: wrote $OUT_FILE (`echo $FILES | wc -w` first-party crates, `echo $SUBN` from subrepos)"
