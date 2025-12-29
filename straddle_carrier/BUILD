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
    ],
)

sh_test(
    name = "cli_test",
    src = "test_cli.sh",
    data = [":straddle_carrier_bin"],
    deps = [":straddle_carrier_bin"],
    labels = ["cli"],
)

