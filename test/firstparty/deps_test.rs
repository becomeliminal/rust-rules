//! What a first-party rule's `--extern` flags have to survive.
//!
//! Both crates here are reached by a label that does not say what the crate
//! is called. `renamed-pkg` builds the crate `renamed`, the way rustls-webpki
//! builds webpki and md-5 builds md5. And `twin` is present twice, so the
//! dep on 0.2 has to name the declaration rather than the crate - which is
//! also a target name a version cannot be stripped off by guesswork.
//!
//! Getting either wrong costs the dep its `--extern`, which is quiet: rustc
//! falls back to searching `-L` and only complains when more than one
//! candidate is reachable. That is why the second version is here.

#[test]
fn a_crate_named_by_its_lib_section_resolves() {
    // Imported under the crate name, not the package name in the label.
    assert_eq!(renamed::twin_version(), 1);
}

#[test]
fn the_declared_version_is_the_one_linked() {
    // Two versions of twin are in the sandbox; this rule asked for 0.2.
    assert_eq!(twin::VERSION, 2);
}
