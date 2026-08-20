//! `rust_repo(patch = ...)` was declared and documented for the life of the
//! plugin and consumed by nothing, so a consumer setting it got no patch and
//! no error. This fails if the patch is not applied.

#[test]
fn the_patch_is_applied() {
    assert_eq!(
        patched::answer(),
        42,
        "rust_repo(patch = ...) did not apply the patch"
    );
}
