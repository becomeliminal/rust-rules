subinclude("//build_defs:rust", "//build_defs:rust_repo")

# rust_repo takes a `patch` argument, which was documented and consumed by
# nothing: a consumer setting it got no patch applied and no error either.
# The crate here returns the wrong answer until the patch is applied, so this
# fails rather than passing quietly if that regresses.

genrule(
    name = "crate_src",
    srcs = glob(["crate/**"]),
    # Under a directory of its own, named for the crate and version, because
    # rust_repo names its subrepo output after the rule.
    outs = ["srcs/patched-0.1.0"],
    cmd = "mkdir -p $(dirname $OUT) && mv $PKG_DIR/crate $OUT",
)

rust_repo(
    name = "patched",
    crate = "patched",
    download = ":crate_src",
    lock = ":patch_lock",
    patch = ["fix.patch"],
    third_party_path = "test/patch",
    version = "0.1.0",
)

rust_resolve(
    name = "patch_lock",
    entries = [
        "patched|patched|0.1.0||true|true",
    ],
)

rust_test(
    name = "patch_test",
    root = "patch_test.rs",
    edition = "2021",
    deps = [":patched"],
)
