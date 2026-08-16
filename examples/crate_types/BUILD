subinclude("//build_defs:rust")

# C-ABI shared object for FFI consumers
rust_library(
    name = "ffi_shared",
    crate_name = "ffi",
    crate_type = "cdylib",
    root = "src/lib.rs",
    edition = "2021",
)

# C-ABI static archive
rust_library(
    name = "ffi_static",
    crate_name = "ffi",
    crate_type = "staticlib",
    root = "src/lib.rs",
    edition = "2021",
)
