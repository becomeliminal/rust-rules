subinclude("//build_defs:rust")

# A crate whose root module is generated rather than checked in. The IDE
# fragment used to record the label it was given, joined to the package, so
# the project file named a path no build could produce.
genrule(
    name = "generated_src",
    outs = ["generated.rs"],
    cmd = 'echo "pub fn answer() -> u32 { 42 }" > $OUT',
)

rust_library(
    name = "genroot",
    root = ":generated_src",
    visibility = ["PUBLIC"],
)

rust_test(
    name = "genroot_test",
    root = "genroot_test.rs",
    deps = [":genroot"],
)
