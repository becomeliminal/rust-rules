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

# plz build //examples/ide:rust-project && ln -sf plz-out/gen/examples/ide/rust-project.json .
rust_project(
    name = "rust-project",
    lock = "//third_party/crates:rust_lock",
    deps = [
        ":greeting",
        ":hello",
    ],
)
