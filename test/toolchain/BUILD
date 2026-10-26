subinclude("///shell//build_defs:shell")

sh_test(
    name = "layout_test",
    src = "layout_test.sh",
    data = {
        "toolchain": ["//third_party/rust:toolchain"],
        "rustc": ["//third_party/rust:toolchain_rustc"],
        "llvm": ["//third_party/rust:toolchain_llvm_tools"],
    },
)
