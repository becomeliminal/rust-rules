#!/bin/sh
# The two libLLVM copies in one toolchain must be one inode, and rustc and
# llvm-cov must start from the split outputs without the parent toolchain on
# the loader search path. A symlink into the other tree dangles there.
set -eu

die() {
    echo "$1" >&2
    exit 1
}

inode() {
    stat -c %i "$1" 2>/dev/null || stat -f %i "$1"
}

: "${DATA_TOOLCHAIN:?}"
: "${DATA_RUSTC:?}"
: "${DATA_LLVM:?}"

found=0
for src in "$DATA_TOOLCHAIN"/rustc/lib/libLLVM.so.* "$DATA_TOOLCHAIN"/rustc/lib/libLLVM*.dylib; do
    [ -f "$src" ] && [ ! -L "$src" ] || continue
    base=$(basename "$src")
    dests=$(find "$DATA_TOOLCHAIN/llvm-tools-preview/lib/rustlib" -path "*/lib/$base" -type f)
    [ -n "$dests" ] || die "no llvm-tools copy of $base"
    src_i=$(inode "$src")
    for dest in $dests; do
        [ ! -L "$dest" ] || die "$dest is a symlink"
        dest_i=$(inode "$dest")
        [ "$dest_i" = "$src_i" ] || die "$base inodes differ ($src_i vs $dest_i)"
    done
    found=1
done
[ "$found" = 1 ] || die "no libLLVM shared library in the toolchain"

for dir in "$DATA_RUSTC/lib" "$DATA_LLVM/lib"; do
    n=0
    for src in "$dir"/libLLVM.so.* "$dir"/libLLVM*.dylib; do
        [ -e "$src" ] || continue
        [ ! -L "$src" ] || die "$src is a symlink"
        [ -f "$src" ] || die "$src is not a regular file"
        n=1
    done
    [ "$n" = 1 ] || die "no libLLVM in $dir"
done

# The parent toolchain is not on PATH or the loader path. Each binary loads
# libLLVM from ../lib next to itself.
env -u LD_LIBRARY_PATH -u LD_PRELOAD -u DYLD_LIBRARY_PATH -u DYLD_FALLBACK_LIBRARY_PATH \
    PATH=/usr/bin:/bin \
    "$DATA_RUSTC/bin/rustc" --version
env -u LD_LIBRARY_PATH -u LD_PRELOAD -u DYLD_LIBRARY_PATH -u DYLD_FALLBACK_LIBRARY_PATH \
    PATH=/usr/bin:/bin \
    "$DATA_LLVM/bin/llvm-cov" --version
