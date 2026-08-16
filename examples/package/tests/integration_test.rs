use math;

#[test]
fn test_add_two_integration() {
    assert_eq!(math::add::add_two(3), 5);
}

#[test]
fn test_sub_two_integration() {
    assert_eq!(math::sub::sub_two(4), 2);
}
