//! A first-party crate with a third-party dependency, so the generated
//! rust-project.json has both halves and a real edge between them.

use anyhow::{Context, Result};

/// Greets someone, or explains why it could not.
pub fn greet(name: &str) -> Result<String> {
    if name.is_empty() {
        anyhow::bail!("nobody to greet");
    }
    Ok(format!("Hello, {}!", name))
}

/// Parses a name out of a line, so there is something for inference to chew
/// on when rust-analyzer analyses this crate.
pub fn greet_line(line: &str) -> Result<String> {
    let name = line.split(':').nth(1).context("no name in line")?;
    greet(name.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greets() {
        assert_eq!(greet("world").unwrap(), "Hello, world!");
        assert!(greet("").is_err());
        assert_eq!(greet_line("name: ada").unwrap(), "Hello, ada!");
    }
}
