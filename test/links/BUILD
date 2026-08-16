subinclude("//build_defs:rust", "//build_defs:rust_repo")

# Integration test for links / DEP_<LINKS>_<KEY> metadata propagation:
# zstub (links = "zstub") exports metadata from its build script; zconsumer's
# build script asserts it arrives as DEP_ZSTUB_* env vars, and its lib
# fails to compile unless the build script confirmed receipt.

genrule(
    name = "zstub_src",
    srcs = glob(["zstub/**"]),
    outs = ["zstub-0.1.0"],
    cmd = "mv $PKG_DIR/zstub $OUT",
)

genrule(
    name = "zconsumer_src",
    srcs = glob(["zconsumer/**"]),
    outs = ["zconsumer-0.1.0"],
    cmd = "mv $PKG_DIR/zconsumer $OUT",
)

rust_repo(
    name = "zstub",
    crate = "zstub",
    version = "0.1.0",
    download = ":zstub_src",
    lock = ":links_lock",
    third_party_path = "test/links",
)

rust_repo(
    name = "zconsumer",
    crate = "zconsumer",
    version = "0.1.0",
    download = ":zconsumer_src",
    lock = ":links_lock",
    third_party_path = "test/links",
)

rust_resolve(
    name = "links_lock",
    entries = [
        "zstub|zstub|0.1.0||true|true",
        "zconsumer|zconsumer|0.1.0||true|true",
    ],
)
