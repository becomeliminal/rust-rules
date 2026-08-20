//! The unpatched crate is deliberately wrong, so a patch that does not apply
//! shows up as a failing test rather than as a silently unchanged build.
pub fn answer() -> u32 {
    41
}
