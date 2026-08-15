filegroup(
    name = "rust",
    srcs = [
        "rust_toolchain.build_defs",
        "rust.build_defs",
        "rust_crate.build_defs",
        "rust_repo.build_defs",
    ],
    visibility = ["PUBLIC"],
)

filegroup(
    name = "rust_repo",
    srcs = ["rust_repo.build_defs"],
    visibility = ["PUBLIC"],
)

subinclude("///proto//build_defs/sdk")

proto_build_defs(
    name = "rust_proto_lang",
    srcs = ["rust_proto.build_defs"],
    proto_languages = {
        "proto_language": ["rust_proto_language"],
        "grpc_language": ["rust_grpc_language"],
    },
    visibility = ["PUBLIC"],
)
