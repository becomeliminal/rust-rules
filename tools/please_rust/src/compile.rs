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

/// Rewrite a path the build script emitted so it points at where its output
/// actually is now.
///
/// A build script that compiles a C library writes it into OUT_DIR and emits
/// an absolute search path into that directory - which belongs to the build
/// script's sandbox and is gone by the time anything links. bzip2-sys,
/// curl-sys and libgit2-sys all do this, and the failure is rustc reporting
/// it cannot find a native static library that was built successfully.
pub(crate) fn rebase_build_path(
    raw: &str,
    built_out_dir: Option<&Path>,
    resolved_out_dir: Option<&Path>,
) -> String {
    if let (Some(old), Some(new)) = (built_out_dir, resolved_out_dir) {
        let old = old.to_string_lossy();
        if let Some(rest) = raw.strip_prefix(old.as_ref()) {
            return format!("{}{}", new.display(), rest);
        }
    }
    raw.to_string()
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

/// Split an externconfig key into the crate name and the declaration that
/// produced it. `syn` and `syn@third_party/crates/syn-2.0.119` both name the
/// crate `syn`; the qualifier exists only to tell two declarations of one
/// crate apart, and is never what rustc is told.
pub fn split_externconfig_key(key: &str) -> (&str, Option<&str>) {
    match key.split_once('@') {
        Some((name, qual)) => (name.trim(), Some(qual.trim())),
        None => (key.trim(), None),
    }
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

    /// Target triple to compile for, when it is not the host's. The sysroot
    /// has to carry a standard library for it; rust_toolchain installs one
    /// for every platform in its architectures list.
    #[arg(long)]
    pub target: Option<String>,

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

    /// C toolchain (cc binary or wrapper directory); used as rustc's linker
    /// for crate types that produce linked artifacts
    #[arg(long)]
    pub cc: Option<PathBuf>,

    /// Direct dependency crate names. When given, --extern is added only for
    /// externconfig entries matching these; everything else in the sandbox
    /// stays reachable via -L only (transitive deps, as cargo does).
    ///
    /// A value may be `name` or `name@qualifier`, the second naming the
    /// declaration wanted. Two versions of one crate both answer to the same
    /// crate name, so a bare name cannot say which was meant and the choice
    /// falls to whichever entry came last.
    #[arg(long = "dep")]
    pub deps: Vec<String>,

    /// Native static libraries (.a) to link into this crate (cc interop);
    /// rustc records the linkage in the rlib for the final link
    #[arg(long = "native", num_args = 0..)]
    pub native: Vec<PathBuf>,

    /// Features to enable
    #[arg(long = "feature")]
    pub features: Vec<String>,

    /// Dependency renames as depname=cratename (e.g. libc_errno=errno for
    /// deps declared with package = "..."). Adds --extern depname=<path of cratename>.
    #[arg(long = "rename")]
    pub renames: Vec<String>,

    /// Statically link the C runtime (crt-static)
    #[arg(long = "static")]
    pub static_crt: bool,

    /// Instrument for coverage (-C instrument-coverage)
    #[arg(long)]
    pub coverage: bool,

    /// Extra flags passed to rustc verbatim (e.g. -Dwarnings for clippy)
    #[arg(long = "rustc-flag")]
    pub rustc_flags: Vec<String>,

    /// Pipelined metadata compile: run the full compile but terminate rustc
    /// as soon as the .rmeta artifact is emitted (cargo/rules_rust scheme; a
    /// plain --emit=metadata rmeta lacks the optimized MIR dependents need)
    #[arg(long)]
    pub pipeline_rmeta: bool,

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
pub(crate) struct BuildScriptDirectives {
    pub(crate) out_dir: Option<PathBuf>,
    /// Where OUT_DIR was when the script ran. Paths the script emitted point
    /// inside it, and that directory is gone by the time anything compiles.
    pub(crate) built_out_dir: Option<PathBuf>,
    pub(crate) rustc_cfgs: Vec<String>,
    pub(crate) rustc_envs: Vec<(String, String)>,
    pub(crate) rustc_link_libs: Vec<String>,
    pub(crate) rustc_link_searches: Vec<String>,
    pub(crate) rustc_link_args: Vec<String>,
}

pub(crate) fn parse_buildscript(path: &Path) -> Result<BuildScriptDirectives> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read buildscript: {}", path.display()))?;

    let mut directives = BuildScriptDirectives::default();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(value) = line.strip_prefix("# OUT_DIR=") {
            directives.built_out_dir = Some(PathBuf::from(value));
            continue;
        }
        if line.starts_with('#') {
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
    let mut cmd = build_command(&args)?;

    eprintln!("please_rust compile: {:?}", cmd);

    run_rustc(cmd, &args)
}

/// Runs rustc, rendering its JSON diagnostics, and — in pipelined metadata
/// mode — terminates it as soon as the .rmeta artifact is on disk: the
/// codegen this skips belongs to the parallel `#link` action.
fn run_rustc(mut cmd: Command, args: &CompileArgs) -> Result<()> {
    use std::io::BufRead;

    cmd.stderr(std::process::Stdio::piped());
    let mut child = cmd
        .spawn()
        .with_context(|| format!("Failed to execute rustc: {}", args.rustc.display()))?;
    let stderr = child.stderr.take().expect("stderr piped");
    let reader = std::io::BufReader::new(stderr);

    let mut rmeta_emitted = false;
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let parsed: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => {
                eprintln!("{}", line);
                continue;
            }
        };
        if let Some(artifact) = parsed.get("artifact").and_then(|a| a.as_str()) {
            if args.pipeline_rmeta
                && (parsed.get("emit").and_then(|e| e.as_str()) == Some("metadata")
                    || artifact.ends_with(".rmeta"))
            {
                rmeta_emitted = true;
                let _ = child.kill();
                break;
            }
            continue;
        }
        if let Some(rendered) = parsed.get("rendered").and_then(|r| r.as_str()) {
            eprint!("{}", rendered);
        }
    }

    let status = child.wait().context("Failed to wait for rustc")?;
    if rmeta_emitted {
        return Ok(());
    }
    if !status.success() {
        anyhow::bail!("rustc failed with exit code: {}", status.code().unwrap_or(-1));
    }
    Ok(())
}

/// Assembles the full rustc invocation; separated from run() so flag and
/// env construction is unit-testable.
fn build_command(args: &CompileArgs) -> Result<Command> {
    let mut cmd = Command::new(&args.rustc);

    // Set sysroot if provided (tells rustc where to find std/core)
    if let Some(sysroot) = &args.sysroot {
        cmd.arg("--sysroot").arg(sysroot);
    }

    if let Some(target) = &args.target {
        cmd.arg("--target").arg(target);
    }

    // The proc_macro crate is compiler-provided; cargo injects it into the
    // extern prelude for proc-macro crates.
    if args.crate_type == "proc-macro" {
        cmd.arg("--extern").arg("proc_macro");
    }

    // Parse externconfig and add --extern flags
    // The externconfig contains lines like: crate_name=libcrate.rlib
    // We search for the actual file in the current directory tree
    // Crate name, the declaration it came from, and where it landed. The
    // declaration matters for renames: aws-smithy-types depends on http-body
    // twice, as http_body_0_4 and http_body_1_0, and both are the crate
    // `http_body`.
    let mut extern_paths: Vec<(String, Option<String>, PathBuf)> = Vec::new();
    if let Some(config_path) = &args.externconfig {
        if config_path.exists() {
            let content = fs::read_to_string(config_path)
                .with_context(|| format!("Failed to read externconfig: {}", config_path.display()))?;

            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if line.starts_with("native=") {
                    continue; // handled separately below
                }
                if let Some((key, filename)) = line.split_once('=') {
                    // `crate` or `crate@declaration`: the crate name is what
                    // rustc is told, the qualifier only picks between entries.
                    let (name, qualifier) = split_externconfig_key(key);
                    let filename = filename.trim();

                    // Search for the file in current directory tree
                    let found_path = find_file_recursive(".", filename);

                    if let Some(path) = found_path {
                        // Every entry is recorded, whether or not it becomes
                        // an --extern: an alias may point at a version that
                        // the filter deliberately left out.
                        extern_paths.push((
                            name.to_string(),
                            qualifier.map(|q| q.to_string()),
                            path.clone(),
                        ));

                        // A dep may name the declaration it wants. It matches
                        // an unqualified entry too, so a crate built by rules
                        // that do not qualify still resolves.
                        let direct = args.deps.is_empty()
                            || args.deps.iter().any(|d| match d.split_once('@') {
                                Some((dep_name, dep_qual)) => {
                                    // A dep names its label; the crate names
                                    // its subrepo. They agree on the tail,
                                    // which is enough to tell one declaration
                                    // of a crate from another.
                                    dep_name == name
                                        && qualifier.map_or(true, |q| {
                                            q == dep_qual || q.ends_with(&format!("/{}", dep_qual))
                                        })
                                }
                                None => d == name,
                            });
                        if direct {
                            cmd.arg("--extern");
                            cmd.arg(format!("{}={}", name, path.display()));
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

    // Native archives from cc deps: -l on the owning crate records the
    // requirement in its rlib; -L makes the archive findable
    for lib in &args.native {
        let stem = lib
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let name = stem.strip_prefix("lib").unwrap_or(&stem).to_string();
        let abs = lib.canonicalize().unwrap_or_else(|_| lib.clone());
        cmd.arg("-l").arg(format!("static={}", name));
        if let Some(dir) = abs.parent() {
            cmd.arg("-L").arg(format!("native={}", dir.display()));
        }
    }

    // native= lines in externconfigs: archives linked by (transitive) deps;
    // they only need to be locatable at link time
    if let Some(config_path) = &args.externconfig {
        if config_path.exists() {
            let content = fs::read_to_string(config_path)?;
            for line in content.lines() {
                if let Some(filename) = line.trim().strip_prefix("native=") {
                    if let Some(path) = find_file_recursive(".", filename.trim()) {
                        if let Some(dir) = path.canonicalize().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
                            cmd.arg("-L").arg(format!("native={}", dir.display()));
                        }
                    }
                }
            }
        }
    }

    // Renamed deps: source refers to them by the rename, so add an extra
    // --extern under that name pointing at the real crate's library.
    for rename in &args.renames {
        if let Some((dep_name, target)) = rename.split_once('=') {
            // The right-hand side names the declaration, optionally with the
            // crate name in front of it. Naming only the declaration is the
            // more useful form: a crate that sets [lib] name is not called
            // after its package - md-5 builds md5 - so the caller often does
            // not know the crate name, only which declaration it meant.
            let (crate_name, want_qual) = split_externconfig_key(target);
            let found = extern_paths.iter().find(|(n, q, _)| {
                (crate_name.is_empty() || n == crate_name)
                    && match (want_qual, q.as_deref()) {
                        (Some(w), Some(have)) => {
                            have == w || have.ends_with(&format!("/{}", w))
                        }
                        (Some(_), None) => false,
                        _ => true,
                    }
            });
            if let Some((name, _, path)) = found {
                // The alias is an extra name for the same library; the crate
                // keeps its own name too, so code can use either.
                let _ = name;
                cmd.arg("--extern");
                cmd.arg(format!("{}={}", dep_name.trim(), path.display()));
            } else {
                eprintln!("Warning: rename {}: crate {} not found in externconfig", dep_name, target);
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
    // Always JSON: rustc tracks the error-format flags in the crate hash
    // (svh), and pipelined metadata twins must produce the same svh as
    // their #link rules, so every compile uses the same diagnostics flags.
    // The wrapper prints the pre-rendered messages, and the artifact
    // notifications drive the pipelined early-cutoff.
    cmd.arg("--error-format=json");
    cmd.arg("--json=diagnostic-rendered-ansi,artifacts");
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
        // Resolve OUT_DIR first: the directives file records it relative to
        // itself, since the build-script rule's sandbox no longer exists at
        // compile time.
        let resolved_out = directives
            .out_dir
            .as_ref()
            .and_then(|d| resolve_out_dir(d, args.buildscript.as_deref()));

        for path in &directives.rustc_link_searches {
            // Handle KIND=PATH format (e.g., "native=/usr/lib")
            let raw = match path.split_once('=') {
                Some((_kind, actual_path)) => actual_path,
                None => path.as_str(),
            };
            cmd.arg("-L").arg(rebase_build_path(
                raw,
                directives.built_out_dir.as_deref(),
                resolved_out.as_deref(),
            ));
        }

        // Expose OUT_DIR as a search path and as the env var, for
        // include!(concat!(env!("OUT_DIR"), ...)).
        if let Some(ref resolved) = resolved_out {
            cmd.arg("-L").arg(resolved);
            cmd.env("OUT_DIR", resolved);
        } else if let Some(ref out_dir) = directives.out_dir {
            eprintln!("Warning: OUT_DIR {} not found", out_dir.display());
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

    // Hermetic linker for artifacts that link (bins, proc-macro dylibs)
    if args.crate_type == "bin" || args.crate_type == "proc-macro" || args.test {
        if let Some((cc, _, _, _)) = crate::build_script::resolve_cc(&args.cc) {
            cmd.arg("-C").arg(format!("linker={}", cc));
        }
    }

    if args.static_crt {
        cmd.arg("-C").arg("target-feature=+crt-static");
    }
    if args.coverage {
        cmd.arg("-C").arg("instrument-coverage");
        // Linked C archives (cc_deps) are gcov-instrumented under the cover
        // config; --coverage makes the cc driver link libgcov for them.
        if args.crate_type == "bin" || args.test {
            cmd.arg("-C").arg("link-arg=--coverage");
        }
        // Instrumented code that runs *during* a build (a proc-macro rustc
        // expands, say) writes its profile to the cwd unless told otherwise,
        // littering the repo with default_*.profraw. Keep those in the build
        // sandbox; the test wrapper sets its own path for the profiles that
        // actually matter.
        if std::env::var_os("LLVM_PROFILE_FILE").is_none() {
            cmd.env("LLVM_PROFILE_FILE", "build-%p.profraw");
        }
    }
    for flag in &args.rustc_flags {
        cmd.arg(flag);
    }
    // Remap the build sandbox cwd out of all embedded paths. This keeps the
    // crate hash (svh) identical across sandbox directories — required for
    // pipelined #rmeta/#link twins to agree — makes artifacts reproducible
    // byte-for-byte across machines, and gives coverage repo-relative paths.
    if let Ok(cwd) = std::env::current_dir() {
        cmd.arg(format!("--remap-path-prefix={}=", cwd.display()));
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

    Ok(cmd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_buildscript_directives() {
        let dir = std::env::temp_dir().join(format!("please_rust_compile_test_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("x.buildscript");
        fs::write(&path, "# comment\nout-dir=out\nrustc-cfg=has_std\nrustc-env=FOO=bar\nrustc-link-lib=static=z\nrustc-link-search=native=/some/dir\nrustc-link-arg=-Wl,-z,now\nmetadata=include=/inc\n").unwrap();
        let d = parse_buildscript(&path).unwrap();
        assert_eq!(d.out_dir.as_deref(), Some(Path::new("out")));
        assert_eq!(d.rustc_cfgs, vec!["has_std"]);
        assert_eq!(d.rustc_envs, vec![("FOO".to_string(), "bar".to_string())]);
        assert_eq!(d.rustc_link_libs, vec!["static=z"]);
        assert_eq!(d.rustc_link_searches, vec!["native=/some/dir"]);
        assert_eq!(d.rustc_link_args, vec!["-Wl,-z,now"]);
    }

    /// A build script that compiles a C library writes it into OUT_DIR and
    /// emits an absolute search path into that directory - which belongs to
    /// the build script's sandbox and is gone by the time anything links.
    /// bzip2-sys, curl-sys and libgit2-sys all do it.
    #[test]
    fn link_search_paths_follow_the_output() {
        let built = Path::new("/plz-out/tmp/x/_bzip2_sys_build_script._build/bzip2_sys_out");
        let now = Path::new("/plz-out/gen/x/bzip2_sys_out");
        assert_eq!(
            rebase_build_path(
                "/plz-out/tmp/x/_bzip2_sys_build_script._build/bzip2_sys_out/lib",
                Some(built),
                Some(now)
            ),
            "/plz-out/gen/x/bzip2_sys_out/lib"
        );
        // A path outside OUT_DIR is a system path and is left alone
        assert_eq!(
            rebase_build_path("/usr/lib/x86_64-linux-gnu", Some(built), Some(now)),
            "/usr/lib/x86_64-linux-gnu"
        );
        // Nothing to rebase against: unchanged rather than mangled
        assert_eq!(rebase_build_path("/some/dir", None, Some(now)), "/some/dir");
    }

    #[test]
    fn out_dir_resolves_relative_to_buildscript() {
        let dir = std::env::temp_dir().join(format!("please_rust_outdir_test_{}", std::process::id()));
        let out = dir.join("out");
        fs::create_dir_all(&out).unwrap();
        let bs = dir.join("x.buildscript");
        fs::write(&bs, "").unwrap();
        let resolved = resolve_out_dir(Path::new("out"), Some(&bs)).unwrap();
        assert_eq!(resolved, out.canonicalize().unwrap());
        assert!(resolve_out_dir(Path::new("nonexistent"), Some(&bs)).is_none());
    }

    #[test]
    fn find_file_searches_recursively() {
        let dir = std::env::temp_dir().join(format!("please_rust_find_test_{}", std::process::id()));
        let deep = dir.join("a/b");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("needle.rlib"), "").unwrap();
        let found = find_file_recursive(dir.to_str().unwrap(), "needle.rlib").unwrap();
        assert!(found.ends_with("a/b/needle.rlib"));
        assert!(find_file_recursive(dir.to_str().unwrap(), "haystack.rlib").is_none());
    }
}

#[cfg(test)]
mod command_tests {
    use super::*;

    // Tests that chdir must not run concurrently (cwd is process-global)
    static CWD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn base_args(dir: &Path) -> CompileArgs {
        CompileArgs {
            externconfig: Some(dir.join("externconfig")),
            buildscript: None,
            manifest_path: None,
            rustc: PathBuf::from("rustc"),
            sysroot: Some(PathBuf::from("/sysroot")),
            target: None,
            crate_name: "demo".to_string(),
            edition: "2021".to_string(),
            crate_type: "lib".to_string(),
            emit: "dep-info,link,metadata".to_string(),
            search_paths: vec![],
            codegen: vec!["metadata=demo-1.0.0".to_string()],
            cap_lints: Some("allow".to_string()),
            cc: None,
            deps: vec![],
            native: vec![],
            coverage: false,
            rustc_flags: vec![],
            pipeline_rmeta: false,
            features: vec!["std".to_string()],
            renames: vec![],
            static_crt: false,
            test: false,
            debug: false,
            optimize: true,
            sources: vec![PathBuf::from("src/lib.rs")],
        }
    }

    fn fixture(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("please_rust_cmd_test_{}_{}", std::process::id(), name));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("liba-1_0_0.rlib"), "").unwrap();
        std::fs::write(dir.join("libb-2_0_0.rlib"), "").unwrap();
        std::fs::write(dir.join("externconfig"), "a=liba-1_0_0.rlib\nb=libb-2_0_0.rlib\n").unwrap();
        dir
    }

    fn argv(cmd: &Command) -> Vec<String> {
        cmd.get_args().map(|a| a.to_string_lossy().to_string()).collect()
    }

    fn joined(cmd: &Command) -> String {
        argv(cmd).join(" ")
    }

    fn envs(cmd: &Command) -> std::collections::HashMap<String, String> {
        cmd.get_envs()
            .filter_map(|(k, v)| v.map(|v| (k.to_string_lossy().to_string(), v.to_string_lossy().to_string())))
            .collect()
    }

    #[test]
    fn basic_flags() {
        let _guard = CWD_LOCK.lock().unwrap();
        let dir = fixture("basic");
        let old = std::env::current_dir().unwrap();
        std::env::set_current_dir(&dir).unwrap();
        let cmd = build_command(&base_args(&dir)).unwrap();
        std::env::set_current_dir(old).unwrap();
        let s = joined(&cmd);
        assert!(s.contains("--sysroot /sysroot"));
        assert!(s.contains("--crate-name=demo"));
        assert!(s.contains("--edition=2021"));
        assert!(s.contains("--crate-type=lib"));
        assert!(s.contains("--cap-lints=allow"));
        assert!(s.contains("-C metadata=demo-1.0.0"));
        assert!(s.contains("--cfg feature=\"std\""));
        assert!(s.contains("-O"));
        assert!(!s.contains(" -g"));
        // All externconfig entries become externs when no --dep filter
        assert!(s.contains("--extern a="));
        assert!(s.contains("--extern b="));
        assert_eq!(envs(&cmd)["CARGO_CRATE_NAME"], "demo");
    }

    #[test]
    fn dep_filter_limits_externs() {
        let _guard = CWD_LOCK.lock().unwrap();
        let dir = fixture("depfilter");
        let mut args = base_args(&dir);
        args.deps = vec!["a".to_string()];
        let old = std::env::current_dir().unwrap();
        std::env::set_current_dir(&dir).unwrap();
        let cmd = build_command(&args).unwrap();
        std::env::set_current_dir(old).unwrap();
        let s = joined(&cmd);
        assert!(s.contains("--extern a="));
        assert!(!s.contains("--extern b="));
        // Transitive crates stay reachable through -L
        assert!(s.contains("-L"));
    }

    /// With two versions of one crate in the sandbox, a bare crate name
    /// cannot say which is wanted, and the loser silently wins by being last.
    #[test]
    fn dep_filter_picks_the_named_version() {
        let _guard = CWD_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("please_rust_two_versions_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("libsyn-2_0_119.rlib"), "").unwrap();
        std::fs::write(dir.join("libsyn-3_0_3.rlib"), "").unwrap();
        std::fs::write(
            dir.join("externconfig"),
            "syn@third_party/crates/syn-2.0.119=libsyn-2_0_119.rlib\n\
             syn@third_party/crates/syn=libsyn-3_0_3.rlib\n",
        )
        .unwrap();

        let mut args = base_args(&dir);
        args.deps = vec!["syn@third_party/crates/syn-2.0.119".to_string()];
        let old = std::env::current_dir().unwrap();
        std::env::set_current_dir(&dir).unwrap();
        let cmd = build_command(&args).unwrap();
        std::env::set_current_dir(old).unwrap();

        let s = joined(&cmd);
        assert!(s.contains("--extern syn=./libsyn-2_0_119.rlib"), "{}", s);
        assert!(!s.contains("libsyn-3_0_3.rlib"), "{}", s);
        // The version it did not ask for stays reachable through -L
        assert!(s.contains("-L"));
    }

    /// A crate that sets [lib] name is not called after its package: md-5
    /// builds md5, rustls-webpki builds webpki. A dependent renaming one of
    /// those knows which declaration it meant but not what the crate ended up
    /// called, so a rename may name the declaration alone.
    #[test]
    fn a_rename_may_name_only_the_declaration() {
        let _guard = CWD_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("please_rust_rename_decl_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("libmd5-0_11_0.rlib"), "").unwrap();
        std::fs::write(
            dir.join("externconfig"),
            "md5@third_party/crates/md_5=libmd5-0_11_0.rlib\n",
        )
        .unwrap();

        let mut args = base_args(&dir);
        args.renames = vec!["md5=@third_party/crates/md_5".to_string()];
        let old = std::env::current_dir().unwrap();
        std::env::set_current_dir(&dir).unwrap();
        let cmd = build_command(&args).unwrap();
        std::env::set_current_dir(old).unwrap();

        let s = joined(&cmd);
        assert!(s.contains("--extern md5=./libmd5-0_11_0.rlib"), "{}", s);
    }

    fn renames_add_aliased_externs() {
        let _guard = CWD_LOCK.lock().unwrap();
        let dir = fixture("renames");
        let mut args = base_args(&dir);
        args.renames = vec!["alias=a".to_string()];
        let old = std::env::current_dir().unwrap();
        std::env::set_current_dir(&dir).unwrap();
        let cmd = build_command(&args).unwrap();
        std::env::set_current_dir(old).unwrap();
        assert!(joined(&cmd).contains("--extern alias="));
    }

    #[test]
    fn test_harness_replaces_crate_type() {
        let dir = fixture("test_flag");
        let mut args = base_args(&dir);
        args.test = true;
        args.crate_type = "bin".to_string();
        let cmd = build_command(&args).unwrap();
        let s = joined(&cmd);
        assert!(s.contains("--test"));
        assert!(!s.contains("--crate-type"));
    }

    #[test]
    fn proc_macro_gets_compiler_extern() {
        let dir = fixture("pm");
        let mut args = base_args(&dir);
        args.crate_type = "proc-macro".to_string();
        let cmd = build_command(&args).unwrap();
        assert!(joined(&cmd).contains("--extern proc_macro"));
    }

    #[test]
    fn static_and_linker_flags() {
        let dir = fixture("static");
        let mut args = base_args(&dir);
        args.crate_type = "bin".to_string();
        args.static_crt = true;
        args.cc = Some(PathBuf::from("cc"));
        let cmd = build_command(&args).unwrap();
        let s = joined(&cmd);
        assert!(s.contains("-C target-feature=+crt-static"));
        assert!(s.contains("-C linker=cc"));
    }

    #[test]
    fn linker_not_applied_to_rlibs() {
        let dir = fixture("rlib_nolink");
        let mut args = base_args(&dir);
        args.cc = Some(PathBuf::from("cc"));
        let cmd = build_command(&args).unwrap();
        assert!(!joined(&cmd).contains("-C linker="));
    }

    #[test]
    fn buildscript_directives_apply() {
        let dir = fixture("bs");
        std::fs::create_dir_all(dir.join("out")).unwrap();
        std::fs::write(
            dir.join("demo.buildscript"),
            "out-dir=out\nrustc-cfg=has_std\nrustc-env=GEN=1\nrustc-link-lib=z\nrustc-link-search=native=/nat\nrustc-link-arg=-s\n",
        )
        .unwrap();
        let mut args = base_args(&dir);
        args.buildscript = Some(dir.join("demo.buildscript"));
        let cmd = build_command(&args).unwrap();
        let s = joined(&cmd);
        assert!(s.contains("--cfg has_std"));
        assert!(s.contains("-l z"));
        assert!(s.contains("-L /nat"));
        assert!(s.contains("-C link-arg=-s"));
        let e = envs(&cmd);
        assert_eq!(e["GEN"], "1");
        assert!(e["OUT_DIR"].ends_with("/out"));
    }

    #[test]
    fn manifest_sets_pkg_env() {
        let dir = fixture("manifest");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"3.1.4\"\n",
        )
        .unwrap();
        let mut args = base_args(&dir);
        args.manifest_path = Some(dir.join("Cargo.toml"));
        args.crate_type = "bin".to_string();
        let cmd = build_command(&args).unwrap();
        let e = envs(&cmd);
        assert_eq!(e["CARGO_PKG_VERSION"], "3.1.4");
        assert_eq!(e["CARGO_BIN_NAME"], "demo");
        assert!(e.contains_key("CARGO_MANIFEST_DIR"));
    }
}
