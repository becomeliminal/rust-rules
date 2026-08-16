subinclude("//build_defs:rust")

rust_library(
    name = "package_b",
    modules = [
        "src/module_one/mod.rs",
        "src/module_one/module_one.rs",
    ],
    root = "src/lib.rs",
    deps = [
        "//examples/local_crate/package_a",
    ],
    edition = "2021",
    visibility = ["PUBLIC"],
)
