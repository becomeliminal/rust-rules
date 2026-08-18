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

/// The triple of the machine this is running on, as the default for flags
/// that name a platform. Derived rather than hardcoded so an unset flag
/// describes reality.
pub fn running_triple() -> String {
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        "x86" => "i686",
        other => other,
    };
    let os = match std::env::consts::OS {
        "linux" => "unknown-linux-gnu",
        "macos" => "apple-darwin",
        "windows" => "pc-windows-msvc",
        other => other,
    };
    format!("{}-{}", arch, os)
}

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

    /// Target triple. A build script branches on this to decide what it is
    /// building for, so defaulting it to a fixed platform tells every script
    /// on every other platform a lie: rustix picks its linux syscall backend
    /// and does not compile on macOS.
    #[arg(long, default_value_t = running_triple())]
    pub target: String,

    /// Host triple
    #[arg(long, default_value_t = running_triple())]
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

    /// Dependency renames as alias=crate, for build-dependencies declared
    /// with package = "...". wasm-bindgen's build script imports
    /// rustversion_compat, which is the rustversion crate.
    #[arg(long = "rename")]
    pub renames: Vec<String>,

    /// C toolchain: either a cc binary or a directory containing cc/c++/ar/ranlib
    #[arg(long)]
    pub cc: Option<PathBuf>,

    /// Buildscript outputs of direct dependencies with links keys; their
    /// metadata is exposed as DEP_<LINKS>_<KEY> env vars (cargo semantics)
    #[arg(long = "dep-metadata", num_args = 0..)]
    pub dep_metadata: Vec<PathBuf>,

    /// NUM_JOBS for the build script. Unset derives it from the machine.
    #[arg(long)]
    pub jobs: Option<usize>,
}

/// How wide a build script may compile C.
///
/// Cargo sets NUM_JOBS to its own job count, and cc-rs and cmake-rs read it
/// to decide how many compilers to run; it reaches nothing else, so this is
/// the width of a -sys crate's vendored C tree and not of anything rustc
/// does. Unset it is half the machine rather than all of it, because plz is
/// scheduling other actions at the same time and several heavy C builds each
/// running flat out is worse than either extreme. `BuildScriptJobs` overrides
/// it for anyone who would rather pick.
fn default_jobs(configured: Option<usize>) -> usize {
    configured.filter(|n| *n > 0).unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(|n| (n.get() / 2).max(1))
            .unwrap_or(1)
    })
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
    errors: Vec<String>,
}

pub fn run(args: BuildScriptArgs) -> Result<()> {
    // 1. Parse Cargo.toml for package metadata
    // Use from_slice() instead of from_path() to avoid filesystem traversal
    // that fails in Please sandbox (from_path calls complete_from_path which
    // traverses parent directories looking for workspace Cargo.toml)
    let manifest_content = fs::read(&args.manifest_path)
        .with_context(|| format!("Failed to read {}", args.manifest_path.display()))?;
    let manifest = crate::resolve::parse_manifest(&manifest_content)
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

    // 3. Build environment variables (cargo sets these at compile time of the
    //    build script too, e.g. for env!("CARGO_PKG_VERSION") in build.rs)
    let env = build_environment(&args, pkg, &out_dir)?;

    // 4. Compile build.rs as a binary
    let edition = match pkg.edition.get() {
        Ok(cargo_toml::Edition::E2015) => "2015",
        Ok(cargo_toml::Edition::E2018) => "2018",
        Ok(cargo_toml::Edition::E2024) => "2024",
        _ => "2021",
    };
    let build_script_binary = compile_build_script(&args, &out_dir, edition, &env)?;

    // 5. Execute build script from the package root (cargo contract)
    let manifest_dir = PathBuf::from(env.get("CARGO_MANIFEST_DIR").cloned().unwrap_or_else(|| ".".to_string()));
    let directives = execute_build_script(&build_script_binary, &env, &manifest_dir)?;

    // 6. Print warnings; error directives fail the build (cargo semantics)
    for warning in &directives.warnings {
        eprintln!("warning: {}", warning);
    }
    if !directives.errors.is_empty() {
        for e in &directives.errors {
            eprintln!("error: {}", e);
        }
        anyhow::bail!("build script of {} reported errors", pkg.name);
    }

    // 7. Write directives to output file
    write_directives(&args.output, &directives, &out_dir, pkg.links.as_deref())?;

    eprintln!(
        "please_rust build-script: Generated {} for {}",
        args.output.display(),
        pkg.name
    );

    Ok(())
}

fn compile_build_script(
    args: &BuildScriptArgs,
    out_dir: &Path,
    edition: &str,
    env: &HashMap<String, String>,
) -> Result<PathBuf> {
    let binary_path = out_dir.join("build_script");

    let mut cmd = Command::new(&args.rustc);

    cmd.arg(&args.build_script)
        .arg("--crate-name=build_script")
        .arg("--crate-type=bin")
        .arg(format!("--edition={}", edition))
        .arg("-o")
        .arg(&binary_path)
        .arg("--cap-lints=allow");

    // Cargo compiles build scripts with the crate's feature cfgs — build.rs
    // commonly branches on cfg!(feature = "..."), e.g. proc-macro2 only emits
    // wrap_proc_macro (real compiler spans) when it sees its proc-macro
    // feature at compile time.
    for feature in &args.features {
        cmd.arg("--cfg").arg(format!("feature=\"{}\"", feature));
    }

    // Cargo exposes the package env vars at compile time as well as run time
    // (build scripts may use env!("CARGO_PKG_VERSION") etc.)
    for (key, value) in env {
        cmd.env(key, value);
    }

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
                if let Some((key, filename)) = line.split_once('=') {
                    let (name, qualifier) = crate::compile::split_externconfig_key(key);
                    // Search for the file
                    if let Some(path) = find_file_recursive(".", filename.trim()) {
                        cmd.arg("--extern").arg(format!("{}={}", name, path.display()));
                        // A renamed dependency is imported under its alias as
                        // well, which is the only name its source knows. The
                        // right-hand side names the declaration, since a crate
                        // setting [lib] name is not called after its package.
                        for rename in &args.renames {
                            if let Some((alias, target)) = rename.split_once('=') {
                                let (want_name, want_qual) =
                                    crate::compile::split_externconfig_key(target);
                                let name_ok = want_name.is_empty() || want_name == name;
                                let qual_ok = match (want_qual, qualifier) {
                                    (Some(w), Some(have)) => {
                                        have == w || have.ends_with(&format!("/{}", w))
                                    }
                                    (Some(_), None) => false,
                                    _ => true,
                                };
                                if name_ok && qual_ok {
                                    cmd.arg("--extern")
                                        .arg(format!("{}={}", alias, path.display()));
                                }
                            }
                        }
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
    env.insert(
        "CARGO_MANIFEST_PATH".to_string(),
        manifest_dir.join("Cargo.toml").display().to_string(),
    );
    env.insert("OUT_DIR".to_string(), out_dir.display().to_string());
    env.insert("TARGET".to_string(), args.target.clone());
    env.insert("HOST".to_string(), args.host.clone());
    env.insert("NUM_JOBS".to_string(), default_jobs(args.jobs).to_string());
    env.insert("RUSTC".to_string(), args.rustc.display().to_string());
    env.insert("RUSTDOC".to_string(), "rustdoc".to_string());

    // DEP_<LINKS>_<KEY> vars from direct dependencies' build scripts
    for path in &args.dep_metadata {
        for (key, value) in read_dep_metadata(path)? {
            env.insert(key, value);
        }
    }

    // Hermetic C toolchain for cc-crate build scripts
    if let Some((cc, cxx, ar, ranlib)) = resolve_cc(&args.cc) {
        env.insert("CC".to_string(), cc);
        env.insert("CXX".to_string(), cxx);
        env.insert("AR".to_string(), ar);
        env.insert("RANLIB".to_string(), ranlib);
    }

    // Probing build scripts (autocfg etc.) invoke $RUSTC themselves and honor
    // RUSTFLAGS; without the sysroot every probe fails as "can't find core"
    // and crates silently configure themselves for no_std.
    if let Some(sysroot) = &args.sysroot {
        let sysroot_abs = sysroot.canonicalize().unwrap_or_else(|_| sysroot.clone());
        env.insert("RUSTFLAGS".to_string(), format!("--sysroot {}", sysroot_abs.display()));
        env.insert(
            "CARGO_ENCODED_RUSTFLAGS".to_string(),
            format!("--sysroot\u{1f}{}", sysroot_abs.display()),
        );
    }

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

    // Package metadata (CARGO_PKG_*)
    for (key, value) in package_env(pkg) {
        env.insert(key, value);
    }

    // Feature environment variables
    for feature in &args.features {
        let feature_upper = feature.replace("-", "_").to_uppercase();
        env.insert(format!("CARGO_FEATURE_{}", feature_upper), "1".to_string());
    }

    // Target cfg variables, derived from the triple's real target info
    if let Some(info) = cfg_expr::targets::get_builtin_target_by_triple(&args.target) {
        if let Some(os) = &info.os {
            env.insert("CARGO_CFG_TARGET_OS".to_string(), os.as_str().to_string());
        }
        env.insert("CARGO_CFG_TARGET_ARCH".to_string(), info.arch.as_str().to_string());
        env.insert(
            "CARGO_CFG_TARGET_VENDOR".to_string(),
            info.vendor.as_ref().map(|v| v.as_str()).unwrap_or("").to_string(),
        );
        env.insert(
            "CARGO_CFG_TARGET_ENV".to_string(),
            info.env.as_ref().map(|e| e.as_str()).unwrap_or("").to_string(),
        );
        env.insert(
            "CARGO_CFG_TARGET_POINTER_WIDTH".to_string(),
            info.pointer_width.to_string(),
        );
        env.insert(
            "CARGO_CFG_TARGET_ENDIAN".to_string(),
            format!("{:?}", info.endian).to_lowercase(),
        );
        let families: Vec<&str> = info.families.iter().map(|f| f.as_str()).collect();
        env.insert("CARGO_CFG_TARGET_FAMILY".to_string(), families.join(","));
        for f in &families {
            if *f == "unix" {
                env.insert("CARGO_CFG_UNIX".to_string(), "".to_string());
            } else if *f == "windows" {
                env.insert("CARGO_CFG_WINDOWS".to_string(), "".to_string());
            }
        }
        if args.target.contains("x86_64") {
            env.insert("CARGO_CFG_TARGET_FEATURE".to_string(), "fxsr,sse,sse2".to_string());
            env.insert("CARGO_CFG_TARGET_HAS_ATOMIC".to_string(), "8,16,32,64,ptr".to_string());
        } else if args.target.contains("aarch64") {
            env.insert("CARGO_CFG_TARGET_HAS_ATOMIC".to_string(), "8,16,32,64,128,ptr".to_string());
        }
    }

    // The links key, when present, is exposed to the build script
    if let Some(links) = &pkg.links {
        env.insert("CARGO_MANIFEST_LINKS".to_string(), links.clone());
    }

    Ok(env)
}

/// Package metadata environment variables (CARGO_PKG_*).
///
/// Cargo sets these both when running build scripts and when compiling the
/// crate itself, so this is shared with the compile subcommand.
pub fn package_env(pkg: &cargo_toml::Package) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = Vec::new();

    env.push(("CARGO_PKG_NAME".to_string(), pkg.name.clone()));

    // CARGO_PKG_VERSION and components
    // pkg.version is Inheritable<String>, .get() returns Result<&String, Error>
    let version_str = pkg.version.get()
        .map(|v| v.clone())
        .unwrap_or_else(|_| "0.0.0".to_string());
    env.push(("CARGO_PKG_VERSION".to_string(), version_str.clone()));

    // Version components, parsed properly (splitting on '.' loses the tail
    // of dotted pre-release identifiers like 1.2.3-beta.1)
    match semver::Version::parse(&version_str) {
        Ok(v) => {
            env.push(("CARGO_PKG_VERSION_MAJOR".to_string(), v.major.to_string()));
            env.push(("CARGO_PKG_VERSION_MINOR".to_string(), v.minor.to_string()));
            env.push(("CARGO_PKG_VERSION_PATCH".to_string(), v.patch.to_string()));
            env.push(("CARGO_PKG_VERSION_PRE".to_string(), v.pre.as_str().to_string()));
        }
        Err(_) => {
            env.push(("CARGO_PKG_VERSION_MAJOR".to_string(), "0".to_string()));
            env.push(("CARGO_PKG_VERSION_MINOR".to_string(), "0".to_string()));
            env.push(("CARGO_PKG_VERSION_PATCH".to_string(), "0".to_string()));
            env.push(("CARGO_PKG_VERSION_PRE".to_string(), "".to_string()));
        }
    }

    // CARGO_PKG_AUTHORS - deprecated but still used by some build scripts
    let authors = pkg.authors.get().map(|a| a.join(":")).unwrap_or_default();
    env.push(("CARGO_PKG_AUTHORS".to_string(), authors));

    // Optional string metadata; cargo sets empty strings when absent
    let opt = |field: &Option<cargo_toml::Inheritable<String>>| -> String {
        field.as_ref().and_then(|f| f.get().ok()).cloned().unwrap_or_default()
    };
    env.push(("CARGO_PKG_DESCRIPTION".to_string(), opt(&pkg.description)));
    env.push(("CARGO_PKG_HOMEPAGE".to_string(), opt(&pkg.homepage)));
    env.push(("CARGO_PKG_REPOSITORY".to_string(), opt(&pkg.repository)));
    env.push(("CARGO_PKG_LICENSE".to_string(), opt(&pkg.license)));
    env.push((
        "CARGO_PKG_LICENSE_FILE".to_string(),
        pkg.license_file.as_ref().and_then(|f| f.get().ok()).map(|p| p.display().to_string()).unwrap_or_default(),
    ));
    env.push(("CARGO_PKG_RUST_VERSION".to_string(), opt(&pkg.rust_version)));

    // CARGO_PKG_README - pkg.readme is Inheritable<OptionalFile>
    // OptionalFile is complex, just set empty for now if not easily extractable
    env.push(("CARGO_PKG_README".to_string(), "".to_string()));

    env
}

fn execute_build_script(
    binary_path: &Path,
    env: &HashMap<String, String>,
    manifest_dir: &Path,
) -> Result<Directives> {
    let mut cmd = Command::new(binary_path);

    // Cargo runs build scripts with cwd = the package root
    cmd.current_dir(manifest_dir);

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
        directives.errors.push(value.to_string());
    } else if let Some(value) = directive.strip_prefix("rustc-flags=") {
        // Legacy directive: whitespace-separated -l / -L flags
        let mut it = value.split_whitespace().peekable();
        while let Some(tok) = it.next() {
            if let Some(rest) = tok.strip_prefix("-l") {
                let v = if rest.is_empty() { it.next().unwrap_or("") } else { rest };
                if !v.is_empty() {
                    directives.rustc_link_libs.push(v.to_string());
                }
            } else if let Some(rest) = tok.strip_prefix("-L") {
                let v = if rest.is_empty() { it.next().unwrap_or("") } else { rest };
                if !v.is_empty() {
                    directives.rustc_link_searches.push(v.to_string());
                }
            }
        }
    } else if directive.starts_with("rerun-if") {
        // Not relevant under plz's input hashing
    } else if let Some((key, val)) = directive.split_once('=') {
        // Cargo treats any other key=value directive as metadata
        // (the classic form: cargo:include=/path)
        directives.metadata.push((key.to_string(), val.to_string()));
    }
}

fn write_directives(output: &Path, directives: &Directives, out_dir: &Path, links: Option<&str>) -> Result<()> {
    let mut content = String::new();

    content.push_str("# Generated by please_rust build-script\n");
    content.push_str(&format!("# OUT_DIR={}\n", out_dir.display()));

    // Record OUT_DIR by name only: this sandbox's absolute path is gone by the
    // time the crate compiles, so compile resolves it relative to this file.
    let out_dir_name = out_dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "out".to_string());
    content.push_str(&format!("out-dir={}\n", out_dir_name));

    // Self-describing links key so dependents' build scripts can map this
    // file's metadata to DEP_<LINKS>_<KEY> env vars
    if let Some(links) = links {
        content.push_str(&format!("links={}\n", links));
    }

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

/// Reads a dependency's buildscript file and returns DEP_<LINKS>_<KEY>
/// env pairs from its links key and metadata directives (cargo semantics:
/// both segments uppercased with dashes as underscores).
pub fn read_dep_metadata(path: &Path) -> Result<Vec<(String, String)>> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read dep buildscript {}", path.display()))?;
    let mut links = None;
    let mut metadata = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("links=") {
            links = Some(v.to_string());
        } else if let Some(v) = line.strip_prefix("metadata=") {
            if let Some((k, val)) = v.split_once('=') {
                metadata.push((k.to_string(), val.to_string()));
            }
        }
    }
    let envify = |s: &str| s.to_uppercase().replace('-', "_");
    Ok(match links {
        Some(links) => metadata
            .into_iter()
            .map(|(k, v)| (format!("DEP_{}_{}", envify(&links), envify(&k)), v))
            .collect(),
        None => vec![],
    })
}

/// Resolve a C toolchain path (cc binary or directory of wrappers) to
/// absolute cc/c++/ar/ranlib paths.
pub fn resolve_cc(cc: &Option<PathBuf>) -> Option<(String, String, String, String)> {
    let cc = cc.as_ref()?;
    let abs = match cc.canonicalize() {
        Ok(p) => p,
        // A bare command name (e.g. "cc"): pass through for PATH resolution
        Err(_) => {
            let name = cc.display().to_string();
            return Some((name, "c++".to_string(), "ar".to_string(), "ranlib".to_string()));
        }
    };
    if abs.is_dir() {
        Some((
            abs.join("cc").display().to_string(),
            abs.join("c++").display().to_string(),
            abs.join("ar").display().to_string(),
            abs.join("ranlib").display().to_string(),
        ))
    } else {
        let dir = abs.parent()?;
        let sibling = |n: &str, fallback: &str| {
            let p = dir.join(n);
            if p.exists() { p.display().to_string() } else { fallback.to_string() }
        };
        Some((
            abs.display().to_string(),
            sibling("c++", "c++"),
            sibling("ar", "ar"),
            sibling("ranlib", "ranlib"),
        ))
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(lines: &[&str]) -> Directives {
        let mut d = Directives::default();
        for line in lines {
            parse_directive(line, &mut d);
        }
        d
    }

    #[test]
    fn parses_directive_forms() {
        let d = parsed(&[
            "cargo:include=/classic/metadata/form",
            "cargo:rerun-if-changed=build.rs",
            "cargo:rustc-cfg=old_style",
            "cargo::rustc-cfg=new_style",
            "cargo:rustc-env=K=V",
            "cargo:rustc-link-lib=z",
            "cargo:rustc-link-search=/dir",
            "cargo:rustc-link-arg=-s",
            "cargo:metadata=root=/x",
            "cargo:warning=heads up",
            "cargo:error=broken",
            "not a directive",
        ]);
        assert_eq!(d.rustc_cfgs, vec!["old_style", "new_style"]);
        assert_eq!(d.rustc_envs, vec![("K".to_string(), "V".to_string())]);
        assert_eq!(d.rustc_link_libs, vec!["z"]);
        assert_eq!(d.rustc_link_searches, vec!["/dir"]);
        assert_eq!(d.rustc_link_args, vec!["-s"]);
        assert_eq!(d.metadata, vec![
            ("include".to_string(), "/classic/metadata/form".to_string()),
            ("root".to_string(), "/x".to_string()),
        ]);
        assert_eq!(d.warnings, vec!["heads up"]);
        assert_eq!(d.errors, vec!["broken"]);
    }

    #[test]
    fn parses_legacy_rustc_flags() {
        let d = parsed(&["cargo:rustc-flags=-l z -L /a -lfoo -L/b"]);
        assert_eq!(d.rustc_link_libs, vec!["z", "foo"]);
        assert_eq!(d.rustc_link_searches, vec!["/a", "/b"]);
    }

    #[test]
    fn package_env_versions() {
        let manifest = crate::resolve::parse_manifest(
            b"[package]\nname = \"demo\"\nversion = \"1.2.3-beta.1\"\nauthors = [\"A\", \"B\"]\ndescription = \"d\"\nlicense = \"MIT\"\n",
        )
        .unwrap();
        let env: std::collections::HashMap<String, String> =
            package_env(manifest.package.as_ref().unwrap()).into_iter().collect();
        assert_eq!(env["CARGO_PKG_NAME"], "demo");
        assert_eq!(env["CARGO_PKG_VERSION"], "1.2.3-beta.1");
        assert_eq!(env["CARGO_PKG_VERSION_MAJOR"], "1");
        assert_eq!(env["CARGO_PKG_VERSION_MINOR"], "2");
        assert_eq!(env["CARGO_PKG_VERSION_PATCH"], "3");
        assert_eq!(env["CARGO_PKG_VERSION_PRE"], "beta.1");
        assert_eq!(env["CARGO_PKG_AUTHORS"], "A:B");
        assert_eq!(env["CARGO_PKG_LICENSE"], "MIT");
        assert_eq!(env["CARGO_PKG_HOMEPAGE"], "");
    }

    #[test]
    fn resolve_cc_forms() {
        // Bare command name passes through
        let (cc, cxx, ar, _) = resolve_cc(&Some(PathBuf::from("cc"))).unwrap();
        assert_eq!(cc, "cc");
        assert_eq!(cxx, "c++");
        assert_eq!(ar, "ar");

        // Directory of wrappers
        let dir = std::env::temp_dir().join(format!("please_rust_cc_test_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        for t in ["cc", "c++", "ar", "ranlib"] {
            fs::write(dir.join(t), "").unwrap();
        }
        let (cc, _, ar, _) = resolve_cc(&Some(dir.clone())).unwrap();
        assert!(cc.ends_with("/cc"));
        assert!(ar.ends_with("/ar"));

        assert!(resolve_cc(&None).is_none());
    }
}

#[cfg(test)]
mod env_tests {
    use super::*;

    fn args(dir: &Path) -> BuildScriptArgs {
        BuildScriptArgs {
            manifest_path: dir.join("Cargo.toml"),
            build_script: dir.join("build.rs"),
            out_dir: dir.join("out"),
            rustc: PathBuf::from("/toolchain/rustc"),
            target: "x86_64-unknown-linux-gnu".to_string(),
            host: "x86_64-unknown-linux-gnu".to_string(),
            features: vec!["std".to_string(), "extra-fast".to_string()],
            debug: false,
            optimize: true,
            output: dir.join("x.buildscript"),
            sysroot: None,
            search_paths: vec![],
            externconfig: None,
            renames: vec![],
            cc: None,
            dep_metadata: vec![],
            jobs: None,
        }
    }

    #[test]
    fn environment_contract() {
        let dir = std::env::temp_dir().join(format!("please_rust_env_test_{}", std::process::id()));
        fs::create_dir_all(dir.join("out")).unwrap();
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.2.0\"\nlinks = \"zlib\"\n",
        )
        .unwrap();
        let a = args(&dir);
        let manifest = crate::resolve::parse_manifest(&fs::read(&a.manifest_path).unwrap()).unwrap();
        let pkg = manifest.package.as_ref().unwrap();
        let out_dir = a.out_dir.canonicalize().unwrap();
        let env = build_environment(&a, pkg, &out_dir).unwrap();

        assert_eq!(env["CARGO"], "/bin/false");
        assert!(env["CARGO_MANIFEST_PATH"].ends_with("Cargo.toml"));
        assert_eq!(env["TARGET"], "x86_64-unknown-linux-gnu");
        assert_eq!(env["PROFILE"], "release");
        assert_eq!(env["OPT_LEVEL"], "3");
        assert_eq!(env["CARGO_FEATURE_STD"], "1");
        assert_eq!(env["CARGO_FEATURE_EXTRA_FAST"], "1");
        assert_eq!(env["CARGO_MANIFEST_LINKS"], "zlib");
        // Target cfgs derived from real target info
        assert_eq!(env["CARGO_CFG_TARGET_OS"], "linux");
        assert_eq!(env["CARGO_CFG_TARGET_ARCH"], "x86_64");
        assert_eq!(env["CARGO_CFG_TARGET_ENV"], "gnu");
        assert_eq!(env["CARGO_CFG_TARGET_POINTER_WIDTH"], "64");
        assert_eq!(env["CARGO_CFG_TARGET_ENDIAN"], "little");
        assert!(env.contains_key("CARGO_CFG_UNIX"));
        assert_eq!(env["RUSTC"], "/toolchain/rustc");
    }

    #[test]
    fn sysroot_sets_rustflags_for_probes() {
        let dir = std::env::temp_dir().join(format!("please_rust_env_rf_test_{}", std::process::id()));
        fs::create_dir_all(dir.join("sysroot")).unwrap();
        fs::create_dir_all(dir.join("out")).unwrap();
        fs::write(dir.join("Cargo.toml"), "[package]\nname = \"d\"\nversion = \"0.1.0\"\n").unwrap();
        let mut a = args(&dir);
        a.sysroot = Some(dir.join("sysroot"));
        let manifest = crate::resolve::parse_manifest(&fs::read(&a.manifest_path).unwrap()).unwrap();
        let out_dir = a.out_dir.canonicalize().unwrap();
        let env = build_environment(&a, manifest.package.as_ref().unwrap(), &out_dir).unwrap();
        assert!(env["RUSTFLAGS"].starts_with("--sysroot "));
        assert!(env["CARGO_ENCODED_RUSTFLAGS"].contains('\u{1f}'));
    }

    #[test]
    fn write_directives_round_trip() {
        let dir = std::env::temp_dir().join(format!("please_rust_wd_test_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let out_dir = dir.join("out");
        fs::create_dir_all(&out_dir).unwrap();
        let mut d = Directives::default();
        d.rustc_cfgs.push("has_std".to_string());
        d.rustc_envs.push(("K".to_string(), "V".to_string()));
        d.rustc_link_libs.push("z".to_string());
        d.rustc_link_searches.push("/dir".to_string());
        d.rustc_link_args.push("-s".to_string());
        d.metadata.push(("inc".to_string(), "/i".to_string()));
        let output = dir.join("x.buildscript");
        write_directives(&output, &d, &out_dir, Some("zlib")).unwrap();

        // The compile side parses what the build-script side writes
        let parsed = crate::compile::parse_buildscript(&output).unwrap();
        assert_eq!(parsed.out_dir.as_deref(), Some(Path::new("out")));
        assert_eq!(parsed.rustc_cfgs, vec!["has_std"]);
        assert_eq!(parsed.rustc_envs, vec![("K".to_string(), "V".to_string())]);
        assert_eq!(parsed.rustc_link_libs, vec!["z"]);
        assert_eq!(parsed.rustc_link_searches, vec!["/dir"]);
        assert_eq!(parsed.rustc_link_args, vec!["-s"]);
    }
}

#[cfg(test)]
mod run_e2e_tests {
    use super::*;

    /// Full pipeline: compile a real build.rs, run it, parse its directives.
    /// Skips when no rustc is reachable (e.g. inside a build sandbox).
    #[test]
    fn compiles_and_runs_a_build_script() {
        if Command::new("rustc").arg("--version").output().is_err() {
            eprintln!("skipping: no rustc on PATH");
            return;
        }
        let dir = std::env::temp_dir().join(format!("please_rust_bs_e2e_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("Cargo.toml"), "[package]\nname = \"demo\"\nversion = \"0.3.0\"\n").unwrap();
        fs::write(dir.join("wanted.txt"), "").unwrap();
        fs::write(
            dir.join("build.rs"),
            r#"fn main() {
    // Reads a file relative to the package root (the cargo cwd contract)
    assert!(std::path::Path::new("wanted.txt").exists());
    assert_eq!(std::env::var("CARGO_PKG_VERSION").unwrap(), "0.3.0");
    let out = std::env::var("OUT_DIR").unwrap();
    std::fs::write(format!("{}/generated.rs", out), "pub const X: u32 = 7;").unwrap();
    println!("cargo:rustc-cfg=from_script");
    println!("cargo:rustc-env=GENERATED=yes");
    println!("cargo:warning=all good");
}"#,
        )
        .unwrap();

        run(BuildScriptArgs {
            manifest_path: dir.join("Cargo.toml"),
            build_script: dir.join("build.rs"),
            out_dir: dir.join("out"),
            rustc: PathBuf::from("rustc"),
            target: "x86_64-unknown-linux-gnu".to_string(),
            host: "x86_64-unknown-linux-gnu".to_string(),
            features: vec![],
            debug: false,
            optimize: false,
            output: dir.join("demo.buildscript"),
            sysroot: None,
            search_paths: vec![],
            externconfig: None,
            renames: vec![],
            cc: None,
            dep_metadata: vec![],
            jobs: None,
        })
        .unwrap();

        let directives = fs::read_to_string(dir.join("demo.buildscript")).unwrap();
        assert!(directives.contains("rustc-cfg=from_script"));
        assert!(directives.contains("rustc-env=GENERATED=yes"));
        assert!(directives.contains("out-dir=out"));
        assert!(dir.join("out/generated.rs").exists());
    }

    #[test]
    fn error_directive_fails_the_build() {
        if Command::new("rustc").arg("--version").output().is_err() {
            eprintln!("skipping: no rustc on PATH");
            return;
        }
        let dir = std::env::temp_dir().join(format!("please_rust_bs_err_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("Cargo.toml"), "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n").unwrap();
        fs::write(dir.join("build.rs"), "fn main() { println!(\"cargo::error=nope\"); }").unwrap();
        let err = run(BuildScriptArgs {
            manifest_path: dir.join("Cargo.toml"),
            build_script: dir.join("build.rs"),
            out_dir: dir.join("out"),
            rustc: PathBuf::from("rustc"),
            target: "x86_64-unknown-linux-gnu".to_string(),
            host: "x86_64-unknown-linux-gnu".to_string(),
            features: vec![],
            debug: false,
            optimize: false,
            output: dir.join("demo.buildscript"),
            sysroot: None,
            search_paths: vec![],
            externconfig: None,
            renames: vec![],
            cc: None,
            dep_metadata: vec![],
            jobs: None,
        })
        .unwrap_err();
        assert!(err.to_string().contains("reported errors"));
    }
}

#[cfg(test)]
mod dep_metadata_tests {
    use super::*;

    /// NUM_JOBS reaches cc-rs and cmake-rs and nothing else, so it is the
    /// width of a -sys crate's C build. Unset it is half the machine, because
    /// plz is scheduling other actions beside this one; a configured number
    /// wins, and a nonsensical one does not become a hang.
    #[test]
    fn jobs_defaults_to_half_the_machine() {
        assert_eq!(default_jobs(Some(4)), 4);
        assert_eq!(default_jobs(Some(1)), 1);
        // Zero would mean "no compilers", which is not a thing to ask for.
        assert_eq!(default_jobs(Some(0)), default_jobs(None));
        let half = std::thread::available_parallelism()
            .map(|n| (n.get() / 2).max(1))
            .unwrap_or(1);
        assert_eq!(default_jobs(None), half);
        assert!(default_jobs(None) >= 1);
    }

    #[test]
    fn links_metadata_becomes_dep_env() {
        let dir = std::env::temp_dir().join(format!("please_rust_depmeta_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let out_dir = dir.join("out");
        fs::create_dir_all(&out_dir).unwrap();

        let mut d = Directives::default();
        d.metadata.push(("include".to_string(), "/some/include".to_string()));
        d.metadata.push(("lib-kind".to_string(), "static".to_string()));
        let bs = dir.join("z.buildscript");
        write_directives(&bs, &d, &out_dir, Some("z-lib")).unwrap();

        let env = read_dep_metadata(&bs).unwrap();
        assert!(env.contains(&("DEP_Z_LIB_INCLUDE".to_string(), "/some/include".to_string())));
        assert!(env.contains(&("DEP_Z_LIB_LIB_KIND".to_string(), "static".to_string())));

        // No links key -> no exports
        let bs2 = dir.join("plain.buildscript");
        write_directives(&bs2, &d, &out_dir, None).unwrap();
        assert!(read_dep_metadata(&bs2).unwrap().is_empty());
    }
}
