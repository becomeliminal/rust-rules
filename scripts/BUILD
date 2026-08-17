subinclude("///shell//build_defs:shell")

sh_binary(
    name = "rustc",
    main = "rustc.sh",
    deps = ["//third_party/rust:toolchain"],
    visibility=["PUBLIC"],
)

sh_binary(
    name = "cargo",
    main = "cargo.sh",
    deps = ["//third_party/rust:toolchain"],
    visibility=["PUBLIC"],
)

# sh_binary(
#     name = "cargo_nightly",
#     main = "cargo_nightly.sh",
#     deps = ["//third_party/rust:toolchain_nightly_cargo"],
#     visibility=["PUBLIC"],
# )
