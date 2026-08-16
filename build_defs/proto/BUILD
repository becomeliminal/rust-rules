# Proto language support lives in its own package so that consumers who do
# not use the proto plugin never parse it (the sdk subinclude would deadlock
# a repo without ///proto// declared).
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
