subinclude("///cc//build_defs:c", "//build_defs:rust")

# --- Rust depending on C ---
c_library(
    name = "adder",
    srcs = ["adder.c"],
    hdrs = ["adder.h"],
)

rust_library(
    name = "ffi_bridge",
    root = "src_lib.rs",
    edition = "2021",
    cc_deps = [":adder"],
)

rust_binary(
    name = "uses_c",
    main = "src_main.rs",
    deps = [":ffi_bridge"],
)

rust_test(
    name = "ffi_bridge_test",
    root = "src_lib.rs",
    cc_deps = [":adder"],
)

# --- C depending on Rust ---
rust_library(
    name = "rust_ffi",
    crate_name = "ffi",
    crate_type = "staticlib",
    root = "rust_ffi_lib.rs",
    edition = "2021",
)

# The header the C side includes is generated from the Rust source rather
# than hand-written, so a signature that changes on one side stops the other
# compiling instead of crashing at run time.
rust_cbindgen(
    name = "ffi_header",
    root = "rust_ffi_lib.rs",
    crate_name = "ffi",
)

c_binary(
    name = "uses_rust",
    srcs = ["main.c"],
    hdrs = [":ffi_header"],
    deps = [":rust_ffi"],
)
