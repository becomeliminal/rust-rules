// Writes a source file into OUT_DIR and hands its path to the compiler
// through a custom env var rather than through OUT_DIR itself. This is what
// mime_guess does, and the path recorded here points inside the build
// script's own sandbox, which no longer exists when the crate is compiled.
use std::io::Write;

fn main() {
    let out = std::env::var("OUT_DIR").expect("OUT_DIR is set for a build script");
    let path = std::path::Path::new(&out).join("generated_answer.rs");
    let mut f = std::fs::File::create(&path).expect("write into OUT_DIR");
    writeln!(f, "pub const ANSWER: u32 = 42;").unwrap();
    println!("cargo:rustc-env=GENERATED_ANSWER_PATH={}", path.display());
    // A value that is not a path must survive untouched.
    println!("cargo:rustc-env=PLAIN_VALUE=not-a-path");
}
