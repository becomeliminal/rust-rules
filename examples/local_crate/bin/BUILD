subinclude("//build_defs:rust")

rust_binary(
    name = "bin",
    main = "src/main.rs",
    deps = [
        "//examples/local_crate/package_a",
        "//examples/local_crate/package_b",
    ],
    edition = "2021",
)
