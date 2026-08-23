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

# The crate graph for rust-analyzer. Run it - `plz run //:rust-project` - and
# it finds every Rust crate in the repo and writes rust-project.json at the
# root. Nothing to keep in step: a crate added anywhere is in the next run.
rust_project(
    name = "rust-project",
    # Every rust_resolve in the repo. The test packages declare their own so
    # that what they are testing is isolated, and a crate that depends on one
    # of those resolves in the editor only if its lock is here too.
    lock = [
        "//third_party/crates:rust_lock",
        "//test/firstparty:firstparty_lock",
        "//test/links:links_lock",
        "//test/patch:patch_lock",
    ],
)
