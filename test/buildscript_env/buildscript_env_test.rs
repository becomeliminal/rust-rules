extern crate genpath;

#[test]
fn a_generated_path_passed_through_an_env_var_resolves() {
    // Reaching this at all means the include! found the generated file.
    assert_eq!(genpath::ANSWER, 42);
}

#[test]
fn an_env_var_that_is_not_a_path_is_untouched() {
    assert_eq!(genpath::plain(), "not-a-path");
}
