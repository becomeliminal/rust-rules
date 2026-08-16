subinclude("//build_defs:rust")

rust_library(
    name = "math",
    root = "src/lib.rs",
    modules = [
        "src/add.rs",
        "src/sub.rs",
    ],
)

# rust_binary(
#     name = "main",
#     main = "src/main.rs",
#     deps = [
#         ":math",
#     ],
# )

rust_test(
    name = 'test',
    root = 'src/lib.rs',
    modules = [
        "src/add.rs",
        "src/sub.rs",
    ],  
)

rust_test(
    name = "integration_test",
    root = "tests/integration_test.rs",
    deps = [":math"],
)

rust_doc_test(
    name = "doc_test",
    crate_name = "math",
    root = "src/lib.rs",
    deps = [":math"],
    modules = [
        "src/add.rs",
        "src/sub.rs",
    ],
)

rust_clippy(
    name = "lint",
    crate_name = "math",
    root = "src/lib.rs",
    modules = [
        "src/add.rs",
        "src/sub.rs",
    ],
)

rust_fmt_test(
    name = "fmt_test",
    srcs = [
        "src/add.rs",
        "src/lib.rs",
        "src/sub.rs",
    ],
)

rust_doc(
    name = "docs",
    crate_name = "math",
    root = "src/lib.rs",
    modules = [
        "src/add.rs",
        "src/sub.rs",
    ],
)
