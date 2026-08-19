//! Depends on the library beside it, so the project file has a first-party
//! edge as well as a third-party one.

fn main() {
    match greeting::greet("rust-analyzer") {
        Ok(s) => println!("{}", s),
        Err(e) => eprintln!("{:#}", e),
    }
}
