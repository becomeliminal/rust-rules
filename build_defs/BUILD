filegroup(
    name = "rust",
    srcs = [
        "rust_toolchain.build_defs",
        "rust.build_defs",
        "rust_crate.build_defs",
        "rust_repo.build_defs",
    ],
    visibility = ["PUBLIC"],
)

filegroup(
    name = "triples",
    srcs = ["rust_toolchain.build_defs"],
    visibility = ["PUBLIC"],
)

filegroup(
    name = "rust_repo",
    # rust_toolchain.build_defs holds the triple helpers rust_repo calls, and
    # this filegroup is a subinclude target in its own right.
    srcs = [
        "rust_toolchain.build_defs",
        "rust_repo.build_defs",
    ],
    visibility = ["PUBLIC"],
)

