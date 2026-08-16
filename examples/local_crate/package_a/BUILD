subinclude("//build_defs:rust")

rust_library(
    name = "package_a",
    modules = [
        "src/module_a/mod.rs",
        "src/module_a/module_a.rs",
        "src/module_b/mod.rs",
        "src/module_b/module_b.rs",
    ],
    root = "src/lib.rs",
    edition = "2021",
    visibility = ["PUBLIC"],
)
