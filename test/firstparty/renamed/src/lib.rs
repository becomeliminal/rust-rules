//! A package whose crate is not called what the package is: the label a
//! dependent writes says renamed-pkg, and its source says `renamed`.

pub fn twin_version() -> u32 {
    twin::VERSION
}
