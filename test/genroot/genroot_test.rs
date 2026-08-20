// The crate this depends on has a generated root module, which is the shape
// the IDE fragment used to record as a label rather than a path.
extern crate genroot;

#[test]
fn a_crate_rooted_at_a_generated_file_builds() {
    assert_eq!(genroot::answer(), 42);
}
