# Both names resolve to the same file. `rust_repo` is a documented subinclude
# target that consumers write, so it keeps working; go-rules keeps `go` and
# `cgo` separate for the same reason - the label is the API, not the layout.
filegroup(
    name = "rust",
    srcs = ["rust.build_defs"],
    visibility = ["PUBLIC"],
)

filegroup(
    name = "rust_repo",
    srcs = ["rust.build_defs"],
    visibility = ["PUBLIC"],
)
