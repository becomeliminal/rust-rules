subinclude("//build_defs:rust")

# What `please_rust ide` has to describe: a first-party library with a
# third-party dependency, and a binary depending on the library. Both edges
# appear in the generated rust-project.json, and rust-analyzer resolves them.

rust_library(
    name = "greeting",
    root = "src/lib.rs",
    edition = "2021",
    deps = ["//third_party/crates:anyhow"],
)

rust_test(
    name = "greeting_test",
    root = "src/lib.rs",
    edition = "2021",
    deps = ["//third_party/crates:anyhow"],
)

rust_binary(
    name = "hello",
    main = "src/main.rs",
    edition = "2021",
    deps = [":greeting"],
)

# The monorepo shape: Rust lives in one subtree, so the project file covers
# that subtree rather than the whole repo. A repo that is one Rust project
# wants the root BUILD's version instead, which takes no targets at all and
# finds everything.
#
#   plz run //examples/ide:rust-project
rust_project(
    name = "rust-project",
    lock = "//third_party/crates:rust_lock",
    out = "examples/ide/rust-project.json",
    targets = ["//examples/ide/..."],
)
