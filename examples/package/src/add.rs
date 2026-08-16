
/// Adds two to the given number.
///
/// ```
/// assert_eq!(math::add::add_two(2), 4);
/// ```
pub fn add_two(a: i32) -> i32 {
    a + 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        assert_eq!(4, add_two(2));
    }
}
