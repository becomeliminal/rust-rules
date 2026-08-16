fn main() {
    // Classic bare-key metadata form, as libz-sys and friends emit it
    println!("cargo:include=/magic/include");
    println!("cargo:libdir=/magic/lib");
}
