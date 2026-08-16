//! Generate BUILD files from Cargo.toml
//!
//! This module parses a crate's Cargo.toml and generates Please BUILD files
//! that can be used in a subrepo.

use anyhow::{Context, Result};
use cargo_toml::Manifest;
use clap::Args;
use std::fs;
use std::path::PathBuf;

#[derive(Args)]
pub struct GenerateArgs {
    /// Crate name
    #[arg(long)]
    pub crate_name: String,

    /// Crate version
    #[arg(long)]
    pub version: String,

    /// Source root directory containing Cargo.toml
    #[arg(long)]
    pub src_root: PathBuf,

    /// Subrepo path (e.g., "third_party/rust/serde")
    #[arg(long)]
    pub subrepo: String,

    /// Third-party folder path
    #[arg(long, default_value = "third_party/rust")]
    pub third_party_folder: String,

    /// Features to enable (comma-separated)
    #[arg(long, default_value = "")]
    pub features: String,

    /// Targets to install/export (comma-separated)
    #[arg(long, default_value = "")]
    pub install: String,

    /// Dependency overrides as package=subrepo_name, routing a dep to a
    /// versioned subrepo (e.g. hashbrown=hashbrown_0_12_3)
    #[arg(long = "override")]
    pub overrides: Vec<String>,

    /// Lock file produced by `please_rust resolve`; when it has an entry for
    /// this subrepo, features and dependency routing come from it and the
    /// heuristic per-manifest resolution below is skipped entirely.
    #[arg(long)]
    pub lock: Option<PathBuf>,

    /// Build label of the please_rust tool, as seen from inside the subrepo
    #[arg(long, default_value = "@//tools/please_rust:bootstrap")]
    pub tool_label: String,

    /// Build label of rustc, as seen from inside the subrepo
    #[arg(long, default_value = "@//third_party/rust:toolchain_rustc")]
    pub rustc_label: String,

    /// Build label of the sysroot, as seen from inside the subrepo
    #[arg(long, default_value = "@//third_party/rust:toolchain_sysroot")]
    pub sysroot_label: String,

    /// Build label of a C toolchain (cc_toolchain rule); empty disables
    #[arg(long, default_value = "")]
    pub cc_label: String,

    /// Emit pipelined-compilation rule shapes: each crate splits into a
    /// `_X#link` compile rule, a `_X#rmeta` metadata-only rule that
    /// dependents' compiles hang off, and a public `X` filegroup that
    /// propagates rlibs to binary links (the rules_rust two-action scheme)
    #[arg(long)]
    pub pipeline: bool,
}

pub fn run(args: GenerateArgs) -> Result<()> {
    let cargo_toml_path = args.src_root.join("Cargo.toml");

    // Parse Cargo.toml
    let manifest_bytes = fs::read(&cargo_toml_path)
        .with_context(|| format!("Failed to read {}", cargo_toml_path.display()))?;
    let manifest = crate::resolve::parse_manifest(&manifest_bytes)
        .with_context(|| format!("Failed to parse {}", cargo_toml_path.display()))?;

    let package = manifest
        .package
        .as_ref()
        .context("Cargo.toml missing [package] section")?;

    // Extract metadata
    let crate_name = &args.crate_name;
    let edition = package.edition.get().unwrap_or(&cargo_toml::Edition::E2021);

    // Check if build script is enabled and where it lives.
    // build = false means disabled; build = "path" overrides the default build.rs
    let build_script_path = match &package.build {
        Some(cargo_toml::OptionalFile::Flag(false)) => None,
        Some(cargo_toml::OptionalFile::Path(p)) => Some(p.to_string_lossy().to_string()),
        Some(cargo_toml::OptionalFile::Flag(true)) => Some("build.rs".to_string()),
        None => {
            // Cargo auto-detects build.rs in the package root
            if args.src_root.join("build.rs").exists() {
                Some("build.rs".to_string())
            } else {
                None
            }
        }
    };

    // Library root; [lib] path overrides the default src/lib.rs (e.g. fnv uses lib.rs)
    let lib_path = manifest
        .lib
        .as_ref()
        .and_then(|l| l.path.clone())
        .unwrap_or_else(|| "src/lib.rs".to_string());
    let has_lib = manifest.lib.is_some() || args.src_root.join(&lib_path).exists();

    // Binaries: explicit [[bin]] entries plus cargo's auto-discovered src/main.rs
    let mut bins: Vec<(String, String)> = Vec::new();
    for b in &manifest.bin {
        let bname = b.name.clone().unwrap_or_else(|| crate_name.clone());
        let bpath = b.path.clone().unwrap_or_else(|| "src/main.rs".to_string());
        if args.src_root.join(&bpath).exists() {
            bins.push((bname, bpath));
        }
    }
    if bins.is_empty() && args.src_root.join("src/main.rs").exists() {
        bins.push((crate_name.clone(), "src/main.rs".to_string()));
    }

    // Determine crate type
    let crate_type = determine_crate_type(&manifest);

    // Prefer the resolved lock entry for this subrepo when available
    let subrepo_key = args
        .subrepo
        .rsplit('/')
        .next()
        .unwrap_or(&args.subrepo)
        .to_string();
    let lock_file = match args.lock.as_ref() {
        Some(p) if p.exists() => match crate::resolve::LockFile::load(p) {
            Ok(lock) => Some(lock),
            Err(e) => {
                eprintln!("warning: {:#}", e);
                None
            }
        },
        _ => None,
    };
    let (lock_entry, host_entry) = match &lock_file {
        Some(lock) => {
            let entry = lock.crates.get(&subrepo_key).cloned();
            if entry.is_none() {
                eprintln!(
                    "warning: {} not in lock file, falling back to heuristic resolution",
                    subrepo_key
                );
            }
            (entry, lock.host_crates.get(&subrepo_key).cloned())
        }
        None => (None, None),
    };

    // Direct normal deps with links keys: their build-script outputs feed
    // this crate's build script as DEP_<LINKS>_<KEY> env vars
    let linked_deps: Vec<String> = match (&lock_file, &lock_entry) {
        (Some(lock), Some(entry)) => entry
            .deps
            .iter()
            .filter(|d| {
                lock.crates
                    .get(&d.subrepo)
                    .map(|e| e.links.is_some())
                    .unwrap_or(false)
            })
            .map(|d| {
                format!(
                    "///{}/{}//:_{}_build_script|buildscript",
                    args.third_party_folder,
                    d.subrepo,
                    d.crate_name.replace('-', "_")
                )
            })
            .collect(),
        _ => vec![],
    };

    let mk = |d: &crate::resolve::LockDep| {
        let target_name = if d.target_name.is_empty() {
            d.crate_name.replace('-', "_")
        } else {
            d.target_name.clone()
        };
        (
            d.name.clone(),
            format!("///{}/{}//:{}", args.third_party_folder, d.subrepo, target_name),
        )
    };

    let (requested_features, deps, build_deps) = if let Some(entry) = &lock_entry {
        (
            entry.features.clone(),
            entry.deps.iter().map(mk).collect::<Vec<_>>(),
            entry.build_deps.iter().map(mk).collect::<Vec<_>>(),
        )
    } else {
        // Heuristic path: requested features + name-based dep routing
        let requested_features: Vec<String> = if args.features.is_empty() {
            Vec::new()
        } else {
            args.features.split(',').map(|s| s.trim().to_string()).collect()
        };

        // Dependency overrides (package -> subrepo name)
        let overrides: std::collections::HashMap<String, String> = args
            .overrides
            .iter()
            .filter_map(|o| o.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())))
            .collect();

        let deps =
            resolve_dependencies(&manifest, &args.third_party_folder, &requested_features, &overrides);
        let build_deps = resolve_build_dependencies(&manifest, &args.third_party_folder);
        (requested_features, deps, build_deps)
    };

    // Generate BUILD file content
    let mut build_content = generate_build_file(
        crate_name,
        &args.version,
        edition,
        &crate_type,
        &requested_features,
        &deps,
        &build_deps,
        build_script_path.as_deref(),
        &lib_path,
        "",
        &linked_deps,
        args.pipeline,
    );

    // Binary targets (e.g. protoc plugins). Named <crate>_bin; they link the
    // crate's own lib (when present) plus the same resolved dependencies.
    if !bins.is_empty() {
        let crate_ident = crate_name.replace('-', "_");
        for (bin_name, bin_path) in &bins {
            build_content.push('\n');
            build_content.push_str(&generate_bin_rule(
                &crate_ident,
                bin_name,
                bin_path,
                &args.version,
                edition,
                &requested_features,
                &deps,
                has_lib,
                args.pipeline,
            ));
        }
    }

    // Host-unit variant for dual crates (proc-macro/build-script consumers
    // with a different unified feature set than the target unit)
    if let Some(host) = &host_entry {
        let host_deps: Vec<(String, String)> = host.deps.iter().map(mk).collect();
        let host_build_deps: Vec<(String, String)> = host.build_deps.iter().map(mk).collect();
        build_content.push('\n');
        build_content.push_str(&generate_build_file(
            crate_name,
            &args.version,
            edition,
            &crate_type,
            &host.features,
            &host_deps,
            &host_build_deps,
            build_script_path.as_deref(),
            &lib_path,
            "_host",
            &linked_deps,
            args.pipeline,
        ));
    }

    // Tool labels are configurable (CONFIG.RUST.*); the emitters use the
    // defaults, substituted here so every rule points at the configured ones.
    let mut build_content = build_content
        .replace("@//tools/please_rust:bootstrap", &args.tool_label)
        .replace("@//third_party/rust:toolchain_rustc", &args.rustc_label)
        .replace("@//third_party/rust:toolchain_sysroot", &args.sysroot_label);
    if args.cc_label.is_empty() {
        build_content = build_content
            .replace("--cc $TOOLS_CC ", "")
            .replace("        \"cc\": [\"__CC_LABEL__\"],\n", "");
    } else {
        build_content = build_content.replace("__CC_LABEL__", &args.cc_label);
    }

    // Write BUILD file
    let build_path = args.src_root.join("BUILD");
    fs::write(&build_path, &build_content)
        .with_context(|| format!("Failed to write {}", build_path.display()))?;

    // Write .plzconfig
    let plzconfig_content = generate_plzconfig();
    let plzconfig_path = args.src_root.join(".plzconfig");
    fs::write(&plzconfig_path, &plzconfig_content)
        .with_context(|| format!("Failed to write {}", plzconfig_path.display()))?;

    // If install targets specified, add a filegroup
    if !args.install.is_empty() {
        // Already included in the BUILD file
    }

    eprintln!(
        "Generated BUILD file for {} v{} at {}",
        crate_name,
        args.version,
        build_path.display()
    );

    Ok(())
}

fn determine_crate_type(manifest: &Manifest) -> String {
    // Check if it's a proc-macro
    if let Some(lib) = &manifest.lib {
        if lib.proc_macro {
            return "proc-macro".to_string();
        }
        // Check crate-type if specified
        if lib.crate_type.contains(&"proc-macro".to_string()) {
            return "proc-macro".to_string();
        }
        // If lib section exists, it's a library
        return "lib".to_string();
    }

    // Check if it's a binary-only crate (no lib)
    if !manifest.bin.is_empty() {
        return "bin".to_string();
    }

    // Default to library
    "lib".to_string()
}

fn resolve_dependencies(
    manifest: &Manifest,
    third_party_folder: &str,
    enabled_features: &[String],
    overrides: &std::collections::HashMap<String, String>,
) -> Vec<(String, String)> {
    let mut deps = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Helper to add a dependency
    let mut add_dep = |name: &str, dep: &cargo_toml::Dependency| {
        // Renamed deps (package = "...") point at the real crate; the
        // rustc-std-workspace-* shims only exist for building std itself.
        let package_name = dep.package().unwrap_or(name);
        if package_name.starts_with("rustc-std-workspace") {
            return;
        }

        // Check if this is an optional dependency
        let is_optional = dep.optional();

        // Optional deps are only included if:
        // 1. The feature with the dep's name is enabled, OR
        // 2. A feature that includes this dep is enabled
        if is_optional {
            let dep_feature_name = name.replace("-", "_");
            let is_enabled = enabled_features.contains(&dep_feature_name)
                || enabled_features.contains(&name.to_string());

            if !is_enabled {
                // Check if any enabled feature activates this dependency
                let mut activated = false;
                for enabled in enabled_features {
                    if let Some(feature_deps) = manifest.features.get(enabled) {
                        // Features can reference deps as "dep:name" or just "name"
                        if feature_deps.contains(&format!("dep:{}", name))
                            || feature_deps.contains(&name.to_string())
                        {
                            activated = true;
                            break;
                        }
                    }
                }
                if !activated {
                    return;
                }
            }
        }

        // Convert crate name to subrepo target
        // e.g., "serde-derive" -> "///third_party/rust/serde_derive//:serde_derive"
        // Overrides route to a versioned subrepo; the target inside is still
        // named after the crate.
        let normalized_name = package_name.replace("-", "_");
        let subrepo_name = overrides
            .get(package_name)
            .cloned()
            .unwrap_or_else(|| normalized_name.clone());
        if seen.insert(normalized_name.clone()) {
            let target = format!(
                "///{}/{}//:{}",
                third_party_folder,
                subrepo_name,
                normalized_name
            );
            deps.push((name.to_string(), target));
        }
    };

    // Process regular dependencies
    for (name, dep) in &manifest.dependencies {
        add_dep(name, dep);
    }

    // Process target-specific dependencies
    // For now, we only include deps that apply to Linux
    for (target_cfg, target_deps) in &manifest.target {
        // Check if this target applies to our platform (Linux/x86_64)
        // Common patterns:
        // - cfg(unix), cfg(target_os = "linux"), cfg(target_family = "unix")
        // - cfg(not(windows)) - applies on Linux
        let applies = target_cfg.contains("unix")
            || target_cfg.contains("linux")
            || target_cfg.contains("target_family = \"unix\"")
            || (target_cfg.contains("not") && target_cfg.contains("windows"))
            || (target_cfg.contains("not") && target_cfg.contains("wasm"));

        if applies {
            for (name, dep) in &target_deps.dependencies {
                add_dep(name, dep);
            }
        }
    }

    deps
}

/// Resolve build-dependencies (used for compiling build.rs)
fn resolve_build_dependencies(
    manifest: &Manifest,
    third_party_folder: &str,
) -> Vec<(String, String)> {
    let mut deps = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Process build-dependencies
    for (name, _dep) in &manifest.build_dependencies {
        let normalized_name = name.replace("-", "_");
        if seen.insert(normalized_name.clone()) {
            let target = format!(
                "///{}/{}//:{}",
                third_party_folder,
                normalized_name,
                normalized_name
            );
            deps.push((name.clone(), target));
        }
    }

    // Also check target-specific build-dependencies
    for (target_cfg, target_deps) in &manifest.target {
        let applies = target_cfg.contains("unix")
            || target_cfg.contains("linux")
            || target_cfg.contains("target_family = \"unix\"")
            || (target_cfg.contains("not") && target_cfg.contains("windows"))
            || (target_cfg.contains("not") && target_cfg.contains("wasm"));

        if applies {
            for (name, _dep) in &target_deps.build_dependencies {
                let normalized_name = name.replace("-", "_");
                if seen.insert(normalized_name.clone()) {
                    let target = format!(
                        "///{}/{}//:{}",
                        third_party_folder,
                        normalized_name,
                        normalized_name
                    );
                    deps.push((name.clone(), target));
                }
            }
        }
    }

    deps
}

/// Rewrites a dep target label to its `_name#rmeta` twin.
fn rmeta_ref(target: &str) -> String {
    match target.rfind(':') {
        Some(i) => format!("{}:_{}#rmeta", &target[..i], &target[i + 1..]),
        None => format!("_{}#rmeta", target),
    }
}

/// Rewrites a dep target label to its `_name#link` compile rule.
fn link_ref(target: &str) -> String {
    match target.rfind(':') {
        Some(i) => format!("{}:_{}#link", &target[..i], &target[i + 1..]),
        None => format!("_{}#link", target),
    }
}

fn generate_build_file(
    crate_name: &str,
    version: &str,
    edition: &cargo_toml::Edition,
    crate_type: &str,
    features: &[String],
    deps: &[(String, String)],
    build_deps: &[(String, String)],
    build_script_path: Option<&str>,
    lib_path: &str,
    suffix: &str,
    linked_deps: &[String],
    pipeline: bool,
) -> String {
    let mut content = String::new();

    let crate_ident = crate_name.replace("-", "_");
    let normalized_name = format!("{}{}", crate_ident, suffix);
    let edition_str = match edition {
        cargo_toml::Edition::E2015 => "2015",
        cargo_toml::Edition::E2018 => "2018",
        cargo_toml::Edition::E2021 => "2021",
        _ => "2024",
    };

    // Multiple versions of a crate can coexist in one dependent's inputs, so
    // library filenames and metadata are disambiguated per version, the same
    // way cargo uses -C extra-filename/-C metadata.
    let version_tag = format!("{}{}", version.replace(['.', '+'], "_"), suffix);

    // Determine outputs based on crate type
    let (out_rlib, out_rmeta, emit) = match crate_type {
        "proc-macro" => (
            format!("lib{}-{}.so", crate_ident, version_tag),
            format!("lib{}-{}.so", crate_ident, version_tag),
            "dep-info,link",
        ),
        _ => (
            format!("lib{}-{}.rlib", crate_ident, version_tag),
            format!("lib{}-{}.rmeta", crate_ident, version_tag),
            "dep-info,link,metadata",
        ),
    };

    // Build feature flags for please_rust compile
    let feature_args: Vec<String> = features
        .iter()
        .map(|f| format!("--feature {}", f))
        .collect();
    let mut feature_str = feature_args.join(" ");

    // Renamed deps (declared name differs from the real package): tell
    // compile to add an --extern under the declared name too.
    for (name, target) in deps {
        let dep_norm = name.replace('-', "_");
        if let Some(pkg) = target.rsplit(':').next() {
            if dep_norm != pkg {
                feature_str.push_str(&format!(" --rename {}={}", dep_norm, pkg));
            }
        }
    }

    // Version-disambiguated symbols and output filenames (see version_tag above)
    feature_str.push_str(&format!(
        " -C metadata={}-{} -C extra-filename=-{}",
        normalized_name, version, version_tag
    ));

    // If this crate has a build script, generate two-stage build
    if let Some(script_path) = build_script_path {
        content.push_str(&generate_build_script_rule(&normalized_name, features, build_deps, script_path, linked_deps, pipeline));
        content.push_str("\n");
        content.push_str(&generate_compile_rule_with_buildscript(
            &normalized_name,
            &crate_ident,
            edition_str,
            crate_type,
            &out_rlib,
            &out_rmeta,
            emit,
            &feature_str,
            deps,
            lib_path,
            pipeline,
        ));
    } else {
        content.push_str(&generate_compile_rule(
            &normalized_name,
            &crate_ident,
            edition_str,
            crate_type,
            &out_rlib,
            &out_rmeta,
            emit,
            &feature_str,
            deps,
            lib_path,
            pipeline,
        ));
    }

    if pipeline {
        // Public filegroup: what dependents and the rust_repo alias point
        // at. It propagates transitive rlibs to binary links via its deps.
        content.push('\n');
        content.push_str("filegroup(\n");
        content.push_str(&format!("    name = \"{}\",\n", normalized_name));
        content.push_str(&format!("    srcs = [\":_{}#link\"],\n", normalized_name));
        if !deps.is_empty() {
            content.push_str("    deps = [\n");
            for (_name, target) in deps {
                content.push_str(&format!("        \"{}\",\n", target));
            }
            content.push_str("    ],\n");
        }
        content.push_str("    visibility = [\"PUBLIC\"],\n");
        content.push_str(")\n");

        content.push('\n');
        if crate_type == "proc-macro" {
            // Proc-macros must fully build before dependents can expand
            // them; the twin just re-exports the externconfig under the
            // uniform name, with the dylib staged via the public dep.
            content.push_str("build_rule(\n");
            content.push_str(&format!("    name = \"_{}#rmeta\",\n", normalized_name));
            content.push_str(&format!("    srcs = [\":_{}#link|externconfig\"],\n", normalized_name));
            content.push_str("    cmd = \"cp $SRCS $OUTS_EXTERNCONFIG\",\n");
            content.push_str("    outs = {\n");
            content.push_str(&format!("        \"externconfig\": [\"{}.rmeta.externconfig\"],\n", normalized_name));
            content.push_str("    },\n");
            content.push_str(&format!("    deps = [\":{}\"],\n", normalized_name));
            content.push_str("    visibility = [\"PUBLIC\"],\n");
            content.push_str(")\n");
        } else {
            content.push_str(&generate_rmeta_rule(
                &normalized_name,
                &crate_ident,
                edition_str,
                &out_rmeta,
                &feature_str,
                deps,
                lib_path,
                build_script_path.is_some(),
            ));
        }
    }

    content
}

/// Generate the metadata-only compile rule (`_X#rmeta`) that dependents'
/// compiles hang off under pipelined compilation. Frontend-only: no codegen,
/// so a chain of crates builds at frontend depth.
fn generate_rmeta_rule(
    normalized_name: &str,
    crate_ident: &str,
    edition_str: &str,
    out_rmeta: &str,
    feature_str: &str,
    deps: &[(String, String)],
    lib_path: &str,
    has_buildscript: bool,
) -> String {
    let mut content = String::new();

    let aggregate_cmd = if deps.is_empty() {
        "true".to_string()
    } else {
        "cat $SRCS_EXTERNCONFIGS > externconfig".to_string()
    };
    let buildscript_arg = if has_buildscript {
        "--buildscript $SRCS_BUILDSCRIPT "
    } else {
        ""
    };
    // The full compile command with --pipeline-rmeta: rustc is terminated as
    // soon as the rmeta artifact lands (a plain --emit=metadata rmeta lacks
    // the optimized MIR dependents' codegen needs). Profile flags must match
    // the link rule so the inlined MIR agrees.
    let compile_base = format!(
        "{} && $TOOLS_PLEASE_RUST compile --pipeline-rmeta --externconfig externconfig {}--manifest-path $SRCS_MANIFEST --rustc $TOOLS_RUSTC --sysroot $TOOLS_SYSROOT --cap-lints allow --crate-name {} --edition {} --crate-type lib --emit dep-info,link,metadata {}",
        aggregate_cmd, buildscript_arg, crate_ident, edition_str, feature_str
    );
    let ec_cmd = format!("echo '{}={}' > $OUTS_EXTERNCONFIG", normalized_name, out_rmeta);
    let cmd_dbg = format!("{} -g $SRCS_MAIN && {}", compile_base, ec_cmd);
    let cmd_opt = format!("{} -O $SRCS_MAIN && {}", compile_base, ec_cmd);

    content.push_str("build_rule(\n");
    content.push_str(&format!("    name = \"_{}#rmeta\",\n", normalized_name));
    content.push_str("    srcs = {\n");
    content.push_str(&format!("        \"main\": [\"{}\"],\n", lib_path));
    content.push_str(&format!("        \"mods\": glob([\"src/**\", \"*.rs\"], exclude=[\"{}\", \"src/lib.rs\", \"src/main.rs\", \"build.rs\"], allow_empty=True),\n", lib_path));
    content.push_str("        \"data\": glob([\"*.md\", \"LICENSE*\", \"examples/**/*\"], allow_empty=True),\n");
    content.push_str("        \"manifest\": [\"Cargo.toml\"],\n");
    if !deps.is_empty() {
        content.push_str("        \"externconfigs\": [\n");
        for (_name, target) in deps {
            content.push_str(&format!("            \"{}|externconfig\",\n", rmeta_ref(target)));
        }
        content.push_str("        ],\n");
    }
    if has_buildscript {
        content.push_str(&format!("        \"buildscript\": [\":_{}_build_script|buildscript\"],\n", normalized_name));
        content.push_str(&format!("        \"buildscript_out\": [\":_{}_build_script|out\"],\n", normalized_name));
    }
    content.push_str("    },\n");
    content.push_str("    cmd = {\n");
    content.push_str(&format!("        \"dbg\": \"{}\",\n", cmd_dbg));
    content.push_str(&format!("        \"opt\": \"{}\",\n", cmd_opt));
    content.push_str(&format!("        \"cover\": \"{}\",\n", cmd_dbg));
    content.push_str("    },\n");
    content.push_str("    outs = {\n");
    content.push_str(&format!("        \"rmeta\": [\"{}\"],\n", out_rmeta));
    content.push_str(&format!("        \"externconfig\": [\"{}.rmeta.externconfig\"],\n", normalized_name));
    content.push_str("    },\n");
    if !deps.is_empty() {
        content.push_str("    deps = [\n");
        for (_name, target) in deps {
            content.push_str(&format!("        \"{}\",\n", rmeta_ref(target)));
        }
        content.push_str("    ],\n");
    }
    content.push_str("    tools = {\n");
    content.push_str("        \"please_rust\": [\"@//tools/please_rust:bootstrap\"],\n");
    content.push_str("        \"rustc\": [\"@//third_party/rust:toolchain_rustc\"],\n");
    content.push_str("        \"sysroot\": [\"@//third_party/rust:toolchain_sysroot\"],\n");
    content.push_str("    },\n");
    content.push_str("    needs_transitive_deps = True,\n");
    content.push_str("    visibility = [\"PUBLIC\"],\n");
    content.push_str(")\n");

    content
}

/// Generate a binary target for a crate's [[bin]] (or src/main.rs).
fn generate_bin_rule(
    crate_ident: &str,
    bin_name: &str,
    bin_path: &str,
    version: &str,
    edition: &cargo_toml::Edition,
    features: &[String],
    deps: &[(String, String)],
    has_lib: bool,
    pipeline: bool,
) -> String {
    let edition_str = match edition {
        cargo_toml::Edition::E2015 => "2015",
        cargo_toml::Edition::E2018 => "2018",
        cargo_toml::Edition::E2021 => "2021",
        _ => "2024",
    };
    let bin_ident = bin_name.replace('-', "_");
    let rule_name = format!("{}_bin", bin_ident);
    let feature_str: String = features
        .iter()
        .map(|f| format!("--feature {} ", f))
        .collect();
    let version_tag = version.replace(['.', '+'], "_");

    let mut content = String::new();
    content.push_str(&format!("# Binary target for {}\n", bin_name));
    content.push_str("build_rule(\n");
    content.push_str(&format!("    name = \"{}\",\n", rule_name));
    content.push_str("    srcs = {\n");
    content.push_str(&format!("        \"main\": [\"{}\"],\n", bin_path));
    content.push_str(&format!("        \"mods\": glob([\"src/**\"], exclude=[\"{}\", \"src/lib.rs\", \"build.rs\"], allow_empty=True),\n", bin_path));
    content.push_str("        \"manifest\": [\"Cargo.toml\"],\n");
    content.push_str("        \"externconfigs\": [\n");
    if has_lib {
        let lib_ec = if pipeline {
            format!(":_{}#link", crate_ident)
        } else {
            format!(":{}", crate_ident)
        };
        content.push_str(&format!("            \"{}|externconfig\",\n", lib_ec));
    }
    for (_name, target) in deps {
        let ec = if pipeline { link_ref(target) } else { target.clone() };
        content.push_str(&format!("            \"{}|externconfig\",\n", ec));
    }
    content.push_str("        ],\n");
    content.push_str("    },\n");
    let compile = format!(
        "cat $SRCS_EXTERNCONFIGS > externconfig && $TOOLS_PLEASE_RUST compile --externconfig externconfig --manifest-path $SRCS_MANIFEST --rustc $TOOLS_RUSTC --sysroot $TOOLS_SYSROOT --cc $TOOLS_CC --cap-lints allow --crate-name {} --edition {} --crate-type bin --emit dep-info,link {}-C metadata={}-bin-{} -O $SRCS_MAIN",
        bin_ident, edition_str, feature_str, bin_ident, version_tag
    );
    content.push_str(&format!("    cmd = \"{}\",\n", compile));
    content.push_str(&format!("    outs = [\"{}\"],\n", bin_ident));
    content.push_str("    binary = True,\n");
    if has_lib || !deps.is_empty() {
        content.push_str("    deps = [\n");
        if has_lib {
            content.push_str(&format!("        \":{}\",\n", crate_ident));
        }
        for (_name, target) in deps {
            content.push_str(&format!("        \"{}\",\n", target));
        }
        content.push_str("    ],\n");
    }
    content.push_str("    tools = {\n");
    content.push_str("        \"please_rust\": [\"@//tools/please_rust:bootstrap\"],\n");
    content.push_str("        \"rustc\": [\"@//third_party/rust:toolchain_rustc\"],\n");
    content.push_str("        \"sysroot\": [\"@//third_party/rust:toolchain_sysroot\"],\n");
    content.push_str("        \"cc\": [\"__CC_LABEL__\"],\n");
    content.push_str("    },\n");
    content.push_str("    needs_transitive_deps = True,\n");
    content.push_str("    visibility = [\"PUBLIC\"],\n");
    content.push_str(")\n");
    content
}

/// Generate a build_rule for the build script (Stage 1)
fn generate_build_script_rule(
    normalized_name: &str,
    features: &[String],
    build_deps: &[(String, String)],
    script_path: &str,
    linked_deps: &[String],
    pipeline: bool,
) -> String {
    let mut content = String::new();

    // Build feature args for build-script command
    let feature_args: Vec<String> = features
        .iter()
        .map(|f| format!("--feature {}", f))
        .collect();
    let feature_str = feature_args.join(" ");

    // Aggregate direct build-dependencies' externconfigs only (transitive
    // ones can contain colliding entries for other versions of a crate)
    let has_build_deps = !build_deps.is_empty();
    let aggregate_cmd = if has_build_deps {
        "cat $SRCS_EXTERNCONFIGS > externconfig && "
    } else {
        ""
    };
    let externconfig_arg = if has_build_deps {
        "--externconfig externconfig "
    } else {
        ""
    };

    // Build script command. OUT_DIR is a declared output directory of this
    // rule so the files a build script generates survive into the crate's
    // compile action (the directives file records it by name; compile
    // resolves it as a sibling of the directives file).
    let dep_metadata_arg = if linked_deps.is_empty() {
        ""
    } else {
        "--dep-metadata $SRCS_DEP_METADATA "
    };
    let build_script_cmd = format!(
        "mkdir -p out && {}$TOOLS_PLEASE_RUST build-script --manifest-path $SRCS_MANIFEST --build-script $SRCS_SCRIPT --out-dir out --rustc $TOOLS_RUSTC --sysroot $TOOLS_SYSROOT --cc $TOOLS_CC {}{}--output $OUTS_BUILDSCRIPT {}",
        aggregate_cmd, externconfig_arg, dep_metadata_arg, feature_str
    );

    content.push_str(&format!("# Stage 1: Run build script for {}\n", normalized_name));
    content.push_str("build_rule(\n");
    content.push_str(&format!("    name = \"_{}_build_script\",\n", normalized_name));
    content.push_str("    srcs = {\n");
    content.push_str(&format!("        \"script\": [\"{}\"],\n", script_path));
    // Cargo runs build scripts from the package root with the whole package
    // present (scripts read source/data files, e.g. blake3 reads c/).
    content.push_str(&format!("        \"package\": glob([\"**/*\"], exclude=[\"{}\", \"Cargo.toml\", \"BUILD\", \".plzconfig\", \"out\"], allow_empty=True),\n", script_path));
    content.push_str("        \"manifest\": [\"Cargo.toml\"],\n");
    if has_build_deps {
        content.push_str("        \"externconfigs\": [\n");
        for (_name, target) in build_deps {
            let ec = if pipeline { link_ref(target) } else { target.clone() };
            content.push_str(&format!("            \"{}|externconfig\",\n", ec));
        }
        content.push_str("        ],\n");
    }
    if !linked_deps.is_empty() {
        content.push_str("        \"dep_metadata\": [\n");
        for label in linked_deps {
            content.push_str(&format!("            \"{}\",\n", label));
        }
        content.push_str("        ],\n");
    }
    content.push_str("    },\n");
    content.push_str(&format!("    cmd = \"{}\",\n", build_script_cmd));
    content.push_str("    outs = {\n");
    content.push_str(&format!("        \"buildscript\": [\"{}.buildscript\"],\n", normalized_name));
    content.push_str("        \"out\": [\"out\"],\n");
    content.push_str("    },\n");

    // Add build-dependencies if any
    if has_build_deps {
        content.push_str("    deps = [\n");
        for (_name, target) in build_deps {
            content.push_str(&format!("        \"{}\",\n", target));
        }
        content.push_str("    ],\n");
    }

    content.push_str("    tools = {\n");
    content.push_str("        \"please_rust\": [\"@//tools/please_rust:bootstrap\"],\n");
    content.push_str("        \"rustc\": [\"@//third_party/rust:toolchain_rustc\"],\n");
    content.push_str("        \"sysroot\": [\"@//third_party/rust:toolchain_sysroot\"],\n");
    content.push_str("        \"cc\": [\"__CC_LABEL__\"],\n");
    content.push_str("    },\n");

    // Need transitive deps if we have build-deps to get their externconfigs
    if has_build_deps {
        content.push_str("    needs_transitive_deps = True,\n");
    }

    content.push_str(")\n");

    content
}

/// Generate the main compile rule with buildscript support (Stage 2)
/// Pipelined-shape naming for a crate's compile rule: the rule name plus how
/// dependents' labels and externconfig refs are written. rlib-ish crates hang
/// off deps' metadata twins; proc-macros link, so they need full dep builds.
fn pipeline_shape(
    normalized_name: &str,
    crate_type: &str,
    pipeline: bool,
) -> (String, fn(&str) -> String, fn(&str) -> String) {
    if !pipeline {
        return (
            normalized_name.to_string(),
            |t| t.to_string(),
            |t| format!("{}|externconfig", t),
        );
    }
    let rule_name = format!("_{}#link", normalized_name);
    if crate_type == "proc-macro" {
        (
            rule_name,
            |t| t.to_string(),
            |t| format!("{}|externconfig", link_ref(t)),
        )
    } else {
        (
            rule_name,
            |t| rmeta_ref(t),
            |t| format!("{}|externconfig", rmeta_ref(t)),
        )
    }
}

fn generate_compile_rule_with_buildscript(
    normalized_name: &str,
    crate_ident: &str,
    edition_str: &str,
    crate_type: &str,
    out_rlib: &str,
    out_rmeta: &str,
    emit: &str,
    feature_str: &str,
    deps: &[(String, String)],
    lib_path: &str,
    pipeline: bool,
) -> String {
    let mut content = String::new();
    let (rule_name, dep_label, dep_ec): (String, fn(&str) -> String, fn(&str) -> String) =
        pipeline_shape(normalized_name, crate_type, pipeline);

    // Direct deps' externconfigs only: transitive configs can contain
    // colliding entries for other versions of the same crate.
    let aggregate_cmd = if deps.is_empty() {
        "true".to_string()
    } else {
        "cat $SRCS_EXTERNCONFIGS > externconfig".to_string()
    };

    // Compile command with --buildscript flag
    let compile_base = format!(
        "$TOOLS_PLEASE_RUST compile --externconfig externconfig --buildscript $SRCS_BUILDSCRIPT --manifest-path $SRCS_MANIFEST --rustc $TOOLS_RUSTC --sysroot $TOOLS_SYSROOT --cc $TOOLS_CC --cap-lints allow --crate-name {} --edition {} --crate-type {} --emit {} {}",
        crate_ident, edition_str, crate_type, emit, feature_str
    );

    let cmd_dbg = format!(
        "{} && {} -g $SRCS_MAIN && echo '{}={}' > $OUTS_EXTERNCONFIG",
        aggregate_cmd, compile_base, normalized_name, out_rlib
    );
    let cmd_opt = format!(
        "{} && {} -O $SRCS_MAIN && echo '{}={}' > $OUTS_EXTERNCONFIG",
        aggregate_cmd, compile_base, normalized_name, out_rlib
    );

    content.push_str(&format!("# Stage 2: Compile {} with build script output\n", normalized_name));
    content.push_str("build_rule(\n");
    content.push_str(&format!("    name = \"{}\",\n", rule_name));
    content.push_str("    srcs = {\n");
    content.push_str(&format!("        \"main\": [\"{}\"],\n", lib_path));
    content.push_str(&format!("        \"mods\": glob([\"src/**\", \"*.rs\"], exclude=[\"{}\", \"src/lib.rs\", \"src/main.rs\", \"build.rs\"], allow_empty=True),\n", lib_path));
    content.push_str("        \"data\": glob([\"*.md\", \"LICENSE*\", \"examples/**/*\"], allow_empty=True),\n");
    content.push_str("        \"manifest\": [\"Cargo.toml\"],\n");
    if !deps.is_empty() {
        content.push_str("        \"externconfigs\": [\n");
        for (_name, target) in deps {
            content.push_str(&format!("            \"{}\",\n", dep_ec(target)));
        }
        content.push_str("        ],\n");
    }
    content.push_str(&format!("        \"buildscript\": [\":_{}_build_script|buildscript\"],\n", normalized_name));
    content.push_str(&format!("        \"buildscript_out\": [\":_{}_build_script|out\"],\n", normalized_name));
    content.push_str("    },\n");
    content.push_str("    cmd = {\n");
    content.push_str(&format!("        \"dbg\": \"{}\",\n", cmd_dbg));
    content.push_str(&format!("        \"opt\": \"{}\",\n", cmd_opt));
    content.push_str(&format!("        \"cover\": \"{}\",\n", cmd_dbg));
    content.push_str("    },\n");
    content.push_str("    outs = {\n");
    content.push_str(&format!("        \"rlib\": [\"{}\"],\n", out_rlib));
    if crate_type != "proc-macro" && !pipeline {
        content.push_str(&format!("        \"rmeta\": [\"{}\"],\n", out_rmeta));
    }
    content.push_str(&format!("        \"externconfig\": [\"{}.externconfig\"],\n", normalized_name));
    content.push_str("    },\n");

    if !deps.is_empty() {
        content.push_str("    deps = [\n");
        for (_name, target) in deps {
            content.push_str(&format!("        \"{}\",\n", dep_label(target)));
        }
        content.push_str("    ],\n");
    }

    content.push_str("    tools = {\n");
    content.push_str("        \"please_rust\": [\"@//tools/please_rust:bootstrap\"],\n");
    content.push_str("        \"rustc\": [\"@//third_party/rust:toolchain_rustc\"],\n");
    content.push_str("        \"sysroot\": [\"@//third_party/rust:toolchain_sysroot\"],\n");
    content.push_str("        \"cc\": [\"__CC_LABEL__\"],\n");
    content.push_str("    },\n");
    content.push_str("    needs_transitive_deps = True,\n");
    content.push_str("    visibility = [\"PUBLIC\"],\n");
    content.push_str(")\n");

    content
}

/// Generate the main compile rule (no build script)
fn generate_compile_rule(
    normalized_name: &str,
    crate_ident: &str,
    edition_str: &str,
    crate_type: &str,
    out_rlib: &str,
    out_rmeta: &str,
    emit: &str,
    feature_str: &str,
    deps: &[(String, String)],
    lib_path: &str,
    pipeline: bool,
) -> String {
    let mut content = String::new();
    let (rule_name, dep_label, dep_ec): (String, fn(&str) -> String, fn(&str) -> String) =
        pipeline_shape(normalized_name, crate_type, pipeline);

    // Direct deps' externconfigs only: transitive configs can contain
    // colliding entries for other versions of the same crate.
    let aggregate_cmd = if deps.is_empty() {
        "true".to_string()
    } else {
        "cat $SRCS_EXTERNCONFIGS > externconfig".to_string()
    };

    let compile_base = format!(
        "$TOOLS_PLEASE_RUST compile --externconfig externconfig --manifest-path $SRCS_MANIFEST --rustc $TOOLS_RUSTC --sysroot $TOOLS_SYSROOT --cc $TOOLS_CC --cap-lints allow --crate-name {} --edition {} --crate-type {} --emit {} {}",
        crate_ident, edition_str, crate_type, emit, feature_str
    );

    let cmd_dbg = format!(
        "{} && {} -g $SRCS_MAIN && echo '{}={}' > $OUTS_EXTERNCONFIG",
        aggregate_cmd, compile_base, normalized_name, out_rlib
    );
    let cmd_opt = format!(
        "{} && {} -O $SRCS_MAIN && echo '{}={}' > $OUTS_EXTERNCONFIG",
        aggregate_cmd, compile_base, normalized_name, out_rlib
    );

    content.push_str("build_rule(\n");
    content.push_str(&format!("    name = \"{}\",\n", rule_name));
    content.push_str("    srcs = {\n");
    content.push_str(&format!("        \"main\": [\"{}\"],\n", lib_path));
    content.push_str(&format!("        \"mods\": glob([\"src/**\", \"*.rs\"], exclude=[\"{}\", \"src/lib.rs\", \"src/main.rs\", \"build.rs\"], allow_empty=True),\n", lib_path));
    content.push_str("        \"data\": glob([\"*.md\", \"LICENSE*\", \"examples/**/*\"], allow_empty=True),\n");
    content.push_str("        \"manifest\": [\"Cargo.toml\"],\n");
    if !deps.is_empty() {
        content.push_str("        \"externconfigs\": [\n");
        for (_name, target) in deps {
            content.push_str(&format!("            \"{}\",\n", dep_ec(target)));
        }
        content.push_str("        ],\n");
    }
    content.push_str("    },\n");
    content.push_str("    cmd = {\n");
    content.push_str(&format!("        \"dbg\": \"{}\",\n", cmd_dbg));
    content.push_str(&format!("        \"opt\": \"{}\",\n", cmd_opt));
    content.push_str(&format!("        \"cover\": \"{}\",\n", cmd_dbg));
    content.push_str("    },\n");
    content.push_str("    outs = {\n");
    content.push_str(&format!("        \"rlib\": [\"{}\"],\n", out_rlib));
    if crate_type != "proc-macro" && !pipeline {
        content.push_str(&format!("        \"rmeta\": [\"{}\"],\n", out_rmeta));
    }
    content.push_str(&format!("        \"externconfig\": [\"{}.externconfig\"],\n", normalized_name));
    content.push_str("    },\n");

    if !deps.is_empty() {
        content.push_str("    deps = [\n");
        for (_name, target) in deps {
            content.push_str(&format!("        \"{}\",\n", dep_label(target)));
        }
        content.push_str("    ],\n");
    }

    content.push_str("    tools = {\n");
    content.push_str("        \"please_rust\": [\"@//tools/please_rust:bootstrap\"],\n");
    content.push_str("        \"rustc\": [\"@//third_party/rust:toolchain_rustc\"],\n");
    content.push_str("        \"sysroot\": [\"@//third_party/rust:toolchain_sysroot\"],\n");
    content.push_str("        \"cc\": [\"__CC_LABEL__\"],\n");
    content.push_str("    },\n");
    content.push_str("    needs_transitive_deps = True,\n");
    content.push_str("    visibility = [\"PUBLIC\"],\n");
    content.push_str(")\n");

    content
}

fn generate_plzconfig() -> String {
    // Reference parent repo's plugin and provide explicit toolchain paths
    // CONFIG.RUST is not available in subrepos, so we specify the tools directly
    r#"[Plugin "rust"]
Target = @//plugins:rust
Rustc = @//third_party/rust:toolchain_rustc
Stdlib = @//third_party/rust:toolchain_stdlib
"#.to_string()
}


#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(body: &str) -> Manifest {
        crate::resolve::parse_manifest(
            format!("[package]\nname = \"t\"\nversion = \"1.0.0\"\n{}", body).as_bytes(),
        )
        .unwrap()
    }

    #[test]
    fn crate_type_detection() {
        assert_eq!(determine_crate_type(&manifest("")), "lib");
        assert_eq!(determine_crate_type(&manifest("[lib]\nproc-macro = true\n")), "proc-macro");
        assert_eq!(determine_crate_type(&manifest("[lib]\ncrate-type = [\"proc-macro\"]\n")), "proc-macro");
        assert_eq!(determine_crate_type(&manifest("[[bin]]\nname = \"tool\"\n")), "bin");
    }

    fn gen(
        crate_type: &str,
        features: &[&str],
        deps: &[(&str, &str)],
        build_script: Option<&str>,
        suffix: &str,
    ) -> String {
        gen_p(crate_type, features, deps, build_script, suffix, false)
    }

    fn gen_p(
        crate_type: &str,
        features: &[&str],
        deps: &[(&str, &str)],
        build_script: Option<&str>,
        suffix: &str,
        pipeline: bool,
    ) -> String {
        generate_build_file(
            "my-crate",
            "1.2.3",
            &cargo_toml::Edition::E2021,
            crate_type,
            &features.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            &deps
                .iter()
                .map(|(n, t)| (n.to_string(), t.to_string()))
                .collect::<Vec<_>>(),
            &[],
            build_script,
            "src/lib.rs",
            suffix,
            &[],
            pipeline,
        )
    }

    #[test]
    fn plain_lib_rule() {
        let out = gen("lib", &["std"], &[("dep_a", "///third_party/rust/dep_a//:dep_a")], None, "");
        assert!(out.contains("name = \"my_crate\""));
        assert!(out.contains("--crate-name my_crate"));
        assert!(out.contains("libmy_crate-1_2_3.rlib"));
        assert!(out.contains("--feature std"));
        assert!(out.contains("\"///third_party/rust/dep_a//:dep_a|externconfig\""));
        assert!(out.contains("-C metadata=my_crate-1.2.3"));
        assert!(!out.contains("_build_script"));
    }

    #[test]
    fn build_script_two_stage() {
        let out = gen("lib", &[], &[], Some("build/main.rs"), "");
        assert!(out.contains("name = \"_my_crate_build_script\""));
        assert!(out.contains("\"script\": [\"build/main.rs\"]"));
        assert!(out.contains("--buildscript $SRCS_BUILDSCRIPT"));
        assert!(out.contains("\"out\": [\"out\"]"));
        assert!(!out.contains("dep_metadata"));
    }

    #[test]
    fn linked_deps_feed_build_script() {
        let out = generate_build_file(
            "my-crate",
            "1.2.3",
            &cargo_toml::Edition::E2021,
            "lib",
            &[],
            &[],
            &[],
            Some("build.rs"),
            "src/lib.rs",
            "",
            &["///third_party/rust/libz_sys//:_libz_sys_build_script|buildscript".to_string()],
            false,
        );
        assert!(out.contains("\"dep_metadata\": ["));
        assert!(out.contains("///third_party/rust/libz_sys//:_libz_sys_build_script|buildscript"));
        assert!(out.contains("--dep-metadata $SRCS_DEP_METADATA"));
    }

    #[test]
    fn host_variant_naming() {
        let out = gen("lib", &["host_feat"], &[], None, "_host");
        // Rule and file names carry the suffix; the crate identity does not
        assert!(out.contains("name = \"my_crate_host\""));
        assert!(out.contains("--crate-name my_crate "));
        assert!(out.contains("libmy_crate-1_2_3_host.rlib"));
        assert!(out.contains("echo 'my_crate_host=libmy_crate-1_2_3_host.rlib'"));
    }

    #[test]
    fn proc_macro_rule_shape() {
        let out = gen("proc-macro", &[], &[], None, "");
        assert!(out.contains("--crate-type proc-macro"));
        assert!(out.contains("libmy_crate-1_2_3.so"));
        assert!(!out.contains("rmeta"));
    }

    #[test]
    fn renames_emitted_for_mismatched_deps() {
        let out = gen("lib", &[], &[("alias_name", "///third_party/rust/real//:real")], None, "");
        assert!(out.contains("--rename alias_name=real"));
    }

    #[test]
    fn pipelined_lib_shape() {
        let out = gen_p("lib", &[], &[("dep_a", "///third_party/rust/dep_a//:dep_a")], None, "", true);
        // Three-rule shape: link rule, metadata twin, public filegroup
        assert!(out.contains("name = \"_my_crate#link\""));
        assert!(out.contains("name = \"_my_crate#rmeta\""));
        assert!(out.contains("name = \"my_crate\""));
        // Compiles hang off deps' metadata twins
        assert!(out.contains("\"///third_party/rust/dep_a//:_dep_a#rmeta|externconfig\""));
        assert!(out.contains("\"///third_party/rust/dep_a//:_dep_a#rmeta\","));
        // The twin runs the identical compile, cut off at the rmeta artifact
        // (identical flags keep the svh in sync with the link rule)
        assert!(out.contains("--pipeline-rmeta"));
        assert!(out.contains("my_crate=libmy_crate-1_2_3.rmeta"));
        let link = out.split("name = \"_my_crate#rmeta\"").next().unwrap();
        assert!(!link.contains("--pipeline-rmeta"));
        // The link rule leaves the declared rmeta output to the twin
        assert!(!link.contains("\"rmeta\": [\"libmy_crate-1_2_3.rmeta\"]"));
        // The public filegroup keeps dep publics for transitive rlib staging
        assert!(out.contains("\"///third_party/rust/dep_a//:dep_a\","));
    }

    #[test]
    fn pipelined_proc_macro_shape() {
        let out = gen_p("proc-macro", &[], &[("dep_a", "///third_party/rust/dep_a//:dep_a")], None, "", true);
        // Proc-macros must fully build: twin is a copy-through of the link
        // rule's externconfig, and dep refs use the link rules
        assert!(out.contains("name = \"_my_crate#link\""));
        assert!(out.contains("name = \"_my_crate#rmeta\""));
        assert!(out.contains("cp $SRCS $OUTS_EXTERNCONFIG"));
        assert!(out.contains("\"///third_party/rust/dep_a//:_dep_a#link|externconfig\""));
        assert!(!out.contains("--pipeline-rmeta"));
    }

    #[test]
    fn pipelined_buildscript_twin_consumes_script() {
        let out = gen_p("lib", &[], &[], Some("build.rs"), "", true);
        // The metadata twin still consumes the build script output (cfgs/env
        // affect the frontend)
        let twin = out.split("name = \"_my_crate#rmeta\"").nth(1).unwrap();
        assert!(twin.contains("--buildscript $SRCS_BUILDSCRIPT"));
        assert!(twin.contains(":_my_crate_build_script|buildscript"));
    }

    #[test]
    fn bin_rule_shape() {
        let out = generate_bin_rule(
            "my_crate",
            "my-tool",
            "src/main.rs",
            "1.2.3",
            &cargo_toml::Edition::E2021,
            &[],
            &[("dep_a".to_string(), "///third_party/rust/dep_a//:dep_a".to_string())],
            true,
            false,
        );
        assert!(out.contains("name = \"my_tool_bin\""));
        assert!(out.contains("--crate-name my_tool"));
        assert!(out.contains("outs = [\"my_tool\"]"));
        assert!(out.contains("binary = True"));
        // Links its own lib plus deps
        assert!(out.contains("\":my_crate|externconfig\""));
        assert!(out.contains("\"///third_party/rust/dep_a//:dep_a\""));
    }
}

#[cfg(test)]
mod run_tests {
    use super::*;
    use crate::resolve::{LockDep, LockEntry, LockFile};
    use std::collections::BTreeMap;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("please_rust_gen_run_{}_{}", std::process::id(), name));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn crate_dir(root: &PathBuf, manifest: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = root.join("demo-1.0.0");
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("Cargo.toml"), manifest).unwrap();
        for (path, content) in files {
            let p = dir.join(path);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, content).unwrap();
        }
        dir
    }

    fn args(src_root: PathBuf, lock: Option<PathBuf>) -> GenerateArgs {
        GenerateArgs {
            crate_name: "demo".to_string(),
            version: "1.0.0".to_string(),
            src_root,
            subrepo: "third_party/rust/demo".to_string(),
            third_party_folder: "third_party/rust".to_string(),
            pipeline: false,
            features: "fallback-feat".to_string(),
            install: "".to_string(),
            overrides: vec![],
            lock,
            tool_label: "@//custom:tool".to_string(),
            rustc_label: "@//custom:rustc".to_string(),
            sysroot_label: "@//custom:sysroot".to_string(),
            cc_label: "@//custom:cc".to_string(),
        }
    }

    fn write_lock(dir: &PathBuf, host: bool) -> PathBuf {
        let mut crates = BTreeMap::new();
        crates.insert(
            "demo".to_string(),
            LockEntry {
                crate_name: "demo".to_string(),
                version: "1.0.0".to_string(),
                features: vec!["locked-feat".to_string()],
                deps: vec![LockDep {
                    name: "dep_a".to_string(),
                    crate_name: "dep-a".to_string(),
                    subrepo: "dep_a".to_string(),
                    target_name: "dep_a".to_string(),
                }],
                build_deps: vec![],
                links: None,
            },
        );
        let mut host_crates = BTreeMap::new();
        if host {
            host_crates.insert(
                "demo".to_string(),
                LockEntry {
                    crate_name: "demo".to_string(),
                    version: "1.0.0".to_string(),
                    features: vec!["host-feat".to_string()],
                    deps: vec![],
                    build_deps: vec![],
                    links: None,
                },
            );
        }
        let lock = LockFile {
            target: "x86_64-unknown-linux-gnu".to_string(),
            crates,
            host_crates,
        };
        let path = dir.join("rust.lock");
        fs::write(&path, serde_json::to_string(&lock).unwrap()).unwrap();
        path
    }

    #[test]
    fn lock_driven_generation() {
        let root = scratch("lock");
        let src = crate_dir(&root, "[package]\nname = \"demo\"\nversion = \"1.0.0\"\nedition = \"2021\"\n", &[("src/lib.rs", "")]);
        let lock = write_lock(&root, false);
        run(args(src.clone(), Some(lock))).unwrap();

        let build = fs::read_to_string(src.join("BUILD")).unwrap();
        assert!(build.contains("--feature locked-feat"));
        assert!(!build.contains("fallback-feat"));
        assert!(build.contains("///third_party/rust/dep_a//:dep_a|externconfig"));
        // Configured labels substituted everywhere
        assert!(build.contains("@//custom:tool"));
        assert!(build.contains("@//custom:rustc"));
        assert!(!build.contains("@//tools/please_rust:bootstrap"));
        // .plzconfig written for the subrepo
        assert!(src.join(".plzconfig").exists());
    }

    #[test]
    fn heuristic_fallback_without_lock() {
        let root = scratch("heuristic");
        let src = crate_dir(
            &root,
            "[package]\nname = \"demo\"\nversion = \"1.0.0\"\n\n[dependencies]\nserde = \"1\"\n",
            &[("src/lib.rs", "")],
        );
        run(args(src.clone(), None)).unwrap();
        let build = fs::read_to_string(src.join("BUILD")).unwrap();
        assert!(build.contains("--feature fallback-feat"));
        assert!(build.contains("///third_party/rust/serde//:serde"));
    }

    #[test]
    fn host_variant_and_bin_rules_emitted() {
        let root = scratch("hostbin");
        let src = crate_dir(
            &root,
            "[package]\nname = \"demo\"\nversion = \"1.0.0\"\n",
            &[("src/lib.rs", ""), ("src/main.rs", "fn main() {}")],
        );
        let lock = write_lock(&root, true);
        run(args(src.clone(), Some(lock))).unwrap();
        let build = fs::read_to_string(src.join("BUILD")).unwrap();
        assert!(build.contains("name = \"demo\""));
        assert!(build.contains("name = \"demo_host\""));
        assert!(build.contains("--feature host-feat"));
        assert!(build.contains("name = \"demo_bin\""));
    }

    #[test]
    fn build_script_detected_and_custom_lib_path() {
        let root = scratch("bs");
        let src = crate_dir(
            &root,
            "[package]\nname = \"demo\"\nversion = \"1.0.0\"\n\n[lib]\npath = \"lib.rs\"\n",
            &[("lib.rs", ""), ("build.rs", "fn main() {}")],
        );
        let lock = write_lock(&root, false);
        run(args(src.clone(), Some(lock))).unwrap();
        let build = fs::read_to_string(src.join("BUILD")).unwrap();
        assert!(build.contains("_demo_build_script"));
        assert!(build.contains("\"main\": [\"lib.rs\"]"));
    }
}

#[cfg(test)]
mod heuristic_tests {
    use super::*;
    use std::collections::HashMap;

    fn manifest(body: &str) -> Manifest {
        crate::resolve::parse_manifest(
            format!("[package]\nname = \"t\"\nversion = \"1.0.0\"\n{}", body).as_bytes(),
        )
        .unwrap()
    }

    #[test]
    fn heuristic_optional_and_overrides() {
        let m = manifest(
            "[dependencies]\nplain = \"1\"\n\n[dependencies.opt]\nversion = \"1\"\noptional = true\n\n[dependencies.gated]\nversion = \"1\"\noptional = true\n\n[features]\nwith_gated = [\"dep:gated\"]\n\n[target.'cfg(windows)'.dependencies]\nwinonly = \"1\"\n",
        );
        let mut overrides = HashMap::new();
        overrides.insert("plain".to_string(), "plain-0.9.0".to_string());

        // No features: only the mandatory dep, routed through the override
        let deps = resolve_dependencies(&m, "third_party/rust", &[], &overrides);
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].1, "///third_party/rust/plain-0.9.0//:plain");

        // Feature-gated and name-activated optionals
        let deps = resolve_dependencies(
            &m,
            "third_party/rust",
            &["with_gated".to_string(), "opt".to_string()],
            &HashMap::new(),
        );
        let names: Vec<&str> = deps.iter().map(|d| d.0.as_str()).collect();
        assert!(names.contains(&"gated"));
        assert!(names.contains(&"opt"));
        assert!(!names.contains(&"winonly"));
    }

    #[test]
    fn heuristic_build_deps() {
        let m = manifest("[build-dependencies]\ncc = \"1\"\n\n[target.'cfg(unix)'.build-dependencies]\nub = \"1\"\n");
        let deps = resolve_build_dependencies(&m, "third_party/rust");
        let names: Vec<&str> = deps.iter().map(|d| d.0.as_str()).collect();
        assert_eq!(names, vec!["cc", "ub"]);
    }
}
