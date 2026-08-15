//! Compile Rust crates by wrapping rustc
//!
//! This module provides a compile command that reads an externconfig file
//! and constructs the appropriate rustc invocation. This follows the go-rules
//! pattern of using a binary to handle config file parsing rather than bash.

use anyhow::{Context, Result};
use clap::Args;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Recursively search for a file with the given name in the directory tree
fn find_file_recursive(dir: &str, filename: &str) -> Option<PathBuf> {
    let dir_path = Path::new(dir);
    find_file_in_dir(dir_path, filename)
}

fn find_file_in_dir(dir: &Path, filename: &str) -> Option<PathBuf> {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(name) = path.file_name() {
                    if name == filename {
                        return Some(path);
                    }
                }
            } else if path.is_dir() {
                if let Some(found) = find_file_in_dir(&path, filename) {
                    return Some(found);
                }
            }
        }
    }
    None
}

#[derive(Args)]
pub struct CompileArgs {
    /// Path to externconfig file (contains crate_name=/path/to/lib.rlib lines)
    #[arg(long)]
    pub externconfig: Option<PathBuf>,

    /// Path to buildscript output file (contains rustc-cfg, rustc-link-lib, etc.)
    #[arg(long)]
    pub buildscript: Option<PathBuf>,

    /// Path to rustc binary
    #[arg(long, default_value = "rustc")]
    pub rustc: PathBuf,

    /// Path to sysroot (contains lib/rustlib/...)
    #[arg(long)]
    pub sysroot: Option<PathBuf>,

    /// Crate name
    #[arg(long)]
    pub crate_name: String,

    /// Rust edition
    #[arg(long, default_value = "2021")]
    pub edition: String,

    /// Crate type (lib, bin, proc-macro)
    #[arg(long, default_value = "lib")]
    pub crate_type: String,

    /// Emit types (comma-separated)
    #[arg(long, default_value = "dep-info,link,metadata")]
    pub emit: String,

    /// Additional -L search paths
    #[arg(short = 'L', long = "search-path")]
    pub search_paths: Vec<PathBuf>,

    /// Features to enable
    #[arg(long = "feature")]
    pub features: Vec<String>,

    /// Build a test harness (passes --test to rustc)
    #[arg(long)]
    pub test: bool,

    /// Debug mode (-g)
    #[arg(short = 'g', long)]
    pub debug: bool,

    /// Optimization mode (-O)
    #[arg(short = 'O', long = "optimize")]
    pub optimize: bool,

    /// Source file(s)
    #[arg(required = true)]
    pub sources: Vec<PathBuf>,
}

/// Parsed build script directives
#[derive(Debug, Default)]
struct BuildScriptDirectives {
    out_dir: Option<PathBuf>,
    rustc_cfgs: Vec<String>,
    rustc_envs: Vec<(String, String)>,
    rustc_link_libs: Vec<String>,
    rustc_link_searches: Vec<String>,
    rustc_link_args: Vec<String>,
}

fn parse_buildscript(path: &Path) -> Result<BuildScriptDirectives> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read buildscript: {}", path.display()))?;

    let mut directives = BuildScriptDirectives::default();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some(value) = line.strip_prefix("out-dir=") {
            directives.out_dir = Some(PathBuf::from(value));
        } else if let Some(value) = line.strip_prefix("rustc-cfg=") {
            directives.rustc_cfgs.push(value.to_string());
        } else if let Some(value) = line.strip_prefix("rustc-env=") {
            if let Some((key, val)) = value.split_once('=') {
                directives.rustc_envs.push((key.to_string(), val.to_string()));
            }
        } else if let Some(value) = line.strip_prefix("rustc-link-lib=") {
            directives.rustc_link_libs.push(value.to_string());
        } else if let Some(value) = line.strip_prefix("rustc-link-search=") {
            directives.rustc_link_searches.push(value.to_string());
        } else if let Some(value) = line.strip_prefix("rustc-link-arg=") {
            directives.rustc_link_args.push(value.to_string());
        }
        // Ignore metadata= and other directives not needed for compilation
    }

    Ok(directives)
}

pub fn run(args: CompileArgs) -> Result<()> {
    let mut cmd = Command::new(&args.rustc);

    // Set sysroot if provided (tells rustc where to find std/core)
    if let Some(sysroot) = &args.sysroot {
        cmd.arg("--sysroot").arg(sysroot);
    }

    // Parse externconfig and add --extern flags
    // The externconfig contains lines like: crate_name=libcrate.rlib
    // We search for the actual file in the current directory tree
    if let Some(config_path) = &args.externconfig {
        if config_path.exists() {
            let content = fs::read_to_string(config_path)
                .with_context(|| format!("Failed to read externconfig: {}", config_path.display()))?;

            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Some((name, filename)) = line.split_once('=') {
                    let name = name.trim();
                    let filename = filename.trim();

                    // Search for the file in current directory tree
                    let found_path = find_file_recursive(".", filename);

                    if let Some(path) = found_path {
                        // Add --extern flag with found path
                        cmd.arg("--extern");
                        cmd.arg(format!("{}={}", name, path.display()));

                        // Also add -L for the directory containing the library
                        if let Some(dir) = path.parent() {
                            if !dir.as_os_str().is_empty() {
                                cmd.arg("-L").arg(dir);
                            }
                        }
                    } else {
                        eprintln!("Warning: Could not find {} for crate {}", filename, name);
                    }
                }
            }
        }
    }

    // Parse buildscript directives (from build.rs output)
    // This adds --cfg, -l, -L, and other flags from the build script
    let buildscript_directives = if let Some(bs_path) = &args.buildscript {
        if bs_path.exists() {
            Some(parse_buildscript(bs_path)?)
        } else {
            None
        }
    } else {
        None
    };

    // Core rustc arguments
    cmd.arg(format!("--crate-name={}", args.crate_name));
    cmd.arg(format!("--edition={}", args.edition));
    if args.test {
        // --test builds a test harness binary; it supersedes --crate-type
        cmd.arg("--test");
    } else {
        cmd.arg(format!("--crate-type={}", args.crate_type));
    }
    cmd.arg(format!("--emit={}", args.emit));
    cmd.arg("--error-format=human");
    cmd.arg("-C").arg("embed-bitcode=no");

    // Search paths from command line
    for path in &args.search_paths {
        cmd.arg("-L").arg(path);
    }

    // Search paths from build script
    if let Some(ref directives) = buildscript_directives {
        for path in &directives.rustc_link_searches {
            // Handle KIND=PATH format (e.g., "native=/usr/lib")
            if let Some((_kind, actual_path)) = path.split_once('=') {
                cmd.arg("-L").arg(actual_path);
            } else {
                cmd.arg("-L").arg(path);
            }
        }

        // Add OUT_DIR as a search path (for generated code)
        if let Some(ref out_dir) = directives.out_dir {
            cmd.arg("-L").arg(out_dir);
        }
    }

    // Features from command line
    for feature in &args.features {
        cmd.arg("--cfg").arg(format!("feature=\"{}\"", feature));
    }

    // Cfg flags from build script
    if let Some(ref directives) = buildscript_directives {
        for cfg in &directives.rustc_cfgs {
            cmd.arg("--cfg").arg(cfg);
        }
    }

    // Link libraries from build script
    if let Some(ref directives) = buildscript_directives {
        for lib in &directives.rustc_link_libs {
            // Handle KIND=NAME format (e.g., "static=foo", "dylib=bar")
            if let Some((kind, name)) = lib.split_once('=') {
                cmd.arg("-l").arg(format!("{}={}", kind, name));
            } else {
                cmd.arg("-l").arg(lib);
            }
        }

        // Link arguments from build script
        for arg in &directives.rustc_link_args {
            cmd.arg("-C").arg(format!("link-arg={}", arg));
        }

        // Set environment variables from build script
        for (key, value) in &directives.rustc_envs {
            cmd.env(key, value);
        }
    }

    // Debug/optimize flags
    if args.debug {
        cmd.arg("-g");
    }
    if args.optimize {
        cmd.arg("-O");
    }

    // Source files
    for src in &args.sources {
        cmd.arg(src);
    }

    eprintln!("please_rust compile: {:?}", cmd);

    let status = cmd.status()
        .with_context(|| format!("Failed to execute rustc: {}", args.rustc.display()))?;

    if !status.success() {
        anyhow::bail!("rustc failed with exit code: {}", status.code().unwrap_or(-1));
    }

    Ok(())
}
