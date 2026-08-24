subinclude("//build_defs:rust", "//build_defs:rust_repo")

# A build script can hand a path to the compiler through a custom env var
# rather than through OUT_DIR, and it records that path where it ran. That
# directory is gone by the time the crate compiles, so the crate does not
# build at all: mime_guess, which does
# include!(env!("MIME_TYPES_GENERATED_PATH")), could not be built here.

genrule(
    name = "genpath_src",
    srcs = glob(["genpath/**"]),
    outs = ["genpath-0.1.0"],
    cmd = "mv $PKG_DIR/genpath $OUT",
)

rust_repo(
    name = "genpath",
    crate = "genpath",
    download = ":genpath_src",
    lock = ":buildscript_env_lock",
    third_party_path = "test/buildscript_env",
    version = "0.1.0",
)

rust_resolve(
    name = "buildscript_env_lock",
    entries = [
        "genpath|genpath|0.1.0||true|true",
    ],
)

rust_test(
    name = "buildscript_env_test",
    root = "buildscript_env_test.rs",
    edition = "2021",
    # The fix this proves is in the tool, so a released tool from before it
    # still fails here. That is the point of the test and not a regression,
    # so the pinned-tool run skips it until a release carries the fix.
    labels = ["requires_unreleased_tool"],
    deps = [":genpath"],
)
