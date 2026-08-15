subinclude("//build_defs:rust")

# Resolved versions + unified features for the crate graph below, produced by
# `please_rust resolve` (cargo resolver v2 semantics, no cargo involved).
# Regenerate after changing rust_repo declarations.
filegroup(
    name = "rust_lock",
    srcs = ["rust.lock"],
    visibility = ["PUBLIC"],
)

rust_toolchain(
    name = "toolchain",
    hashes = ["b4cdbc7cc6b0ee0a2666b1872769fdb2ad8393b28b63952f6493b4b400e4832b"],
    version = "1.97.1",
    visibility = ["PUBLIC"],
)

subinclude("//build_defs:rust_repo")

rust_repo(
    name = "unicode_ident-1.0.12",
    crate = "unicode-ident",
    version = "1.0.12",
)

# Has build.rs (ignored for now - works without it for most use cases)
rust_repo(
    name = "proc_macro2-1.0.85",
    crate = "proc-macro2",
    version = "1.0.85",
    features = ["default", "proc-macro"],
)

rust_repo(
    name = "quote-1.0.36",
    crate = "quote",
    version = "1.0.36",
    features = ["default", "proc-macro"],
)

# Note: syn has many features enabled
rust_repo(
    name = "syn-2.0.87",
    crate = "syn",
    version = "2.0.87",
    features = ["clone-impls", "default", "derive", "extra-traits", "full", "parsing", "printing", "proc-macro", "visit-mut"],
)

# proc-macro crate
rust_repo(
    name = "serde_derive-1.0.215",
    crate = "serde_derive",
    version = "1.0.215",
    features = ["default"],
)

# Main serde crate with derive feature
rust_repo(
    name = "serde-1.0.215",
    crate = "serde",
    version = "1.0.215",
    features = ["default", "derive", "serde_derive", "std"],
)

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
    name = "smallvec-1.13.2",
    crate = "smallvec",
    version = "1.13.2",
)

rust_repo(
    name = "parking_lot_core",
    crate = "parking_lot_core",
    version = "0.9.10",
)

rust_repo(
    name = "hashbrown-0.14.5",
    crate = "hashbrown",
    version = "0.14.5",
    features = ["raw"],
)

rust_repo(
    name = "hashbrown-0.12.3",
    crate = "hashbrown",
    version = "0.12.3",
    features = ["raw"],
)

rust_repo(
    name = "equivalent-1.0.1",
    crate = "equivalent",
    version = "1.0.1",
)

rust_repo(
    name = "indexmap-2.2.6",
    crate = "indexmap",
    version = "2.2.6",
    features = ["default", "std"],
)

rust_repo(
    name = "indexmap-1.9.3",
    crate = "indexmap",
    version = "1.9.3",
)

rust_repo(
    name = "indexmap-1.2.0",
    crate = "indexmap",
    version = "1.2.0",
)

rust_repo(
    name = "anyhow-1.0.86",
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
    name = "itoa-1.0.11",
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
    name = "bitflags-1.3.2",
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
    hashes = ["2304e00983f87ffb38b55b444b5e3b60a884b5d30c0fca7d82fe33449bbe55ea"],
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
    name = "memchr-2.7.4",
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
    name = "once_cell-1.10.0",
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
    features = ["std", "static-context"],
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
    name = "digest-0.9.0",
    crate = "digest",
    version = "0.9.0",
    features = ["std"],
)

# sha2 0.9 stack for libsecp256k1 (its std feature activates sha2/std)
rust_repo(
    name = "sha2",
    crate = "sha2",
    version = "0.9.9",
)

rust_repo(
    name = "block_buffer-0.9.0",
    crate = "block-buffer",
    version = "0.9.0",
)

rust_repo(
    name = "opaque_debug",
    crate = "opaque-debug",
    version = "0.3.1",
)

rust_repo(
    name = "cpufeatures",
    crate = "cpufeatures",
    version = "0.2.14",
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
    name = "version_check",
    crate = "version_check",
    version = "0.9.5",
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
    features = ["default", "std", "pure"],
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
    name = "anstyle-1.0.10",
    crate = "anstyle",
    version = "1.0.10",
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
    name = "serde_json-1.0.133",
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
    hashes = ["29790946404f91d9c5d06f9874efddea1dc06c5efe94541a7d6863108e3a5e4b"],
)

rust_repo(
    name = "same_file",
    crate = "same-file",
    version = "1.0.6",
    hashes = ["93fc1dc3aaa9bfed95e02e6eadabb4baf7e3078b0bd1b4d7b6b0b68378900502"],
)

# Added by please_rust sync
rust_repo(
    name = "anstream",
    crate = "anstream",
    version = "0.6.21",
    hashes = ["43d5b281e737544384e969a5ccad3f1cdd24b48086a0fc1b2a5262a26b8f4f4a"],
)

rust_repo(
    name = "anstyle",
    crate = "anstyle",
    version = "1.0.13",
    hashes = ["5192cca8006f1fd4f7237516f40fa183bb07f8fbdfedaa0036de5ea9b0b45e78"],
)

rust_repo(
    name = "anstyle_parse",
    crate = "anstyle-parse",
    version = "0.2.7",
    hashes = ["4e7644824f0aa2c7b9384579234ef10eb7efb6a0deb83f9630a49594dd9c15c2"],
)

rust_repo(
    name = "anstyle_query",
    crate = "anstyle-query",
    version = "1.1.5",
    hashes = ["40c48f72fd53cd289104fc64099abca73db4166ad86ea0b4341abe65af83dadc"],
)

rust_repo(
    name = "anstyle_wincon",
    crate = "anstyle-wincon",
    version = "3.0.11",
    hashes = ["291e6a250ff86cd4a820112fb8898808a366d8f9f58ce16d1f538353ad55747d"],
)

rust_repo(
    name = "anyhow",
    crate = "anyhow",
    version = "1.0.100",
    features = ["default"],
    hashes = ["a23eb6b1614318a8071c9b2521f36b424b2c83db5eb3a0fead4a6c0809af6e61"],
)

rust_repo(
    name = "cargo_toml",
    crate = "cargo_toml",
    version = "0.20.5",
    features = ["default"],
    hashes = ["88da5a13c620b4ca0078845707ea9c3faf11edbc3ffd8497d11d686211cd1ac0"],
)

rust_repo(
    name = "cfg_expr",
    crate = "cfg-expr",
    version = "0.15.8",
    features = ["default"],
    hashes = ["d067ad48b8650848b989a59a86c6c36a995d02d2bf778d45c3c5d57bc2718f02"],
)

rust_repo(
    name = "clap",
    crate = "clap",
    version = "4.5.53",
    features = ["derive", "default"],
    hashes = ["c9e340e012a1bf4935f5282ed1436d1489548e8f72308207ea5df0e23d2d03f8"],
)

rust_repo(
    name = "clap_builder",
    crate = "clap_builder",
    version = "4.5.53",
    hashes = ["d76b5d13eaa18c901fd2f7fca939fefe3a0727a953561fefdf3b2922b8569d00"],
)

rust_repo(
    name = "clap_derive",
    crate = "clap_derive",
    version = "4.5.49",
    hashes = ["2a0b5487afeab2deb2ff4e03a807ad1a03ac532ff5a2cee5d86884440c7f7671"],
)

rust_repo(
    name = "clap_lex",
    crate = "clap_lex",
    version = "0.7.6",
    hashes = ["a1d728cc89cf3aee9ff92b05e62b19ee65a02b5702cff7d5a377e32c6ae29d8d"],
)

rust_repo(
    name = "colorchoice",
    crate = "colorchoice",
    version = "1.0.4",
    hashes = ["b05b61dc5112cbb17e4b6cd61790d9845d13888356391624cbe7e41efeac1e75"],
)

rust_repo(
    name = "equivalent",
    crate = "equivalent",
    version = "1.0.2",
    hashes = ["877a4ace8713b0bcf2a4e7eec82529c029f1d0619886d18145fea96c3ffe5c0f"],
)

rust_repo(
    name = "hashbrown",
    crate = "hashbrown",
    version = "0.16.1",
    hashes = ["841d1cc9bed7f9236f321df977030373f4a4163ae1a7dbfe1a51a2c1a51d9100"],
)

rust_repo(
    name = "indexmap",
    crate = "indexmap",
    version = "2.12.1",
    hashes = ["0ad4bb2b565bca0645f4d68c5c9af97fba094e9791da685bf83cb5f3ce74acf2"],
)

rust_repo(
    name = "is_terminal_polyfill",
    crate = "is_terminal_polyfill",
    version = "1.70.2",
    hashes = ["a6cb138bb79a146c1bd460005623e142ef0181e3d0219cb493e02f7d08a35695"],
)

rust_repo(
    name = "itoa",
    crate = "itoa",
    version = "1.0.16",
    hashes = ["7ee5b5339afb4c41626dde77b7a611bd4f2c202b897852b4bcf5d03eddc61010"],
)

rust_repo(
    name = "memchr",
    crate = "memchr",
    version = "2.7.6",
    hashes = ["f52b00d39961fc5b2736ea853c9cc86238e165017a493d1d5c8eac6bdc4cc273"],
)

rust_repo(
    name = "once_cell_polyfill",
    crate = "once_cell_polyfill",
    version = "1.70.2",
    hashes = ["384b8ab6d37215f3c5301a95a4accb5d64aa607f1fcb26a11b5303878451b4fe"],
)

rust_repo(
    name = "proc_macro2",
    crate = "proc-macro2",
    version = "1.0.103",
    features = ["default"],
    hashes = ["5ee95bc4ef87b8d5ba32e8b7714ccc834865276eab0aed5c9958d00ec45f49e8"],
)

rust_repo(
    name = "quote",
    crate = "quote",
    version = "1.0.42",
    hashes = ["a338cc41d27e6cc6dce6cefc13a0729dfbb81c262b1f519331575dd80ef3067f"],
)

rust_repo(
    name = "semver",
    crate = "semver",
    version = "1.0.23",
    features = ["default"],
    hashes = ["61697e0a1c7e512e84a621326239844a24d8207b4669b41bc18b32ea5cbf988b"],
)

rust_repo(
    name = "serde",
    crate = "serde",
    version = "1.0.228",
    features = ["derive", "default"],
    hashes = ["9a8e94ea7f378bd32cbbd37198a4a91436180c5bb472411e48b5ec2e2124ae9e"],
)

rust_repo(
    name = "serde_core",
    crate = "serde_core",
    version = "1.0.228",
    hashes = ["41d385c7d4ca58e59fc732af25c3983b67ac852c1a25000afe1175de458b67ad"],
)

rust_repo(
    name = "serde_derive",
    crate = "serde_derive",
    version = "1.0.228",
    hashes = ["d540f220d3187173da220f885ab66608367b6574e925011a9353e4badda91d79"],
)

rust_repo(
    name = "serde_json",
    crate = "serde_json",
    version = "1.0.147",
    features = ["default"],
    hashes = ["6af14725505314343e673e9ecb7cd7e8a36aa9791eb936235a3567cc31447ae4"],
)

rust_repo(
    name = "serde_spanned",
    crate = "serde_spanned",
    version = "0.6.9",
    hashes = ["bf41e0cfaf7226dca15e8197172c295a782857fcb97fad1808a166870dee75a3"],
)

rust_repo(
    name = "smallvec",
    crate = "smallvec",
    version = "1.15.2",
    hashes = ["8ed6a63f02c8539c91a8685a86f4099661ba3da017932f6ebbea6de3f0fa7c90"],
)

rust_repo(
    name = "strsim",
    crate = "strsim",
    version = "0.11.1",
    hashes = ["7da8b5736845d9f2fcb837ea5d9e2628564b3b043a70948a3f0b778838c5fb4f"],
)

rust_repo(
    name = "syn",
    crate = "syn",
    version = "2.0.111",
    features = ["full", "parsing", "default"],
    hashes = ["390cc9a294ab71bdb1aa2e99d13be9c753cd2d7bd6560c77118597410c4d2e87"],
)

rust_repo(
    name = "thiserror",
    crate = "thiserror",
    version = "1.0.69",
    features = ["default"],
    hashes = ["b6aaf5339b578ea85b50e080feb250a3e8ae8cfcdff9a461c9ec2904bc923f52"],
)

rust_repo(
    name = "thiserror_impl",
    crate = "thiserror-impl",
    version = "1.0.69",
    hashes = ["4fee6c4efc90059e10f81e6d42c60a18f76588c3d74cb83a0b242a2b6c7504c1"],
)

rust_repo(
    name = "toml",
    crate = "toml",
    version = "0.8.23",
    features = ["default"],
    hashes = ["dc1beb996b9d83529a9e75c17a1686767d148d70663143c7854d8b4a09ced362"],
)

rust_repo(
    name = "toml_datetime",
    crate = "toml_datetime",
    version = "0.6.11",
    hashes = ["22cddaf88f4fbc13c51aebbf5f8eceb5c7c5a9da2ac40a13519eb5b0a0e8f11c"],
)

rust_repo(
    name = "toml_edit",
    crate = "toml_edit",
    version = "0.22.27",
    hashes = ["41fe8c660ae4257887cf66394862d21dbca4a6ddd26f04a3560410406a2f819a"],
)

rust_repo(
    name = "toml_write",
    crate = "toml_write",
    version = "0.1.2",
    hashes = ["5d99f8c9a7727884afe522e9bd5edbfc91a3312b36a77b5fb8926e4c31a41801"],
)

rust_repo(
    name = "unicode_ident",
    crate = "unicode-ident",
    version = "1.0.22",
    hashes = ["9312f7c4f6ff9069b165498234ce8be658059c6728633667c526e27dc2cf1df5"],
)

rust_repo(
    name = "utf8parse",
    crate = "utf8parse",
    version = "0.2.2",
    hashes = ["06abde3611657adf66d383f00b093d7faecc7fa57071cce2578660c9f1010821"],
)

rust_repo(
    name = "winapi_util",
    crate = "winapi-util",
    version = "0.1.11",
    hashes = ["c2a7b1c03c876122aa43f3020e6c3c3ee5c05081c9a00739faf7503aeba10d22"],
)

rust_repo(
    name = "windows_link",
    crate = "windows-link",
    version = "0.2.1",
    hashes = ["f0805222e57f7521d6a62e36fa9163bc891acd422f971defe97d64e70d0a4fe5"],
)

rust_repo(
    name = "windows_sys",
    crate = "windows-sys",
    version = "0.61.2",
    hashes = ["ae137229bcbd6cdf0f7b80a31df61766145077ddf49416a728b02cb3921ff3fc"],
)

rust_repo(
    name = "winnow",
    crate = "winnow",
    version = "0.7.14",
    hashes = ["5a5364e9d77fcdeeaa6062ced926ee3381faa2ee02d3eb83a5c27a8825540829"],
)

rust_repo(
    name = "zmij",
    crate = "zmij",
    version = "0.1.9",
    hashes = ["d0095ecd462946aa3927d9297b63ef82fb9a5316d7a37d134eeb36e58228615a"],
)

# Added by please_rust sync
# Added by please_rust sync
