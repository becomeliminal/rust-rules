#[allow(non_upper_case_globals)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
#[allow(dead_code)]
#[path = "point_bindings.rs"]
mod bindings;

#[cfg(test)]
mod tests {
    use super::bindings::*;

    #[test]
    fn struct_layout_matches_c() {
        assert_eq!(std::mem::size_of::<Point>(), 8);
        let p = Point { x: 3, y: 4 };
        assert_eq!(p.x, 3);
        assert_eq!(p.y, 4);
    }

    #[test]
    fn constants_and_enums_bound() {
        assert_eq!(POINT_MAX_COORD, 4096);
        assert_eq!(Direction_DIR_WEST, 3);
    }
}
