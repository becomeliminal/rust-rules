subinclude("//build_defs:rust")

rust_binary(
    name = "straddle_carrier_bin",
    main = "src/main.rs",
    edition = "2021",
    deps = [
        "//third_party/rust:serde",
        "//third_party/rust:serde_json",
        "//third_party/rust:clap",
        "//third_party/rust:anyhow",
        "//third_party/rust:toml",
        "//third_party/rust:regex_lite",
    ],
)

sh_test(
    name = "cli_test",
    src = "test_cli.sh",
    data = [":straddle_carrier_bin"],
    deps = [":straddle_carrier_bin"],
    labels = ["cli"],
)

filegroup(
    name = "test_data",
    srcs = glob(["test_data/**/*.build"]) + glob(["test_data/**/*.toml"]),
)

sh_test(
    name = "merge_test",
    src = "test_merge.sh",
    data = [
        ":straddle_carrier_bin",
        ":test_data",
    ],
    deps = [":straddle_carrier_bin"],
    labels = ["merge"],
)

# Test that straddle_carrier's own deps are in sync with its Cargo.toml (compatible mode)
rust_deps_check(
    name = "deps_compatible_test",
    cargo_toml = "Cargo.toml",
    third_party_build = "third_party_crates.build",
    mode = "compatible",
    straddle_carrier = ":straddle_carrier_bin",
)

# Test that straddle_carrier's own deps exactly match its Cargo.toml
rust_deps_check(
    name = "deps_exact_test",
    cargo_toml = "Cargo.toml",
    third_party_build = "third_party_crates.build",
    mode = "exact",
    straddle_carrier = ":straddle_carrier_bin",
)

# Negative test: verify should fail when dependencies are missing
sh_test(
    name = "verify_negative_test",
    src = "test_verify_negative.sh",
    data = [
        ":straddle_carrier_bin",
        ":test_data",
        "//third_party/rust:toolchain_cargo",
        "//third_party/rust:toolchain_rustc",
    ],
    labels = ["verify", "negative"],
    sandbox = False,
)

# Test that merge doesn't produce duplicate crate definitions
sh_test(
    name = "merge_no_duplicates_test",
    src = "test_merge_no_duplicates.sh",
    data = [
        ":straddle_carrier_bin",
        ":test_data",
    ],
    labels = ["merge"],
)

# Test merge with real third_party/rust/BUILD structure
sh_test(
    name = "merge_real_files_test",
    src = "test_merge_real_files.sh",
    data = [
        ":straddle_carrier_bin",
        ":test_data",
    ],
    labels = ["merge"],
)

# Test that merge preserves rust_crate_download rules
sh_test(
    name = "merge_preserves_downloads_test",
    src = "test_merge_preserves_downloads.sh",
    data = [
        ":straddle_carrier_bin",
        ":test_data",
    ],
    labels = ["merge"],
)

