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
