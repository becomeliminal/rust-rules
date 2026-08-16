fn main() {
    println!("2 + 3 = {}", ffi_bridge::add_via_c(2, 3));
}
