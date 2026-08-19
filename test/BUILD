subinclude("///e2e//build_defs:e2e")

# Exports the plugin as a repo archive that e2e test repos consume
exports = [
    "//:config",
    "//build_defs:rust",
    "//build_defs:rust_repo",
    # rust.build_defs and rust_repo.build_defs subinclude this for the triple
    # helpers, so it has to ship even though nobody names it directly.
    "//build_defs:triples",
]

genrule(
    name = "export",
    srcs = ["//:plugin_config"],
    cmd = "rm .plzconfig && $TOOL --noupdate export {exports} -o $TMP_DIR/_tmp && cp $SRCS $TMP_DIR/_tmp/.plzconfig && tar -czf $OUT _tmp".format(exports = " ".join(exports)),
    outs = ["rust_rules.tar.gz"],
    deps = exports,
    visibility = ["//test/..."],
    tools = [CONFIG.E2E.PLEASE_TOOL],
)
