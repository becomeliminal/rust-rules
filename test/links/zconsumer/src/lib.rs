#[cfg(not(dep_metadata_received))]
compile_error!("build script did not receive DEP_ZSTUB_* metadata");

pub fn consumes() -> u32 {
    zstub::stub()
}
