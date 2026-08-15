subinclude("//build_defs:rust")

rust_toolchain(
    name = "toolchain",
    hashes = ["0a4022f567602bd6e9cc35e5b0a9b1a88f3867e6"],
    version = "1.82.0",
    visibility = ["PUBLIC"],
)

# rust_toolchain(
#     name = "toolchain_nightly",
#     # The hash changes all the time.
#     hashes = ["af79758278cfe385ab0d4520f2c64f4ae7f9c7c6"],
#     version = "nightly",
#     visibility = ["PUBLIC"],
# )

# MIGRATED TO rust_repo - see end of file
# #### Used by examples/rustc_guessing_game ####
# rust_crate(
#     name="libc",
#     crate="libc",
#     version="0.2.155",
#     edition="2015",
#     features = [
#         "default",
#         "std",
#     ],
#     build_root="build.rs",
# )
#
# rust_crate(
#     name="cfg_if",
#     crate="cfg-if",
#     version="1.0.0",
#     edition="2018",
# )
#
# rust_crate(
#     name="ppv_lite86",
#     crate="ppv-lite86",
#     version="0.2.17",
#     edition="2018",
#     features=["simd", "std"]
# )
#
# rust_crate(
#     name="getrandom",
#     crate="getrandom",
#     version="0.2.15",
#     edition="2018",
#     deps=[
#         ":cfg_if",
#         ":libc",
#     ],
#     features=["std"],
# )
#
# rust_crate(
#     name="rand_core",
#     crate="rand_core",
#     version="0.6.4",
#     edition="2018",
#     features=["alloc", "getrandom", "std"],
#     deps = [":getrandom"],
# )
#
# rust_crate(
#     name="rand_chacha",
#     crate="rand_chacha",
#     version="0.3.1",
#     edition="2018",
#     deps=[
#         ":ppv_lite86",
#         ":rand_core",
#     ],
#     features=["std"],
# )
#
# rust_crate(
#     name="rand",
#     crate="rand",
#     version="0.8.5",
#     edition="2018",
#     features=["alloc", "default", "getrandom", "libc", "rand_chacha", "small_rng", "std", "std_rng"],
#     deps=[
#         ":libc",
#         ":rand_chacha",
#         ":rand_core",
#     ],
# )

#### Used by examples/rustc_calculator ####
# MIGRATED TO rust_repo - see end of file
# rust_crate(
#     name = "unicode_ident",
#     crate = "unicode-ident",
#     edition = "2018",
#     version = "1.0.12",
# )

# MIGRATED TO rust_repo - see end of file
# rust_crate(
#     name = "proc_macro2",
#     build_root = "build.rs",
#     crate = "proc-macro2",
#     edition = "2021",
#     features = ["default", "proc-macro"],
#     version = "1.0.85",
#     deps = [":unicode_ident"],
# )

# MIGRATED TO rust_repo - see end of file
# rust_crate(
#     name = "autocfg",
#     crate = "autocfg",
#     edition = "2015",
#     version = "1.3.0",
# )
#
# rust_crate(
#     name = "pin_project_lite",
#     ...
# )
#
# rust_crate(
#     name = "bytes",
#     ...
# )
# ... (remaining crates migrated to rust_repo at end of file)

# MIGRATED TO rust_repo - see end of file
# rust_crate(
#     name = "quote",
#     crate = "quote",
#     edition = "2018",
#     version = "1.0.36",
#     features = [
#         "default",
#         "proc-macro",
#     ],
#     deps = [
#         ":proc_macro2",
#     ],
# )

# MIGRATED TO rust_repo
# rust_crate(
#     name = "futures_task",
#     crate = "futures-task",
#     edition = "2018",
#     version = "0.3.30",
#     features = [
#         "alloc",
#     ],
# )

# rust_crate(
#     name = "itoa",
#     crate = "itoa",
#     edition = "2018",
#     version = "1.0.11",
# )

# rust_crate(
#     name = "http",
#     crate = "http",
#     edition = "2018",
#     version = "0.2.12",
#     deps = [
#         ":bytes",
#         ":fnv",
#         ":itoa",
#     ],
# )

# MIGRATED TO rust_repo - see end of file
# rust_crate(
#     name = "syn",
#     crate = "syn",
#     edition = "2021",
#     version = "2.0.87",
#     features = [
#         "clone-impls",
#         "default",
#         "derive",
#         "extra-traits",
#         "full",
#         "parsing",
#         "printing",
#         "proc-macro",
#         "visit-mut",
#     ],
#     deps = [
#         ":proc_macro2",
#         ":quote",
#         ":unicode_ident",
#     ],
# )

# MIGRATED TO rust_repo - all remaining rust_crate definitions below are commented out
# See the rust_repo section at the end of this file for the new definitions

# rust_crate(
#     name = "log",
#     ...
# )
#
# ... (all remaining rust_crate definitions removed - using rust_repo at end of file)

# Migrated to rust_repo below
#rust_crate(
#    name = "multimap",
#    crate = "multimap",
#    version = "0.10.0",
#    edition = "2015",
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "tower_layer",
#    crate = "tower-layer",
#    version = "0.3.2",
#    edition = "2018",
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "regex_automata",
#    crate = "regex-automata",
#    version = "0.4.7",
#    edition = "2021",
#    features = [
#        "alloc",
#        "meta",
#        "nfa-pikevm",
#        "nfa-thompson",
#        "std",
#        "syntax",
#        "unicode-bool",
#    ],
#    deps = [
#        ":regex_syntax",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "mime",
#    crate = "mime",
#    version = "0.3.17",
#    edition = "2015",
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "tempfile",
#    crate = "tempfile",
#    version = "3.10.1",
#    edition = "2021",
#    deps = [
#        ":cfg_if",
#        ":fastrand",
#        ":rustix",
#    ],
#)

# MIGRATED TO rust_repo - see end of file
# # must match the serde_derive version, see:
# # https://github.com/serde-rs/serde/issues/2584#issuecomment-1685252251
# rust_crate(
#     name = "serde",
#     build_root = "build.rs",
#     crate = "serde",
#     edition = "2018",
#     features = [
#         "default",
#         "derive",
#         "serde_derive",
#         "std",
#     ],
#     version = "1.0.215",
#     deps = [
#         ":serde_derive",
#     ],
# )

# MIGRATED TO rust_repo - see end of file
# # must match the serde version, see:
# # https://github.com/serde-rs/serde/issues/2584#issuecomment-1685252251
# rust_crate(
#     name = "serde_derive",
#     crate = "serde_derive",
#     crate_type = "proc-macro",
#     edition = "2018",
#     features = [
#         "default",
#     ],
#     version = "1.0.215",
#     deps = [
#         ":proc_macro2",
#         ":quote",
#         ":syn",
#     ],
# )

# Migrated to rust_repo below
#rust_crate(
#    name = "matchit",
#    crate = "matchit",
#    version = "0.7.3",
#    edition = "2021",
#    features = [
#        "default",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "percent_encoding",
#    crate = "percent-encoding",
#    version = "2.3.1",
#    edition = "2018",
#    features = [
#        "alloc",
#        "default",
#        "std",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "sync_wrapper",
#    crate = "sync_wrapper",
#    version = "0.1.2",
#    edition = "2018",
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "memchr",
#    crate = "memchr",
#    version = "2.7.4",
#    edition = "2021",
#    features = [
#        "alloc",
#        "default",
#        "std",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "base64",
#    crate = "base64",
#    version = "0.21.7",
#    edition = "2018",
#    features = [
#        "alloc",
#        "default",
#        "std",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "regex",
#    crate = "regex",
#    version = "1.10.5",
#    edition = "2021",
#    features = [
#        "std",
#        "unicode-bool",
#    ],
#    deps = [
#        ":regex_automata",
#        ":regex_syntax",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "tokio_macros",
#    crate = "tokio-macros",
#    version = "2.3.0",
#    edition = "2021",
#    crate_type = "proc-macro",
#    deps = [
#        ":proc_macro2",
#        ":quote",
#        ":syn",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "prost_derive",
#    crate = "prost-derive",
#    version = "0.12.6",
#    edition = "2021",
#    crate_type = "proc-macro",
#    deps = [
#        ":anyhow",
#        ":itertools",
#        ":proc_macro2",
#        ":quote",
#        ":syn",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "tracing_attributes",
#    crate = "tracing-attributes",
#    version = "0.1.27",
#    edition = "2018",
#    crate_type = "proc-macro",
#    deps = [
#        ":proc_macro2",
#        ":quote",
#        ":syn",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "pin_project_internal",
#    crate = "pin-project-internal",
#    version = "1.1.5",
#    edition = "2021",
#    crate_type = "proc-macro",
#    deps = [
#        ":proc_macro2",
#        ":quote",
#        ":syn",
#    ],
#    warns = [
#        "unreachable_pub",
#        "clippy::undocumented_unsafe_blocks",
#        "clippy::transmute_undefined_repr",
#        "clippy::trailing_empty_array",
#        "single_use_lifetimes",
#        "rust_2018_idioms",
#        "clippy::pedantic",
#        "non_ascii_idents",
#        "clippy::inline_asm_x86_att_syntax",
#        "improper_ctypes_definitions",
#        "improper_ctypes",
#        "clippy::default_union_representation",
#        "clippy::as_ptr_cast_mut",
#        "clippy::all",
#    ],
#    allows = [
#        "clippy::type_complexity",
#        "clippy::too_many_lines",
#        "clippy::too_many_arguments",
#        "clippy::struct_field_names",
#        "clippy::struct_excessive_bools",
#        "clippy::single_match_else",
#        "clippy::single_match",
#        "clippy::similar_names",
#        "clippy::module_name_repetitions",
#        "clippy::missing_errors_doc",
#        "clippy::manual_range_contains",
#        "clippy::manual_assert",
#        "clippy::float_cmp",
#        "clippy::doc_markdown",
#        "clippy::declare_interior_mutable_const",
#        "clippy::borrow_as_ptr",
#        "clippy::bool_assert_comparison",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "async_trait",
#    crate = "async-trait",
#    version = "0.1.80",
#    edition = "2021",
#    crate_type = "proc-macro",
#    deps = [
#        ":proc_macro2",
#        ":quote",
#        ":syn",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "axum_core",
#    crate = "axum-core",
#    version = "0.3.4",
#    edition = "2021",
#    deps = [
#        ":async_trait",
#        ":bytes",
#        ":futures_util",
#        ":http",
#        ":http_body",
#        ":mime",
#        ":tower_layer",
#        ":tower_service",
#    ],
#    build_root = "build.rs",
#    build_deps = [
#        ":rustversion",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "async_stream",
#    crate = "async-stream",
#    version = "0.3.6",
#    edition = "2018",
#    crate_type = "lib",
#    deps = [
#        ":async_stream_impl",
#        ":futures_core",
#        ":pin_project_lite",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "async_stream_impl",
#    crate = "async-stream-impl",
#    version = "0.3.6",
#    edition = "2018",
#    crate_type = "proc-macro",
#    deps = [
#        ":proc_macro2",
#        ":quote",
#        ":syn",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "tokio",
#    crate = "tokio",
#    version = "1.38.0",
#    edition = "2021",
#    crate_type = "lib",
#    features = [
#        "bytes", "default", "fs", "full", "io-std", "io-util", "libc", "macros", "mio",
#        "net", "num_cpus", "parking_lot", "process", "rt", "rt-multi-thread", "signal",
#        "signal-hook-registry", "socket2", "sync", "time", "tokio-macros",
#    ],
#    deps = [
#        ":bytes",
#        ":libc",
#        ":mio",
#        ":num_cpus",
#        ":parking_lot",
#        ":pin_project_lite",
#        ":signal_hook_registry",
#        ":socket2",
#        ":tokio_macros",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "pin_project",
#    crate = "pin-project",
#    version = "1.1.5",
#    edition = "2021",
#    crate_type = "lib",
#    warns = [
#        "unreachable_pub", "clippy::undocumented_unsafe_blocks", "clippy::transmute_undefined_repr",
#        "clippy::trailing_empty_array", "single_use_lifetimes", "rust_2018_idioms", "clippy::pedantic",
#        "non_ascii_idents", "clippy::inline_asm_x86_att_syntax", "improper_ctypes_definitions",
#        "improper_ctypes", "clippy::default_union_representation", "clippy::as_ptr_cast_mut",
#        "clippy::all",
#    ],
#    allows = [
#        "clippy::type_complexity", "clippy::too_many_lines", "clippy::too_many_arguments",
#        "clippy::struct_field_names", "clippy::struct_excessive_bools", "clippy::single_match_else",
#        "clippy::single_match", "clippy::similar_names", "clippy::module_name_repetitions",
#        "clippy::missing_errors_doc", "clippy::manual_range_contains", "clippy::manual_assert",
#        "clippy::float_cmp", "clippy::doc_markdown", "clippy::declare_interior_mutable_const",
#        "clippy::borrow_as_ptr", "clippy::bool_assert_comparison",
#    ],
#    deps = [
#        ":pin_project_internal",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "tracing",
#    crate = "tracing",
#    version = "0.1.40",
#    edition = "2018",
#    crate_type = "lib",
#    features = [
#        "attributes", "default", "std", "tracing-attributes",
#    ],
#    deps = [
#        ":pin_project_lite",
#        ":tracing_attributes",
#        ":tracing_core",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "prost",
#    crate = "prost",
#    version = "0.12.6",
#    edition = "2021",
#    crate_type = "lib",
#    features = [
#        "derive", "prost-derive", "std",
#    ],
#    deps = [
#        ":bytes",
#        ":prost_derive",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "prost_types",
#    crate = "prost-types",
#    version = "0.12.6",
#    edition = "2021",
#    crate_type = "lib",
#    deps = [
#        ":prost",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "prost_build",
#    crate = "prost-build",
#    version = "0.12.6",
#    edition = "2021",
#    crate_type = "lib",
#    features = [
#        "default", "format",
#    ],
#    deps = [
#        ":bytes",
#        ":heck",
#        ":itertools",
#        ":log",
#        ":multimap",
#        ":once_cell",
#        ":petgraph",
#        ":prettyplease",
#        ":prost",
#        ":prost_types",
#        ":regex",
#        ":syn",
#        ":tempfile",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "tonic_build",
#    crate = "tonic-build",
#    version = "0.11.0",
#    edition = "2021",
#    crate_type = "lib",
#    features = [
#        "default", "prost", "prost-build", "transport",
#    ],
#    deps = [
#        ":prettyplease",
#        ":proc_macro2",
#        ":prost_build",
#        ":quote",
#        ":syn",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "tokio_util",
#    crate = "tokio-util",
#    version = "0.7.11",
#    edition = "2021",
#    crate_type = "lib",
#    features = [
#        "codec", "default", "io",
#    ],
#    deps = [
#        ":bytes",
#        ":futures_core",
#        ":futures_sink",
#        ":pin_project_lite",
#        ":tokio",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "tokio_io_timeout",
#    crate = "tokio-io-timeout",
#    version = "1.2.0",
#    edition = "2018",
#    crate_type = "lib",
#    deps = [
#        ":pin_project_lite",
#        ":tokio",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "tokio_stream",
#    crate = "tokio-stream",
#    version = "0.1.15",
#    edition = "2021",
#    crate_type = "lib",
#    features = [
#        "default", "time",
#    ],
#    deps = [
#        ":futures_core",
#        ":pin_project_lite",
#        ":tokio",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "h2",
#    crate = "h2",
#    version = "0.3.26",
#    edition = "2018",
#    crate_type = "lib",
#    deps = [
#        ":bytes",
#        ":fnv",
#        ":futures_core",
#        ":futures_sink",
#        ":futures_util",
#        ":http",
#        ":indexmap",
#        ":slab",
#        ":tokio",
#        ":tokio_util",
#        ":tracing",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "tower",
#    crate = "tower",
#    version = "0.4.13",
#    edition = "2018",
#    crate_type = "lib",
#    features = [
#        "__common", "balance", "buffer", "discover", "futures-core", "futures-util", "indexmap",
#        "limit", "load", "make", "pin-project", "pin-project-lite", "rand", "ready-cache", "slab",
#        "timeout", "tokio", "tokio-util", "tracing", "util",
#    ],
#    deps = [
#        ":futures_core",
#        ":futures_util",
#        # https://github.com/indexmap-rs/indexmap/issues/289
#        #":indexmap.1.9.3",
#        ":indexmap.1.2.0",
#        ":pin_project",
#        ":pin_project_lite",
#        ":rand",
#        ":slab",
#        ":tokio",
#        ":tokio_util",
#        ":tower_layer",
#        ":tower_service",
#        ":tracing",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "hyper",
#    crate = "hyper",
#    version = "0.14.29",
#    edition = "2018",
#    crate_type = "lib",
#    features = [
#        "client", "default", "full", "h2", "http1", "http2", "runtime", "server", "socket2",
#        "stream", "tcp",
#    ],
#    deps = [
#        ":bytes",
#        ":futures_channel",
#        ":futures_core",
#        ":futures_util",
#        ":h2",
#        ":http",
#        ":http_body",
#        ":httparse",
#        ":httpdate",
#        ":itoa",
#        ":pin_project_lite",
#        ":socket2",
#        ":tokio",
#        ":tower_service",
#        ":tracing",
#        ":want",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "axum",
#    crate = "axum",
#    version = "0.6.20",
#    edition = "2021",
#    crate_type = "lib",
#    deps = [
#        ":async_trait",
#        ":axum_core",
#        ":bitflags.1.3.2",
#        ":bytes",
#        ":futures_util",
#        ":http",
#        ":http_body",
#        ":hyper",
#        ":itoa",
#        ":matchit",
#        ":memchr",
#        ":mime",
#        ":percent_encoding",
#        ":pin_project_lite",
#        ":serde",
#        ":sync_wrapper",
#        ":tower",
#        ":tower_layer",
#        ":tower_service",
#    ],
#    build_root = "build.rs",
#    build_deps = [
#        ":rustversion",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "hyper_timeout",
#    crate = "hyper-timeout",
#    version = "0.4.1",
#    edition = "2018",
#    crate_type = "lib",
#    deps = [
#        ":hyper",
#        ":pin_project_lite",
#        ":tokio",
#        ":tokio_io_timeout",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "tonic",
#    crate = "tonic",
#    version = "0.11.0",
#    edition = "2021",
#    crate_type = "lib",
#    features = [
#        "channel",
#        "codegen",
#        "default",
#        "prost",
#        "transport",
#    ],
#    deps = [
#        ":async_stream",
#        ":async_trait",
#        ":axum",
#        ":base64",
#        ":bytes",
#        ":h2",
#        ":http",
#        ":http_body",
#        ":hyper",
#        ":hyper_timeout",
#        ":percent_encoding",
#        ":pin_project",
#        ":prost",
#        ":tokio",
#        ":tokio_stream",
#        ":tower",
#        ":tower_layer",
#        ":tower_service",
#        ":tracing",
#    ],
#)

#### rust-proto examples ####

rust_crate(
    name = "once_cell.1.10.0",
    crate = "once_cell",
    edition = "2021",
    version = "1.10.0",
    # crate_name = "once_cell",
)

rust_crate_download(
    name = "protoc_gen_prost_dl",
    crate = "protoc-gen-prost",
    version = "0.3.1",
)

rust_crate(
    name = "protoc_gen_prost_lib",
    crate = "protoc-gen-prost",
    edition = "2021",
    download = ":protoc_gen_prost_dl",
    version = "0.3.1",
    features = [],
    deps = [
        ":once_cell",
        ":prost",
        ":prost_build",
        ":prost_types",
        ":regex",
    ],
)

# Migrated to rust_repo below
#rust_crate(
#    name = "protoc_gen_prost",
#    crate = "protoc-gen-prost",
#    edition = "2021",
#    download = ":protoc_gen_prost_dl",
#    version = "0.3.1",
#    features = [],
#    crate_type = 'bin',
#    src_root = 'src/main.rs',
#    deps = [
#        ":prost",
#        ":protoc_gen_prost_lib",
#    ],
#)


rust_crate_download(
    name = "protoc_gen_tonic_dl",
    crate = "protoc-gen-tonic",
    version = "0.4.0",
)

rust_crate(
    name = "protoc_gen_tonic_lib",
    crate = "protoc-gen-tonic",
    download = ":protoc_gen_tonic_dl",
    edition = "2021",
    features = [],
    version = "0.4.0",
    deps = [
        ":heck",
        ":prettyplease",
        ":prost_build",
        ":prost_types",
        ":prost",
        ":protoc_gen_prost_lib",
        ":regex",
        ":syn",
        ":tonic_build",
    ],
)

# Migrated to rust_repo below
#rust_crate(
#    name = "protoc_gen_tonic",
#    crate = "protoc-gen-tonic",
#    crate_type = "bin",
#    download = ":protoc_gen_tonic_dl",
#    edition = "2021",
#    features = [],
#    src_root = "src/main.rs",
#    version = "0.4.0",
#    deps = [
#        ":prost",
#        ":protoc_gen_prost_lib",
#        ":protoc_gen_tonic_lib",
#    ],
#)

## Test build scripts with external deps.

# Migrated to rust_repo below
#rust_crate(
#    name = "libsecp256k1",
#    crate = "libsecp256k1",
#    edition = "2018",
#    version = "0.6.0",
#    build_root = "build.rs",
#    features = [
#        "default",
#    ],
#    deps = [
#        ":arrayref",
#        ":base64",
#        ":digest.0.9.0",
#        ":libsecp256k1_core",
#        ":rand",
#        ":serde",
#    ],
#    build_deps = [
#        ":libsecp256k1_gen_ecmult",
#        ":libsecp256k1_gen_genmult",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "arrayref",
#    crate = "arrayref",
#    edition = "2021",
#    version = "0.3.9",
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "libsecp256k1_core",
#    crate = "libsecp256k1-core",
#    edition = "2018",
#    version = "0.2.2",
#    deps = [
#        ":crunchy",
#        ":digest.0.9.0",
#        ":subtle",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "crunchy",
#    crate = "crunchy",
#    edition = "2021",
#    version = "0.2.2",
#    build_root = "build.rs",
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "subtle",
#    crate = "subtle",
#    edition = "2021",
#    version = "2.6.1",
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "digest",
#    crate = "digest",
#    edition = "2018",
#    version = "0.10.7",
#    features = [
#        "default",
#        "core-api",
#        "block-buffer",
#    ],
#    deps = [
#        ":crypto_common",
#        ":block_buffer",
#    ],
#)

rust_crate(
    name = "digest.0.9.0",
    crate = "digest",
    edition = "2018",
    version = "0.9.0",
    features = [
        "std",
    ],
    deps = [
        ":generic_array",
    ],
)

# Migrated to rust_repo below
#rust_crate(
#    name = "block_buffer",
#    crate = "block-buffer",
#    edition = "2018",
#    version = "0.10.4",
#    deps = [
#        ":generic_array",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "crypto_common",
#    crate = "crypto-common",
#    edition = "2018",
#    version = "0.1.6",
#    deps = [
#        ":generic_array",
#        ":typenum",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "generic_array",
#    crate = "generic-array",
#    edition = "2018",
#    version = "0.14.7",
#    deps = [
#        ":typenum",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "typenum",
#    crate = "typenum",
#    build_root = "build/main.rs",
#    edition = "2018",
#    version = "1.17.0",
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "libsecp256k1_gen_ecmult",
#    crate = "libsecp256k1-gen-ecmult",
#    edition = "2018",
#    version = "0.3.0",
#    deps = [
#        ":libsecp256k1_core",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "libsecp256k1_gen_genmult",
#    crate = "libsecp256k1-gen-genmult",
#    edition = "2018",
#    version = "0.3.0",
#    deps = [
#        ":libsecp256k1_core",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "blake3",
#    crate = "blake3",
#    edition = "2021",
#    version = "1.5.4",
#    features = [
#        "default",
#        "std",
#    ],
#    deps = [
#        ":arrayref",
#        ":arrayvec",
#        ":cfg_if",
#        ":constant_time_eq",
#    ],
#    build_root = "build.rs",
#    build_deps = [
#        ":cc",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "arrayvec",
#    crate = "arrayvec",
#    edition = "2021",
#    version = "0.7.6",
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "constant_time_eq",
#    crate = "constant_time_eq",
#    edition = "2021",
#    version = "0.3.1",
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "cc",
#    crate = "cc",
#    edition = "2021",
#    version = "1.2.0",
#    deps = [
#        ":shlex",
#        # ":libc",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "shlex",
#    crate = "shlex",
#    edition = "2021",
#    version = "1.3.0",
#    features = [
#        "std",
#    ],
#)

#### BENCHMARKING
# Migrated to rust_repo below
#rust_crate(
#    name = "criterion",
#    crate = "criterion",
#    edition = "2021",
#    version = "0.5.1",
#    features = [
#        "html_reports",
#        "csv_reports",
#    ],
#    deps = [
#        ":anes",
#        ":cast",
#        ":ciborium",
#        ":clap",
#        ":criterion_plot",
#        ":is_terminal",
#        ":itertools",
#        ":num_traits",
#        ":once_cell",
#        ":oorandom",
#        ":regex",
#        ":serde",
#        ":serde_derive",
#        ":serde_json",
#        ":tinytemplate",
#        ":walkdir",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "anes",
#    crate = "anes",
#    edition = "2021",
#    version = "0.1.6",
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "cast",
#    crate = "cast",
#    edition = "2021",
#    version = "0.3.0",
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "ciborium",
#    crate = "ciborium",
#    edition = "2021",
#    version = "0.2.2",
#    features = [
#        "std",
#    ],
#    deps = [
#        ":ciborium_io",
#        ":ciborium_ll",
#        ":serde",
#    ]
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "ciborium_io",
#    crate = "ciborium-io",
#    edition = "2021",
#    version = "0.2.2",
#    features = [
#        "std",
#        "default",
#        "alloc",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "ciborium_ll",
#    crate = "ciborium-ll",
#    edition = "2021",
#    version = "0.2.2",
#    deps = [
#        ":ciborium_io",
#        ":half",
#    ]
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "half",
#    crate = "half",
#    edition = "2021",
#    version = "2.4.1",
#    deps = [
#        ":cfg_if",
#        ":crunchy",
#    ]
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "clap",
#    crate = "clap",
#    edition = "2021",
#    version = "4.4.9",
#    features = [
#        "std",
#        "help",
#    ],
#    deps = [
#        ":clap_builder",
#    ]
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "clap_builder",
#    crate = "clap_builder",
#    edition = "2021",
#    version = "4.5.21",
#    features = [
#        "std",
#    ],
#    deps = [
#        ":anstyle",
#        ":clap_lex",
#    ]
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "anstyle",
#    crate = "anstyle",
#    edition = "2021",
#    version = "1.0.10",
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "clap_lex",
#    crate = "clap_lex",
#    edition = "2021",
#    version = "0.7.3",
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "criterion_plot",
#    crate = "criterion-plot",
#    edition = "2021",
#    version = "0.5.0",
#    deps = [
#        ":cast",
#        ":itertools",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "is_terminal",
#    crate = "is-terminal",
#    edition = "2021",
#    version = "0.4.13",
#    deps = [
#        ":hermit_abi",
#        ":libc",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "hermit_abi",
#    crate = "hermit-abi",
#    edition = "2021",
#    version = "0.4.0",
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "num_traits",
#    crate = "num-traits",
#    edition = "2018",
#    features = [
#        "std",
#        "default",
#        "i128",
#    ],
#    version = "0.2.19",
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "oorandom",
#    crate = "oorandom",
#    edition = "2021",
#    version = "11.1.4",
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "serde_json",
#    crate = "serde_json",
#    edition = "2021",
#    version = "1.0.133",
#    features = [
#        "std",
#    ],
#    deps = [
#        ":itoa",
#        ":memchr",
#        ":ryu",
#        ":serde",
#    ],
#    build_root = "build.rs",
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "ryu",
#    crate = "ryu",
#    edition = "2021",
#    version = "1.0.18",
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "tinytemplate",
#    crate = "tinytemplate",
#    edition = "2015",
#    version = "1.2.1",
#    deps = [
#        ":serde",
#        ":serde_json",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "walkdir",
#    crate = "walkdir",
#    edition = "2021",
#    version = "2.5.0",
#    deps = [
#        ":same_file",
#    ],
#)

# Migrated to rust_repo below
#rust_crate(
#    name = "same_file",
#    crate = "same-file",
#    edition = "2021",
#    version = "1.0.6",
#)

################################################################################
# rust_repo-based crates (subrepo architecture)
# These replace the rust_crate definitions above
# Names match the old rust_crate names so other crates can still use `:serde`, etc.
################################################################################

subinclude("//build_defs:rust_repo")

# Serde chain - using rust_repo instead of rust_crate

# Leaf dependency - no deps
rust_repo(
    name = "unicode_ident",
    crate = "unicode-ident",
    version = "1.0.12",
)

# Has build.rs (ignored for now - works without it for most use cases)
rust_repo(
    name = "proc_macro2",
    crate = "proc-macro2",
    version = "1.0.85",
    features = ["default", "proc-macro"],
)

rust_repo(
    name = "quote",
    crate = "quote",
    version = "1.0.36",
    features = ["default", "proc-macro"],
)

# Note: syn has many features enabled
rust_repo(
    name = "syn",
    crate = "syn",
    version = "2.0.87",
    features = ["clone-impls", "default", "derive", "extra-traits", "full", "parsing", "printing", "proc-macro", "visit-mut"],
)

# proc-macro crate
rust_repo(
    name = "serde_derive",
    crate = "serde_derive",
    version = "1.0.215",
    features = ["default"],
)

# Main serde crate with derive feature
rust_repo(
    name = "serde",
    crate = "serde",
    version = "1.0.215",
    features = ["default", "derive", "serde_derive", "std"],
)

################################################################################
# Additional crates migrated to rust_repo
################################################################################

# Used by examples/rustc_guessing_game
rust_repo(
    name = "libc",
    crate = "libc",
    version = "0.2.155",
    features = ["default", "std"],
)

rust_repo(
    name = "cfg_if",
    crate = "cfg-if",
    version = "1.0.0",
)

rust_repo(
    name = "ppv_lite86",
    crate = "ppv-lite86",
    version = "0.2.17",
    features = ["simd", "std"],
)

rust_repo(
    name = "getrandom",
    crate = "getrandom",
    version = "0.2.15",
    features = ["std"],
)

rust_repo(
    name = "rand_core",
    crate = "rand_core",
    version = "0.6.4",
    features = ["alloc", "getrandom", "std"],
)

rust_repo(
    name = "rand_chacha",
    crate = "rand_chacha",
    version = "0.3.1",
    features = ["std"],
)

rust_repo(
    name = "rand",
    crate = "rand",
    version = "0.8.5",
    features = ["alloc", "default", "getrandom", "libc", "rand_chacha", "small_rng", "std", "std_rng"],
)

# General utility crates
rust_repo(
    name = "autocfg",
    crate = "autocfg",
    version = "1.3.0",
)

rust_repo(
    name = "pin_project_lite",
    crate = "pin-project-lite",
    version = "0.2.14",
)

rust_repo(
    name = "bytes",
    crate = "bytes",
    version = "1.6.0",
    features = ["default", "std"],
)

rust_repo(
    name = "futures_core",
    crate = "futures-core",
    version = "0.3.30",
    features = ["alloc", "default", "std"],
)

rust_repo(
    name = "once_cell",
    crate = "once_cell",
    version = "1.19.0",
    features = ["alloc", "default", "race", "std"],
)

rust_repo(
    name = "scopeguard",
    crate = "scopeguard",
    version = "1.2.0",
)

rust_repo(
    name = "smallvec",
    crate = "smallvec",
    version = "1.13.2",
)

rust_repo(
    name = "parking_lot_core",
    crate = "parking_lot_core",
    version = "0.9.10",
)

rust_repo(
    name = "hashbrown",
    crate = "hashbrown",
    version = "0.14.5",
    features = ["raw"],
)

rust_repo(
    name = "hashbrown_0_12_3",
    crate = "hashbrown",
    version = "0.12.3",
    features = ["raw"],
)

rust_repo(
    name = "equivalent",
    crate = "equivalent",
    version = "1.0.1",
)

rust_repo(
    name = "indexmap",
    crate = "indexmap",
    version = "2.2.6",
    features = ["default", "std"],
)

rust_repo(
    name = "indexmap_1_9_3",
    crate = "indexmap",
    version = "1.9.3",
)

rust_repo(
    name = "indexmap_1_2_0",
    crate = "indexmap",
    version = "1.2.0",
)

rust_repo(
    name = "anyhow",
    crate = "anyhow",
    version = "1.0.86",
    features = ["default", "std"],
)

rust_repo(
    name = "either",
    crate = "either",
    version = "1.12.0",
)

rust_repo(
    name = "lock_api",
    crate = "lock_api",
    version = "0.4.12",
    features = ["atomic_usize", "default"],
)

rust_repo(
    name = "parking_lot",
    crate = "parking_lot",
    version = "0.12.3",
    features = ["default"],
)

rust_repo(
    name = "itertools",
    crate = "itertools",
    version = "0.12.1",
    features = ["use_alloc"],
)

rust_repo(
    name = "slab",
    crate = "slab",
    version = "0.4.9",
    features = ["default", "std"],
)

rust_repo(
    name = "pin_utils",
    crate = "pin-utils",
    version = "0.1.0",
)

rust_repo(
    name = "fnv",
    crate = "fnv",
    version = "1.0.7",
    features = ["default", "std"],
)

rust_repo(
    name = "futures_task",
    crate = "futures-task",
    version = "0.3.30",
    features = ["alloc"],
)

rust_repo(
    name = "itoa",
    crate = "itoa",
    version = "1.0.11",
)

rust_repo(
    name = "http",
    crate = "http",
    version = "0.2.12",
)

rust_repo(
    name = "futures_util",
    crate = "futures-util",
    version = "0.3.30",
    features = ["alloc"],
)

rust_repo(
    name = "num_cpus",
    crate = "num_cpus",
    version = "1.16.0",
)

rust_repo(
    name = "signal_hook_registry",
    crate = "signal-hook-registry",
    version = "1.4.2",
)

rust_repo(
    name = "mio",
    crate = "mio",
    version = "0.8.11",
    features = ["net", "os-ext", "os-poll"],
)

rust_repo(
    name = "socket2",
    crate = "socket2",
    version = "0.5.7",
    features = ["all"],
)

rust_repo(
    name = "rustversion",
    crate = "rustversion",
    version = "1.0.17",
)

rust_repo(
    name = "tracing_core",
    crate = "tracing-core",
    version = "0.1.32",
    features = ["once_cell", "std"],
)

rust_repo(
    name = "futures_sink",
    crate = "futures-sink",
    version = "0.3.30",
    features = ["alloc", "default", "std"],
)

rust_repo(
    name = "bitflags",
    crate = "bitflags",
    version = "2.5.0",
    features = ["std"],
)

rust_repo(
    name = "bitflags_1_3_2",
    crate = "bitflags",
    version = "1.3.2",
    features = ["default"],
)

rust_repo(
    name = "httparse",
    crate = "httparse",
    version = "1.9.3",
    features = ["default", "std"],
)

rust_repo(
    name = "linux_raw_sys",
    crate = "linux-raw-sys",
    version = "0.4.14",
    features = ["elf", "errno", "general", "ioctl", "no_std"],
)

rust_repo(
    name = "errno",
    crate = "errno",
    version = "0.3.9",
)

rust_repo(
    name = "rustix",
    crate = "rustix",
    version = "0.38.34",
    features = ["alloc", "default", "fs", "libc-extra-traits", "std", "use-libc-auxv"],
)

rust_repo(
    name = "tower_service",
    crate = "tower-service",
    version = "0.3.2",
)

rust_repo(
    name = "regex_syntax",
    crate = "regex-syntax",
    version = "0.8.4",
    features = ["std", "unicode-bool"],
)

rust_repo(
    name = "prettyplease",
    crate = "prettyplease",
    version = "0.2.20",
)

rust_repo(
    name = "http_body",
    crate = "http-body",
    version = "0.4.6",
)

rust_repo(
    name = "try_lock",
    crate = "try-lock",
    version = "0.2.5",
)

rust_repo(
    name = "fastrand",
    crate = "fastrand",
    version = "2.1.0",
    features = ["alloc", "default", "std"],
)

rust_repo(
    name = "fixedbitset",
    crate = "fixedbitset",
    version = "0.4.2",
)

rust_repo(
    name = "want",
    crate = "want",
    version = "0.3.1",
)

rust_repo(
    name = "petgraph",
    crate = "petgraph",
    version = "0.6.5",
)

rust_repo(
    name = "futures_channel",
    crate = "futures-channel",
    version = "0.3.30",
    features = ["alloc", "default", "std"],
)

rust_repo(
    name = "heck",
    crate = "heck",
    version = "0.5.0",
)

rust_repo(
    name = "httpdate",
    crate = "httpdate",
    version = "1.0.3",
)

rust_repo(
    name = "log",
    crate = "log",
    version = "0.4.21",
    features = ["default", "std"],
)

rust_repo(
    name = "multimap",
    crate = "multimap",
    version = "0.10.0",
)

rust_repo(
    name = "tower_layer",
    crate = "tower-layer",
    version = "0.3.2",
)

rust_repo(
    name = "regex_automata",
    crate = "regex-automata",
    version = "0.4.7",
    features = ["alloc", "meta", "nfa-pikevm", "nfa-thompson", "std", "syntax", "unicode-bool"],
)

rust_repo(
    name = "mime",
    crate = "mime",
    version = "0.3.17",
)

rust_repo(
    name = "tempfile",
    crate = "tempfile",
    version = "3.10.1",
)

rust_repo(
    name = "matchit",
    crate = "matchit",
    version = "0.7.3",
    features = ["default"],
)

rust_repo(
    name = "percent_encoding",
    crate = "percent-encoding",
    version = "2.3.1",
    features = ["alloc", "default", "std"],
)

rust_repo(
    name = "sync_wrapper",
    crate = "sync_wrapper",
    version = "0.1.2",
)

rust_repo(
    name = "memchr",
    crate = "memchr",
    version = "2.7.4",
    features = ["alloc", "default", "std"],
)

rust_repo(
    name = "base64",
    crate = "base64",
    version = "0.21.7",
    features = ["alloc", "default", "std"],
)

rust_repo(
    name = "regex",
    crate = "regex",
    version = "1.10.5",
    features = ["std", "unicode-bool"],
)

rust_repo(
    name = "tokio_macros",
    crate = "tokio-macros",
    version = "2.3.0",
)

rust_repo(
    name = "prost_derive",
    crate = "prost-derive",
    version = "0.12.6",
)

rust_repo(
    name = "tracing_attributes",
    crate = "tracing-attributes",
    version = "0.1.27",
)

rust_repo(
    name = "pin_project_internal",
    crate = "pin-project-internal",
    version = "1.1.5",
)

rust_repo(
    name = "async_trait",
    crate = "async-trait",
    version = "0.1.80",
)

rust_repo(
    name = "axum_core",
    crate = "axum-core",
    version = "0.3.4",
)

rust_repo(
    name = "async_stream",
    crate = "async-stream",
    version = "0.3.6",
)

rust_repo(
    name = "async_stream_impl",
    crate = "async-stream-impl",
    version = "0.3.6",
)

rust_repo(
    name = "tokio",
    crate = "tokio",
    version = "1.38.0",
    features = ["bytes", "default", "fs", "full", "io-std", "io-util", "libc", "macros", "mio", "net", "num_cpus", "parking_lot", "process", "rt", "rt-multi-thread", "signal", "signal-hook-registry", "socket2", "sync", "time", "tokio-macros"],
)

rust_repo(
    name = "pin_project",
    crate = "pin-project",
    version = "1.1.5",
)

rust_repo(
    name = "tracing",
    crate = "tracing",
    version = "0.1.40",
    features = ["attributes", "default", "std", "tracing-attributes"],
)

rust_repo(
    name = "prost",
    crate = "prost",
    version = "0.12.6",
    features = ["derive", "prost-derive", "std"],
)

rust_repo(
    name = "prost_types",
    crate = "prost-types",
    version = "0.12.6",
)

rust_repo(
    name = "prost_build",
    crate = "prost-build",
    version = "0.12.6",
    features = ["default", "format"],
)

rust_repo(
    name = "tonic_build",
    crate = "tonic-build",
    version = "0.11.0",
    features = ["default", "prost", "prost-build", "transport"],
)

rust_repo(
    name = "tokio_util",
    crate = "tokio-util",
    version = "0.7.11",
    features = ["codec", "default", "io"],
)

rust_repo(
    name = "tokio_io_timeout",
    crate = "tokio-io-timeout",
    version = "1.2.0",
)

rust_repo(
    name = "tokio_stream",
    crate = "tokio-stream",
    version = "0.1.15",
    features = ["default", "time"],
)

rust_repo(
    name = "h2",
    crate = "h2",
    version = "0.3.26",
)

rust_repo(
    name = "tower",
    crate = "tower",
    version = "0.4.13",
    features = ["__common", "balance", "buffer", "discover", "futures-core", "futures-util", "indexmap", "limit", "load", "make", "pin-project", "pin-project-lite", "rand", "ready-cache", "slab", "timeout", "tokio", "tokio-util", "tracing", "util"],
)

rust_repo(
    name = "hyper",
    crate = "hyper",
    version = "0.14.29",
    features = ["client", "default", "full", "h2", "http1", "http2", "runtime", "server", "socket2", "stream", "tcp"],
)

rust_repo(
    name = "axum",
    crate = "axum",
    version = "0.6.20",
)

rust_repo(
    name = "hyper_timeout",
    crate = "hyper-timeout",
    version = "0.4.1",
)

rust_repo(
    name = "tonic",
    crate = "tonic",
    version = "0.11.0",
    features = ["channel", "codegen", "default", "prost", "transport"],
)

# rust-proto examples
rust_repo(
    name = "once_cell_1_10_0",
    crate = "once_cell",
    version = "1.10.0",
)

rust_repo(
    name = "protoc_gen_prost",
    crate = "protoc-gen-prost",
    version = "0.3.1",
)

rust_repo(
    name = "protoc_gen_tonic",
    crate = "protoc-gen-tonic",
    version = "0.4.0",
)

# Test build scripts with external deps
rust_repo(
    name = "libsecp256k1",
    crate = "libsecp256k1",
    version = "0.6.0",
    features = ["default"],
)

rust_repo(
    name = "arrayref",
    crate = "arrayref",
    version = "0.3.9",
)

rust_repo(
    name = "libsecp256k1_core",
    crate = "libsecp256k1-core",
    version = "0.2.2",
)

rust_repo(
    name = "crunchy",
    crate = "crunchy",
    version = "0.2.2",
)

rust_repo(
    name = "subtle",
    crate = "subtle",
    version = "2.6.1",
)

rust_repo(
    name = "digest",
    crate = "digest",
    version = "0.10.7",
    features = ["default", "core-api", "block-buffer"],
)

rust_repo(
    name = "digest_0_9_0",
    crate = "digest",
    version = "0.9.0",
    features = ["std"],
)

rust_repo(
    name = "block_buffer",
    crate = "block-buffer",
    version = "0.10.4",
)

rust_repo(
    name = "crypto_common",
    crate = "crypto-common",
    version = "0.1.6",
)

rust_repo(
    name = "generic_array",
    crate = "generic-array",
    version = "0.14.7",
)

rust_repo(
    name = "typenum",
    crate = "typenum",
    version = "1.17.0",
)

rust_repo(
    name = "libsecp256k1_gen_ecmult",
    crate = "libsecp256k1-gen-ecmult",
    version = "0.3.0",
)

rust_repo(
    name = "libsecp256k1_gen_genmult",
    crate = "libsecp256k1-gen-genmult",
    version = "0.3.0",
)

rust_repo(
    name = "blake3",
    crate = "blake3",
    version = "1.5.4",
    features = ["default", "std"],
)

rust_repo(
    name = "arrayvec",
    crate = "arrayvec",
    version = "0.7.6",
)

rust_repo(
    name = "constant_time_eq",
    crate = "constant_time_eq",
    version = "0.3.1",
)

rust_repo(
    name = "cc",
    crate = "cc",
    version = "1.2.0",
)

rust_repo(
    name = "shlex",
    crate = "shlex",
    version = "1.3.0",
    features = ["std"],
)

# Benchmarking
rust_repo(
    name = "criterion",
    crate = "criterion",
    version = "0.5.1",
    features = ["html_reports", "csv_reports"],
)

rust_repo(
    name = "anes",
    crate = "anes",
    version = "0.1.6",
)

rust_repo(
    name = "cast",
    crate = "cast",
    version = "0.3.0",
)

rust_repo(
    name = "ciborium",
    crate = "ciborium",
    version = "0.2.2",
    features = ["std"],
)

rust_repo(
    name = "ciborium_io",
    crate = "ciborium-io",
    version = "0.2.2",
    features = ["std", "default", "alloc"],
)

rust_repo(
    name = "ciborium_ll",
    crate = "ciborium-ll",
    version = "0.2.2",
)

rust_repo(
    name = "half",
    crate = "half",
    version = "2.4.1",
)

rust_repo(
    name = "clap",
    crate = "clap",
    version = "4.4.9",
    features = ["std", "help"],
)

rust_repo(
    name = "clap_builder",
    crate = "clap_builder",
    version = "4.5.21",
    features = ["std"],
)

rust_repo(
    name = "anstyle",
    crate = "anstyle",
    version = "1.0.10",
)

rust_repo(
    name = "clap_lex",
    crate = "clap_lex",
    version = "0.7.3",
)

rust_repo(
    name = "criterion_plot",
    crate = "criterion-plot",
    version = "0.5.0",
)

rust_repo(
    name = "is_terminal",
    crate = "is-terminal",
    version = "0.4.13",
)

rust_repo(
    name = "hermit_abi",
    crate = "hermit-abi",
    version = "0.4.0",
)

rust_repo(
    name = "num_traits",
    crate = "num-traits",
    version = "0.2.19",
    features = ["std", "default", "i128"],
)

rust_repo(
    name = "oorandom",
    crate = "oorandom",
    version = "11.1.4",
)

rust_repo(
    name = "serde_json",
    crate = "serde_json",
    version = "1.0.133",
    features = ["std"],
)

rust_repo(
    name = "ryu",
    crate = "ryu",
    version = "1.0.18",
)

rust_repo(
    name = "tinytemplate",
    crate = "tinytemplate",
    version = "1.2.1",
)

rust_repo(
    name = "walkdir",
    crate = "walkdir",
    version = "2.5.0",
)

rust_repo(
    name = "same_file",
    crate = "same-file",
    version = "1.0.6",
)
