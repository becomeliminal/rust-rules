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

# The discovery half of rust_project. A real script rather than shell built by
# string concatenation: it grew a recursive sweep, and generated shell is where
# the quoting bugs live. Referenced through a config key for the same reason
# the tool is - a label in a build_defs file resolves against the repo that
# subincludes it, not the one that ships it.
filegroup(
    name = "project_script",
    srcs = ["rust_project.sh"],
    visibility = ["PUBLIC"],
)
