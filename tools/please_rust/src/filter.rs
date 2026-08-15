//! Filter source files by cfg/features
//!
//! This module filters Rust source files based on #[cfg(...)] attributes,
//! similar to how please_go filter works for build tags.

use anyhow::Result;
use clap::Args;
use std::fs;
use std::path::PathBuf;

#[derive(Args)]
pub struct FilterArgs {
    /// Target triple (e.g., x86_64-unknown-linux-gnu)
    #[arg(long, default_value = "x86_64-unknown-linux-gnu")]
    pub target: String,

    /// Features to enable (comma-separated)
    #[arg(long, default_value = "")]
    pub features: String,

    /// Source files to filter
    #[arg(required = true)]
    pub sources: Vec<PathBuf>,
}

pub fn run(args: FilterArgs) -> Result<()> {
    let features: Vec<String> = if args.features.is_empty() {
        Vec::new()
    } else {
        args.features.split(',').map(|s| s.trim().to_string()).collect()
    };

    // Parse target triple
    let target_parts: Vec<&str> = args.target.split('-').collect();
    let target_arch = target_parts.get(0).unwrap_or(&"x86_64");
    let target_os = target_parts.get(2).unwrap_or(&"linux");

    for source in &args.sources {
        if should_include_source(source, target_arch, target_os, &features)? {
            // Print included files to stdout (like please_go filter)
            println!("{}", source.display());
        }
    }

    Ok(())
}

fn should_include_source(
    source: &PathBuf,
    target_arch: &str,
    target_os: &str,
    features: &[String],
) -> Result<bool> {
    // Read the file content
    let content = fs::read_to_string(source)?;

    // Check for #![cfg(...)] or #[cfg(...)] at the module level
    // This is a simplified check - a full implementation would parse the AST

    // Check for common cfg patterns that would exclude this file
    for line in content.lines() {
        let line = line.trim();

        // Check for #![cfg(not(...))] or #[cfg(not(...))] patterns
        if line.starts_with("#![cfg(") || line.starts_with("#[cfg(") {
            if let Some(cfg_content) = extract_cfg_content(line) {
                if !evaluate_cfg(&cfg_content, target_arch, target_os, features) {
                    return Ok(false);
                }
            }
        }

        // Also check for cfg_attr
        if line.starts_with("#![cfg_attr(") || line.starts_with("#[cfg_attr(") {
            // cfg_attr doesn't exclude, it conditionally applies attributes
            continue;
        }

        // Stop checking after we've passed the header attributes
        if !line.starts_with("#") && !line.starts_with("//") && !line.is_empty() {
            break;
        }
    }

    Ok(true)
}

fn extract_cfg_content(line: &str) -> Option<String> {
    // Extract content between cfg( and )
    let start = line.find("cfg(")?;
    let content = &line[start + 4..];

    // Find matching closing paren
    let mut depth = 1;
    let mut end = 0;
    for (i, c) in content.chars().enumerate() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    end = i;
                    break;
                }
            }
            _ => {}
        }
    }

    if end > 0 {
        Some(content[..end].to_string())
    } else {
        None
    }
}

fn evaluate_cfg(cfg: &str, target_arch: &str, target_os: &str, features: &[String]) -> bool {
    let cfg = cfg.trim();

    // Handle not(...)
    if cfg.starts_with("not(") && cfg.ends_with(")") {
        let inner = &cfg[4..cfg.len() - 1];
        return !evaluate_cfg(inner, target_arch, target_os, features);
    }

    // Handle all(...)
    if cfg.starts_with("all(") && cfg.ends_with(")") {
        let inner = &cfg[4..cfg.len() - 1];
        return split_cfg_args(inner)
            .iter()
            .all(|arg| evaluate_cfg(arg, target_arch, target_os, features));
    }

    // Handle any(...)
    if cfg.starts_with("any(") && cfg.ends_with(")") {
        let inner = &cfg[4..cfg.len() - 1];
        return split_cfg_args(inner)
            .iter()
            .any(|arg| evaluate_cfg(arg, target_arch, target_os, features));
    }

    // Handle feature = "..."
    if cfg.starts_with("feature") {
        if let Some(feature_name) = extract_string_value(cfg) {
            return features.contains(&feature_name);
        }
        return false;
    }

    // Handle target_os = "..."
    if cfg.starts_with("target_os") {
        if let Some(os) = extract_string_value(cfg) {
            return os == target_os;
        }
        return false;
    }

    // Handle target_arch = "..."
    if cfg.starts_with("target_arch") {
        if let Some(arch) = extract_string_value(cfg) {
            return arch == target_arch;
        }
        return false;
    }

    // Handle unix, windows, etc.
    match cfg {
        "unix" => target_os == "linux" || target_os == "macos" || target_os == "darwin",
        "windows" => target_os == "windows",
        "linux" => target_os == "linux",
        "macos" | "darwin" => target_os == "macos" || target_os == "darwin",
        _ => {
            // Unknown cfg, default to true (include the file)
            true
        }
    }
}

fn split_cfg_args(s: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut depth = 0;

    for c in s.chars() {
        match c {
            '(' => {
                depth += 1;
                current.push(c);
            }
            ')' => {
                depth -= 1;
                current.push(c);
            }
            ',' if depth == 0 => {
                if !current.trim().is_empty() {
                    result.push(current.trim().to_string());
                }
                current = String::new();
            }
            _ => current.push(c),
        }
    }

    if !current.trim().is_empty() {
        result.push(current.trim().to_string());
    }

    result
}

fn extract_string_value(cfg: &str) -> Option<String> {
    // Extract value from patterns like `feature = "derive"` or `target_os = "linux"`
    let parts: Vec<&str> = cfg.split('=').collect();
    if parts.len() == 2 {
        let value = parts[1].trim().trim_matches('"');
        return Some(value.to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evaluate_cfg_unix() {
        assert!(evaluate_cfg("unix", "x86_64", "linux", &[]));
        assert!(!evaluate_cfg("unix", "x86_64", "windows", &[]));
    }

    #[test]
    fn test_evaluate_cfg_feature() {
        let features = vec!["derive".to_string()];
        assert!(evaluate_cfg("feature = \"derive\"", "x86_64", "linux", &features));
        assert!(!evaluate_cfg("feature = \"std\"", "x86_64", "linux", &features));
    }

    #[test]
    fn test_evaluate_cfg_not() {
        assert!(!evaluate_cfg("not(unix)", "x86_64", "linux", &[]));
        assert!(evaluate_cfg("not(windows)", "x86_64", "linux", &[]));
    }

    #[test]
    fn test_split_cfg_args() {
        let args = split_cfg_args("unix, target_arch = \"x86_64\"");
        assert_eq!(args.len(), 2);
        assert_eq!(args[0], "unix");
        assert_eq!(args[1], "target_arch = \"x86_64\"");
    }
}
