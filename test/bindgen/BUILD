subinclude("//build_defs:rust")

# End-to-end check of rust_bindgen: C header -> bindgen (built from crates,
# host libclang) -> generated bindings compiled into a rust_test.
rust_bindgen(
    name = "point_bindings",
    header = "include/point.h",
)

rust_test(
    name = "bindgen_test",
    root = "bindings_test.rs",
    modules = [":point_bindings"],
)
