//! Build script execution for Rust crates
//!
//! This module handles compiling and running Cargo build scripts (build.rs),
//! parsing their output directives, and producing a .buildscript file that
//! can be consumed by the compile command.

use anyhow::{Context, Result};
use cargo_toml::Manifest;
use clap::Args;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Args)]
pub struct BuildScriptArgs {
    /// Path to Cargo.toml
    #[arg(long)]
    pub manifest_path: PathBuf,

    /// Path to build.rs
    #[arg(long)]
    pub build_script: PathBuf,

    /// Output directory for build script (OUT_DIR)
    #[arg(long)]
    pub out_dir: PathBuf,

    /// Path to rustc binary
    #[arg(long, default_value = "rustc")]
    pub rustc: PathBuf,

    /// Target triple
    #[arg(long, default_value = "x86_64-unknown-linux-gnu")]
    pub target: String,

    /// Host triple
    #[arg(long, default_value = "x86_64-unknown-linux-gnu")]
    pub host: String,

    /// Features to enable (can be specified multiple times)
    #[arg(long = "feature")]
    pub features: Vec<String>,

    /// Debug mode (-g)
    #[arg(short = 'g', long)]
    pub debug: bool,

    /// Optimization mode (-O)
    #[arg(short = 'O', long = "optimize")]
    pub optimize: bool,

    /// Output file for parsed directives
    #[arg(long)]
    pub output: PathBuf,

    /// Path to sysroot (contains lib/rustlib/...)
    #[arg(long)]
    pub sysroot: Option<PathBuf>,

    /// Additional -L search paths for build script compilation
    #[arg(short = 'L', long = "search-path")]
    pub search_paths: Vec<PathBuf>,

    /// Externconfig file for build script dependencies
    #[arg(long)]
    pub externconfig: Option<PathBuf>,
}

/// Parsed build script directives
#[derive(Debug, Default)]
struct Directives {
    rustc_cfgs: Vec<String>,
    rustc_envs: Vec<(String, String)>,
    rustc_link_libs: Vec<String>,
    rustc_link_searches: Vec<String>,
    rustc_link_args: Vec<String>,
    metadata: Vec<(String, String)>,
    warnings: Vec<String>,
}

pub fn run(args: BuildScriptArgs) -> Result<()> {
    // 1. Parse Cargo.toml for package metadata
    // Use from_slice() instead of from_path() to avoid filesystem traversal
    // that fails in Please sandbox (from_path calls complete_from_path which
    // traverses parent directories looking for workspace Cargo.toml)
    let manifest_content = fs::read(&args.manifest_path)
        .with_context(|| format!("Failed to read {}", args.manifest_path.display()))?;
    let manifest = Manifest::from_slice(&manifest_content)
        .with_context(|| format!("Failed to parse {}", args.manifest_path.display()))?;

    let pkg = manifest
        .package
        .as_ref()
        .context("Cargo.toml missing [package] section")?;

    // 2. Create OUT_DIR
    fs::create_dir_all(&args.out_dir)
        .with_context(|| format!("Failed to create OUT_DIR: {}", args.out_dir.display()))?;

    let out_dir = args.out_dir.canonicalize()
        .with_context(|| format!("Failed to canonicalize OUT_DIR: {}", args.out_dir.display()))?;

    // 3. Compile build.rs as a binary
    let build_script_binary = compile_build_script(&args, &out_dir)?;

    // 4. Build environment variables
    let env = build_environment(&args, pkg, &out_dir)?;

    // 5. Execute build script
    let directives = execute_build_script(&build_script_binary, &env)?;

    // 6. Print warnings
    for warning in &directives.warnings {
        eprintln!("warning: {}", warning);
    }

    // 7. Write directives to output file
    write_directives(&args.output, &directives, &out_dir)?;

    eprintln!(
        "please_rust build-script: Generated {} for {}",
        args.output.display(),
        pkg.name
    );

    Ok(())
}

fn compile_build_script(args: &BuildScriptArgs, out_dir: &Path) -> Result<PathBuf> {
    let binary_path = out_dir.join("build_script");

    let mut cmd = Command::new(&args.rustc);

    cmd.arg(&args.build_script)
        .arg("--crate-name=build_script")
        .arg("--crate-type=bin")
        .arg("--edition=2021")
        .arg("-o")
        .arg(&binary_path);

    // Set sysroot if provided (tells rustc where to find std/core)
    if let Some(sysroot) = &args.sysroot {
        cmd.arg("--sysroot").arg(sysroot);
    }

    // Add search paths
    for path in &args.search_paths {
        cmd.arg("-L").arg(path);
    }

    // Add extern crates from externconfig (for build-dependencies)
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
                    // Search for the file
                    if let Some(path) = find_file_recursive(".", filename.trim()) {
                        cmd.arg("--extern").arg(format!("{}={}", name.trim(), path.display()));
                        if let Some(dir) = path.parent() {
                            if !dir.as_os_str().is_empty() {
                                cmd.arg("-L").arg(dir);
                            }
                        }
                    }
                }
            }
        }
    }

    // Optimization/debug flags
    if args.optimize {
        cmd.arg("-O");
    }
    if args.debug {
        cmd.arg("-g");
    }

    eprintln!("please_rust build-script compile: {:?}", cmd);

    let status = cmd
        .status()
        .with_context(|| format!("Failed to execute rustc: {}", args.rustc.display()))?;

    if !status.success() {
        anyhow::bail!(
            "Failed to compile build script: {}",
            args.build_script.display()
        );
    }

    Ok(binary_path)
}

fn build_environment(
    args: &BuildScriptArgs,
    pkg: &cargo_toml::Package,
    out_dir: &Path,
) -> Result<HashMap<String, String>> {
    let mut env = HashMap::new();

    // Get the manifest directory (parent of Cargo.toml)
    // If manifest_path is just "Cargo.toml" (no parent), use current directory
    let manifest_dir = match args.manifest_path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.canonicalize()
            .with_context(|| format!("Failed to canonicalize manifest directory: {}", p.display()))?,
        _ => std::env::current_dir()
            .context("Failed to get current directory")?,
    };

    // Core variables
    env.insert("CARGO".to_string(), "/bin/false".to_string()); // Fake cargo
    env.insert("CARGO_MANIFEST_DIR".to_string(), manifest_dir.display().to_string());
    env.insert("OUT_DIR".to_string(), out_dir.display().to_string());
    env.insert("TARGET".to_string(), args.target.clone());
    env.insert("HOST".to_string(), args.host.clone());
    env.insert("NUM_JOBS".to_string(), "1".to_string());
    env.insert("RUSTC".to_string(), args.rustc.display().to_string());
    env.insert("RUSTDOC".to_string(), "rustdoc".to_string());

    // Optimization level
    if args.optimize {
        env.insert("OPT_LEVEL".to_string(), "3".to_string());
        env.insert("DEBUG".to_string(), "false".to_string());
        env.insert("PROFILE".to_string(), "release".to_string());
    } else {
        env.insert("OPT_LEVEL".to_string(), "0".to_string());
        env.insert("DEBUG".to_string(), "true".to_string());
        env.insert("PROFILE".to_string(), "debug".to_string());
    }

    // Package metadata - CARGO_PKG_NAME
    env.insert("CARGO_PKG_NAME".to_string(), pkg.name.clone());

    // CARGO_PKG_VERSION and components
    // pkg.version is Inheritable<String>, .get() returns Result<&String, Error>
    let version_str = pkg.version.get()
        .map(|v| v.clone())
        .unwrap_or_else(|_| "0.0.0".to_string());
    env.insert("CARGO_PKG_VERSION".to_string(), version_str.clone());

    // Parse version components (e.g., "1.2.3-beta.1" -> major=1, minor=2, patch=3, pre=beta.1)
    let parts: Vec<&str> = version_str.split('.').collect();
    env.insert("CARGO_PKG_VERSION_MAJOR".to_string(), parts.first().unwrap_or(&"0").to_string());
    env.insert("CARGO_PKG_VERSION_MINOR".to_string(), parts.get(1).unwrap_or(&"0").to_string());

    // Handle patch version which might have pre-release suffix
    let patch_part = parts.get(2).unwrap_or(&"0");
    let (patch, pre) = if let Some(idx) = patch_part.find('-') {
        (&patch_part[..idx], &patch_part[idx + 1..])
    } else {
        (*patch_part, "")
    };
    env.insert("CARGO_PKG_VERSION_PATCH".to_string(), patch.to_string());
    env.insert("CARGO_PKG_VERSION_PRE".to_string(), pre.to_string());

    // CARGO_PKG_AUTHORS - deprecated but still used by some build scripts
    // pkg.authors is Inheritable<Vec<String>>
    if let Ok(authors) = pkg.authors.get() {
        env.insert("CARGO_PKG_AUTHORS".to_string(), authors.join(":"));
    } else {
        env.insert("CARGO_PKG_AUTHORS".to_string(), "".to_string());
    }

    // CARGO_PKG_DESCRIPTION - pkg.description is Option<Inheritable<String>>
    if let Some(ref desc_inheritable) = pkg.description {
        if let Ok(desc) = desc_inheritable.get() {
            env.insert("CARGO_PKG_DESCRIPTION".to_string(), desc.clone());
        }
    }
    // Set empty string if not present (Cargo does this)
    env.entry("CARGO_PKG_DESCRIPTION".to_string()).or_insert_with(|| "".to_string());

    // CARGO_PKG_HOMEPAGE - pkg.homepage is Option<Inheritable<String>>
    if let Some(ref homepage_inheritable) = pkg.homepage {
        if let Ok(homepage) = homepage_inheritable.get() {
            env.insert("CARGO_PKG_HOMEPAGE".to_string(), homepage.clone());
        }
    }
    env.entry("CARGO_PKG_HOMEPAGE".to_string()).or_insert_with(|| "".to_string());

    // CARGO_PKG_REPOSITORY - pkg.repository is Option<Inheritable<String>>
    if let Some(ref repo_inheritable) = pkg.repository {
        if let Ok(repo) = repo_inheritable.get() {
            env.insert("CARGO_PKG_REPOSITORY".to_string(), repo.clone());
        }
    }
    env.entry("CARGO_PKG_REPOSITORY".to_string()).or_insert_with(|| "".to_string());

    // CARGO_PKG_LICENSE - pkg.license is Option<Inheritable<String>>
    if let Some(ref license_inheritable) = pkg.license {
        if let Ok(license) = license_inheritable.get() {
            env.insert("CARGO_PKG_LICENSE".to_string(), license.clone());
        }
    }
    env.entry("CARGO_PKG_LICENSE".to_string()).or_insert_with(|| "".to_string());

    // CARGO_PKG_LICENSE_FILE - pkg.license_file is Option<Inheritable<PathBuf>>
    if let Some(ref license_file_inheritable) = pkg.license_file {
        if let Ok(license_file) = license_file_inheritable.get() {
            env.insert("CARGO_PKG_LICENSE_FILE".to_string(), license_file.display().to_string());
        }
    }
    env.entry("CARGO_PKG_LICENSE_FILE".to_string()).or_insert_with(|| "".to_string());

    // CARGO_PKG_RUST_VERSION - pkg.rust_version is Option<Inheritable<String>>
    if let Some(ref rust_version_inheritable) = pkg.rust_version {
        if let Ok(rust_version) = rust_version_inheritable.get() {
            env.insert("CARGO_PKG_RUST_VERSION".to_string(), rust_version.clone());
        }
    }
    env.entry("CARGO_PKG_RUST_VERSION".to_string()).or_insert_with(|| "".to_string());

    // CARGO_PKG_README - pkg.readme is Inheritable<OptionalFile>
    // OptionalFile is complex, just set empty for now if not easily extractable
    env.insert("CARGO_PKG_README".to_string(), "".to_string());

    // Feature environment variables
    for feature in &args.features {
        let feature_upper = feature.replace("-", "_").to_uppercase();
        env.insert(format!("CARGO_FEATURE_{}", feature_upper), "1".to_string());
    }

    // Target cfg variables (for x86_64-unknown-linux-gnu)
    // These are the most common ones; we can expand this based on the target triple
    if args.target.contains("linux") {
        env.insert("CARGO_CFG_TARGET_OS".to_string(), "linux".to_string());
        env.insert("CARGO_CFG_TARGET_FAMILY".to_string(), "unix".to_string());
        env.insert("CARGO_CFG_UNIX".to_string(), "".to_string());
    } else if args.target.contains("windows") {
        env.insert("CARGO_CFG_TARGET_OS".to_string(), "windows".to_string());
        env.insert("CARGO_CFG_TARGET_FAMILY".to_string(), "windows".to_string());
        env.insert("CARGO_CFG_WINDOWS".to_string(), "".to_string());
    } else if args.target.contains("darwin") || args.target.contains("macos") {
        env.insert("CARGO_CFG_TARGET_OS".to_string(), "macos".to_string());
        env.insert("CARGO_CFG_TARGET_FAMILY".to_string(), "unix".to_string());
        env.insert("CARGO_CFG_UNIX".to_string(), "".to_string());
    }

    if args.target.contains("x86_64") {
        env.insert("CARGO_CFG_TARGET_ARCH".to_string(), "x86_64".to_string());
        env.insert("CARGO_CFG_TARGET_POINTER_WIDTH".to_string(), "64".to_string());
        env.insert("CARGO_CFG_TARGET_ENDIAN".to_string(), "little".to_string());
        env.insert("CARGO_CFG_TARGET_HAS_ATOMIC".to_string(), "8,16,32,64,ptr".to_string());
        env.insert("CARGO_CFG_TARGET_FEATURE".to_string(), "fxsr,sse,sse2".to_string());
    } else if args.target.contains("aarch64") {
        env.insert("CARGO_CFG_TARGET_ARCH".to_string(), "aarch64".to_string());
        env.insert("CARGO_CFG_TARGET_POINTER_WIDTH".to_string(), "64".to_string());
        env.insert("CARGO_CFG_TARGET_ENDIAN".to_string(), "little".to_string());
        env.insert("CARGO_CFG_TARGET_HAS_ATOMIC".to_string(), "8,16,32,64,128,ptr".to_string());
    }

    // Extract vendor and env from target triple (x86_64-unknown-linux-gnu)
    let parts: Vec<&str> = args.target.split('-').collect();
    if parts.len() >= 4 {
        env.insert("CARGO_CFG_TARGET_VENDOR".to_string(), parts[1].to_string());
        env.insert("CARGO_CFG_TARGET_ENV".to_string(), parts[3].to_string());
    }

    Ok(env)
}

fn execute_build_script(binary_path: &Path, env: &HashMap<String, String>) -> Result<Directives> {
    let mut cmd = Command::new(binary_path);

    // Clear environment and set only what we want
    cmd.env_clear();

    // Set PATH so the script can find basic utilities
    if let Ok(path) = std::env::var("PATH") {
        cmd.env("PATH", path);
    }

    // Set all our environment variables
    for (key, value) in env {
        cmd.env(key, value);
    }

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::inherit());

    eprintln!("please_rust build-script execute: {:?}", binary_path);

    let mut child = cmd
        .spawn()
        .with_context(|| format!("Failed to execute build script: {}", binary_path.display()))?;

    let stdout = child.stdout.take().context("Failed to capture stdout")?;
    let reader = BufReader::new(stdout);

    let mut directives = Directives::default();

    for line in reader.lines() {
        let line = line.context("Failed to read build script output")?;
        parse_directive(&line, &mut directives);
    }

    let status = child.wait().context("Failed to wait for build script")?;

    if !status.success() {
        anyhow::bail!("Build script failed with exit code: {:?}", status.code());
    }

    Ok(directives)
}

fn parse_directive(line: &str, directives: &mut Directives) {
    // Support both cargo:: (new) and cargo: (old) prefixes
    let directive = if let Some(rest) = line.strip_prefix("cargo::") {
        rest
    } else if let Some(rest) = line.strip_prefix("cargo:") {
        rest
    } else {
        return; // Not a directive
    };

    if let Some(value) = directive.strip_prefix("rustc-cfg=") {
        directives.rustc_cfgs.push(value.to_string());
    } else if let Some(value) = directive.strip_prefix("rustc-env=") {
        if let Some((key, val)) = value.split_once('=') {
            directives.rustc_envs.push((key.to_string(), val.to_string()));
        }
    } else if let Some(value) = directive.strip_prefix("rustc-link-lib=") {
        directives.rustc_link_libs.push(value.to_string());
    } else if let Some(value) = directive.strip_prefix("rustc-link-search=") {
        directives.rustc_link_searches.push(value.to_string());
    } else if let Some(value) = directive.strip_prefix("rustc-link-arg=") {
        directives.rustc_link_args.push(value.to_string());
    } else if let Some(value) = directive.strip_prefix("metadata=") {
        if let Some((key, val)) = value.split_once('=') {
            directives.metadata.push((key.to_string(), val.to_string()));
        }
    } else if let Some(value) = directive.strip_prefix("warning=") {
        directives.warnings.push(value.to_string());
    } else if let Some(value) = directive.strip_prefix("error=") {
        // Immediately fail on error directive
        eprintln!("error: {}", value);
    }
    // Ignore rerun-if-changed and rerun-if-env-changed (not relevant for Please)
}

fn write_directives(output: &Path, directives: &Directives, out_dir: &Path) -> Result<()> {
    let mut content = String::new();

    content.push_str("# Generated by please_rust build-script\n");
    content.push_str(&format!("# OUT_DIR={}\n", out_dir.display()));

    // Write OUT_DIR so compile can access generated files
    content.push_str(&format!("out-dir={}\n", out_dir.display()));

    for cfg in &directives.rustc_cfgs {
        content.push_str(&format!("rustc-cfg={}\n", cfg));
    }

    for (key, value) in &directives.rustc_envs {
        content.push_str(&format!("rustc-env={}={}\n", key, value));
    }

    for lib in &directives.rustc_link_libs {
        content.push_str(&format!("rustc-link-lib={}\n", lib));
    }

    for path in &directives.rustc_link_searches {
        content.push_str(&format!("rustc-link-search={}\n", path));
    }

    for arg in &directives.rustc_link_args {
        content.push_str(&format!("rustc-link-arg={}\n", arg));
    }

    for (key, value) in &directives.metadata {
        content.push_str(&format!("metadata={}={}\n", key, value));
    }

    fs::write(output, &content)
        .with_context(|| format!("Failed to write directives to {}", output.display()))?;

    Ok(())
}

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
