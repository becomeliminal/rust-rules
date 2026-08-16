extern "C" {
    fn native_add(a: i32, b: i32) -> i32;
}

pub fn add_via_c(a: i32, b: i32) -> i32 {
    unsafe { native_add(a, b) }
}

#[cfg(test)]
mod tests {
    #[test]
    fn c_addition_works() {
        assert_eq!(super::add_via_c(20, 22), 42);
    }
}
