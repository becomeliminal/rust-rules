fn main() {
    cfg_if::cfg_if! {
        if #[cfg(unix)] {
            println!("hello from a rust-rules consumer");
        } else {
            println!("hello from somewhere exotic");
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn consumer_test_works() {
        assert_eq!(2 + 2, 4);
    }
}
