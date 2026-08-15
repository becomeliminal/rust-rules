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

/// Resolve the OUT_DIR recorded in a buildscript directives file to an
/// absolute path. Tries, in order: relative to the directives file itself
/// (the normal case — the out dir is a sibling output of the same rule),
/// then as given (absolute paths from older directive files).
fn resolve_out_dir(recorded: &Path, buildscript: Option<&Path>) -> Option<PathBuf> {
    if let Some(bs) = buildscript {
        if let Some(parent) = bs.parent() {
            let candidate = parent.join(recorded);
            if candidate.is_dir() {
                return candidate.canonicalize().ok();
            }
        }
    }
    if recorded.is_dir() {
        return recorded.canonicalize().ok();
    }
    None
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

#[derive(Args)]
pub struct CompileArgs {
    /// Path to externconfig file (contains crate_name=/path/to/lib.rlib lines)
    #[arg(long)]
    pub externconfig: Option<PathBuf>,

    /// Path to buildscript output file (contains rustc-cfg, rustc-link-lib, etc.)
    #[arg(long)]
    pub buildscript: Option<PathBuf>,

    /// Path to Cargo.toml; sets CARGO_PKG_*/CARGO_MANIFEST_DIR for the compile
    /// (crates may use env!("CARGO_PKG_VERSION") etc. in normal source)
    #[arg(long)]
    pub manifest_path: Option<PathBuf>,

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

    /// Additional -C codegen options (e.g. metadata=..., extra-filename=...)
    #[arg(short = 'C', long = "codegen")]
    pub codegen: Vec<String>,

    /// Cap lints at this level (cargo passes allow for registry crates)
    #[arg(long)]
    pub cap_lints: Option<String>,

    /// Direct dependency crate names. When given, --extern is added only for
    /// externconfig entries matching these; everything else in the sandbox
    /// stays reachable via -L only (transitive deps, as cargo does).
    #[arg(long = "dep")]
    pub deps: Vec<String>,

    /// Features to enable
    #[arg(long = "feature")]
    pub features: Vec<String>,

    /// Dependency renames as depname=cratename (e.g. libc_errno=errno for
    /// deps declared with package = "..."). Adds --extern depname=<path of cratename>.
    #[arg(long = "rename")]
    pub renames: Vec<String>,

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

    // The proc_macro crate is compiler-provided; cargo injects it into the
    // extern prelude for proc-macro crates.
    if args.crate_type == "proc-macro" {
        cmd.arg("--extern").arg("proc_macro");
    }

    // Parse externconfig and add --extern flags
    // The externconfig contains lines like: crate_name=libcrate.rlib
    // We search for the actual file in the current directory tree
    let mut extern_paths: Vec<(String, PathBuf)> = Vec::new();
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
                        let direct = args.deps.is_empty() || args.deps.iter().any(|d| d == name);
                        if direct {
                            cmd.arg("--extern");
                            cmd.arg(format!("{}={}", name, path.display()));
                            extern_paths.push((name.to_string(), path.clone()));
                        }
                        // -L keeps transitive crates resolvable by metadata hash
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

    // Renamed deps: source refers to them by the rename, so add an extra
    // --extern under that name pointing at the real crate's library.
    for rename in &args.renames {
        if let Some((dep_name, crate_name)) = rename.split_once('=') {
            if let Some((_, path)) = extern_paths.iter().find(|(n, _)| n == crate_name.trim()) {
                cmd.arg("--extern");
                cmd.arg(format!("{}={}", dep_name.trim(), path.display()));
            } else {
                eprintln!("Warning: rename {}: crate {} not found in externconfig", dep_name, crate_name);
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
    if let Some(level) = &args.cap_lints {
        cmd.arg(format!("--cap-lints={}", level));
    }
    cmd.arg("-C").arg("embed-bitcode=no");
    for opt in &args.codegen {
        cmd.arg("-C").arg(opt);
    }

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

        // Resolve OUT_DIR and expose it both as a search path and as the
        // OUT_DIR env var (for include!(concat!(env!("OUT_DIR"), ...))).
        // The directives file records it relative to itself, since the
        // build-script rule's sandbox no longer exists at compile time.
        if let Some(ref out_dir) = directives.out_dir {
            if let Some(resolved) = resolve_out_dir(out_dir, args.buildscript.as_deref()) {
                cmd.arg("-L").arg(&resolved);
                cmd.env("OUT_DIR", &resolved);
            } else {
                eprintln!("Warning: OUT_DIR {} not found", out_dir.display());
            }
        }
    }

    // Package metadata env vars from Cargo.toml
    if let Some(mp) = &args.manifest_path {
        if mp.exists() {
            let content = fs::read(mp)
                .with_context(|| format!("Failed to read {}", mp.display()))?;
            if let Ok(manifest) = crate::resolve::parse_manifest(&content) {
                if let Some(pkg) = &manifest.package {
                    for (key, value) in crate::build_script::package_env(pkg) {
                        cmd.env(key, value);
                    }
                }
            }
            let manifest_dir = match mp.parent() {
                Some(p) if !p.as_os_str().is_empty() => p.canonicalize().ok(),
                _ => std::env::current_dir().ok(),
            };
            if let Some(dir) = manifest_dir {
                cmd.env("CARGO_MANIFEST_DIR", dir);
            }
            cmd.env("CARGO", "/bin/false");
        }
    }
    cmd.env("CARGO_CRATE_NAME", &args.crate_name);
    if args.crate_type == "bin" {
        cmd.env("CARGO_BIN_NAME", &args.crate_name);
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
