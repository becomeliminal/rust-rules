subinclude("//build_defs:rust")

# Include the config file in the build graph so `plz export` picks it up for
# the e2e tests
filegroup(
    name = "config",
    srcs = [".plzconfig"],
    visibility = ["PUBLIC"],
)

filegroup(
    name = "plugin_config",
    srcs = [".plzconfig.plugin"],
    visibility = ["PUBLIC"],
)

# The crate graph for rust-analyzer, covering this repo's own Rust. Build it
# and put it at the root, which is where rust-analyzer looks and what the
# paths inside are relative to:
#
#   plz build //:rust-project && ln -sf plz-out/gen/rust-project.json .
rust_project(
    name = "rust-project",
    lock = "//third_party/crates:rust_lock",
    deps = [
        "//examples/ide:greeting",
        "//examples/ide:hello",
        "//tools/please_rust:please_rust",
    ],
)
