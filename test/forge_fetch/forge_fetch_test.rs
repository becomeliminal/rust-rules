// The crate under test came from codeberg rather than crates.io. If the
// archive scheme were wrong the fetch would fail; this checks the source
// that arrived is the crate we asked for and compiles.
extern crate dunce;
extern crate unit_prefix;

use unit_prefix::NumberPrefix;

#[test]
fn a_crate_fetched_from_a_non_github_forge_works() {
    match NumberPrefix::decimal(1_500_f64) {
        NumberPrefix::Prefixed(prefix, n) => {
            assert_eq!(prefix.symbol(), "k");
            assert!((n - 1.5).abs() < 1e-9);
        }
        NumberPrefix::Standalone(n) => panic!("1500 should be prefixed, got {}", n),
    }
}

#[test]
fn a_crate_from_gitlab_works_too() {
    // gitlab's archive scheme differs from github's, so this arriving at all
    // is the assertion. dunce normalises windows paths and is a no-op here.
    let p = std::path::Path::new("/tmp");
    assert_eq!(dunce::simplified(p), p);
}
