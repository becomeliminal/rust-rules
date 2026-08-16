fn main() {
    // The whole point: zstub's metadata must arrive as DEP_ZSTUB_* env vars
    let include = std::env::var("DEP_ZSTUB_INCLUDE").expect("DEP_ZSTUB_INCLUDE not set");
    assert_eq!(include, "/magic/include");
    let libdir = std::env::var("DEP_ZSTUB_LIBDIR").expect("DEP_ZSTUB_LIBDIR not set");
    assert_eq!(libdir, "/magic/lib");
    println!("cargo:rustc-cfg=dep_metadata_received");
}
