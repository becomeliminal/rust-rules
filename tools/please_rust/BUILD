subinclude("//build_defs:rust")

# Bootstrap target that builds please_rust using cargo directly.
# This is similar to how go-rules bootstraps please_go.
# Once built, this can be used as the tool for rust_repo.
genrule(
    name = "bootstrap",
    srcs = glob(["src/**/*.rs"]) + ["Cargo.toml", "Cargo.lock"],
    outs = ["please_rust"],
    binary = True,
    # Build using cargo with the Please-managed toolchain
    cmd = """
        set -e

        # Set up environment for cargo to use our toolchain
        export CARGO_HOME=$TMP_DIR/.cargo
        export RUSTC=$TOOLS_RUSTC
        export RUSTFLAGS="--sysroot $TOOLS_SYSROOT"

        # Build release binary from the source directory
        $TOOLS_CARGO build --release --manifest-path tools/please_rust/Cargo.toml --target-dir $TMP_DIR/target 2>&1

        # Copy the binary to output
        cp $TMP_DIR/target/release/please_rust $OUTS
    """,
    sandbox = False,  # Needed for cargo to download dependencies
    tools = {
        "rustc": ["//third_party/rust:toolchain_rustc"],
        "sysroot": ["//third_party/rust:toolchain_sysroot"],
        "cargo": ["//third_party/rust:toolchain_cargo"],
    },
    visibility = ["PUBLIC"],
)

# Version file for packaging
genrule(
    name = "version",
    srcs = ["VERSION"],
    outs = ["version.build_defs"],
    cmd = "echo PLEASE_RUST_VERSION = \\\"$(cat $SRCS)\\\" > $OUT",
    visibility = ["PUBLIC"],
)
