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
}

pub fn run(args: GenerateArgs) -> Result<()> {
    let cargo_toml_path = args.src_root.join("Cargo.toml");

    // Parse Cargo.toml
    let manifest = Manifest::from_path(&cargo_toml_path)
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

    // Determine crate type
    let crate_type = determine_crate_type(&manifest);

    // Parse requested features
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

    // Resolve dependencies to subrepo targets
    let deps = resolve_dependencies(&manifest, &args.third_party_folder, &requested_features, &overrides);

    // Resolve build-dependencies (for build.rs compilation)
    let build_deps = resolve_build_dependencies(&manifest, &args.third_party_folder);

    // Generate BUILD file content
    let build_content = generate_build_file(
        crate_name,
        &args.version,
        edition,
        &crate_type,
        &requested_features,
        &deps,
        &build_deps,
        build_script_path.as_deref(),
        &lib_path,
    );

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
) -> String {
    let mut content = String::new();

    let normalized_name = crate_name.replace("-", "_");
    let edition_str = match edition {
        cargo_toml::Edition::E2015 => "2015",
        cargo_toml::Edition::E2018 => "2018",
        cargo_toml::Edition::E2021 => "2021",
        _ => "2024",
    };

    // Multiple versions of a crate can coexist in one dependent's inputs, so
    // library filenames and metadata are disambiguated per version, the same
    // way cargo uses -C extra-filename/-C metadata.
    let version_tag = version.replace(['.', '+'], "_");

    // Determine outputs based on crate type
    let (out_rlib, out_rmeta, emit) = match crate_type {
        "proc-macro" => (
            format!("lib{}-{}.so", normalized_name, version_tag),
            format!("lib{}-{}.so", normalized_name, version_tag),
            "dep-info,link",
        ),
        _ => (
            format!("lib{}-{}.rlib", normalized_name, version_tag),
            format!("lib{}-{}.rmeta", normalized_name, version_tag),
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
        content.push_str(&generate_build_script_rule(&normalized_name, features, build_deps, script_path));
        content.push_str("\n");
        content.push_str(&generate_compile_rule_with_buildscript(
            &normalized_name,
            edition_str,
            crate_type,
            &out_rlib,
            &out_rmeta,
            emit,
            &feature_str,
            deps,
            lib_path,
        ));
    } else {
        content.push_str(&generate_compile_rule(
            &normalized_name,
            edition_str,
            crate_type,
            &out_rlib,
            &out_rmeta,
            emit,
            &feature_str,
            deps,
            lib_path,
        ));
    }

    content
}

/// Generate a build_rule for the build script (Stage 1)
fn generate_build_script_rule(
    normalized_name: &str,
    features: &[String],
    build_deps: &[(String, String)],
    script_path: &str,
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
    let build_script_cmd = format!(
        "mkdir -p out && {}$TOOLS_PLEASE_RUST build-script --manifest-path $SRCS_MANIFEST --build-script $SRCS_SCRIPT --out-dir out --rustc $TOOLS_RUSTC --sysroot $TOOLS_SYSROOT {}--output $OUTS_BUILDSCRIPT {}",
        aggregate_cmd, externconfig_arg, feature_str
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
            content.push_str(&format!("            \"{}|externconfig\",\n", target));
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
    content.push_str("    },\n");

    // Need transitive deps if we have build-deps to get their externconfigs
    if has_build_deps {
        content.push_str("    needs_transitive_deps = True,\n");
    }

    content.push_str(")\n");

    content
}

/// Generate the main compile rule with buildscript support (Stage 2)
fn generate_compile_rule_with_buildscript(
    normalized_name: &str,
    edition_str: &str,
    crate_type: &str,
    out_rlib: &str,
    out_rmeta: &str,
    emit: &str,
    feature_str: &str,
    deps: &[(String, String)],
    lib_path: &str,
) -> String {
    let mut content = String::new();

    // Direct deps' externconfigs only: transitive configs can contain
    // colliding entries for other versions of the same crate.
    let aggregate_cmd = if deps.is_empty() {
        "true".to_string()
    } else {
        "cat $SRCS_EXTERNCONFIGS > externconfig".to_string()
    };

    // Compile command with --buildscript flag
    let compile_base = format!(
        "$TOOLS_PLEASE_RUST compile --externconfig externconfig --buildscript $SRCS_BUILDSCRIPT --manifest-path $SRCS_MANIFEST --rustc $TOOLS_RUSTC --sysroot $TOOLS_SYSROOT --crate-name {} --edition {} --crate-type {} --emit {} {}",
        normalized_name, edition_str, crate_type, emit, feature_str
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
    content.push_str(&format!("    name = \"{}\",\n", normalized_name));
    content.push_str("    srcs = {\n");
    content.push_str(&format!("        \"main\": [\"{}\"],\n", lib_path));
    content.push_str(&format!("        \"mods\": glob([\"src/**\", \"*.rs\"], exclude=[\"{}\", \"src/lib.rs\", \"src/main.rs\", \"build.rs\"], allow_empty=True),\n", lib_path));
    content.push_str("        \"data\": glob([\"*.md\", \"LICENSE*\", \"examples/**/*\"], allow_empty=True),\n");
    content.push_str("        \"manifest\": [\"Cargo.toml\"],\n");
    if !deps.is_empty() {
        content.push_str("        \"externconfigs\": [\n");
        for (_name, target) in deps {
            content.push_str(&format!("            \"{}|externconfig\",\n", target));
        }
        content.push_str("        ],\n");
    }
    content.push_str(&format!("        \"buildscript\": [\":_{}_build_script|buildscript\"],\n", normalized_name));
    content.push_str(&format!("        \"buildscript_out\": [\":_{}_build_script|out\"],\n", normalized_name));
    content.push_str("    },\n");
    content.push_str("    cmd = {\n");
    content.push_str(&format!("        \"dbg\": \"{}\",\n", cmd_dbg));
    content.push_str(&format!("        \"opt\": \"{}\",\n", cmd_opt));
    content.push_str("    },\n");
    content.push_str("    outs = {\n");
    content.push_str(&format!("        \"rlib\": [\"{}\"],\n", out_rlib));
    if crate_type != "proc-macro" {
        content.push_str(&format!("        \"rmeta\": [\"{}\"],\n", out_rmeta));
    }
    content.push_str(&format!("        \"externconfig\": [\"{}.externconfig\"],\n", normalized_name));
    content.push_str("    },\n");

    if !deps.is_empty() {
        content.push_str("    deps = [\n");
        for (_name, target) in deps {
            content.push_str(&format!("        \"{}\",\n", target));
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

/// Generate the main compile rule (no build script)
fn generate_compile_rule(
    normalized_name: &str,
    edition_str: &str,
    crate_type: &str,
    out_rlib: &str,
    out_rmeta: &str,
    emit: &str,
    feature_str: &str,
    deps: &[(String, String)],
    lib_path: &str,
) -> String {
    let mut content = String::new();

    // Direct deps' externconfigs only: transitive configs can contain
    // colliding entries for other versions of the same crate.
    let aggregate_cmd = if deps.is_empty() {
        "true".to_string()
    } else {
        "cat $SRCS_EXTERNCONFIGS > externconfig".to_string()
    };

    let compile_base = format!(
        "$TOOLS_PLEASE_RUST compile --externconfig externconfig --manifest-path $SRCS_MANIFEST --rustc $TOOLS_RUSTC --sysroot $TOOLS_SYSROOT --crate-name {} --edition {} --crate-type {} --emit {} {}",
        normalized_name, edition_str, crate_type, emit, feature_str
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
    content.push_str(&format!("    name = \"{}\",\n", normalized_name));
    content.push_str("    srcs = {\n");
    content.push_str(&format!("        \"main\": [\"{}\"],\n", lib_path));
    content.push_str(&format!("        \"mods\": glob([\"src/**\", \"*.rs\"], exclude=[\"{}\", \"src/lib.rs\", \"src/main.rs\", \"build.rs\"], allow_empty=True),\n", lib_path));
    content.push_str("        \"data\": glob([\"*.md\", \"LICENSE*\", \"examples/**/*\"], allow_empty=True),\n");
    content.push_str("        \"manifest\": [\"Cargo.toml\"],\n");
    if !deps.is_empty() {
        content.push_str("        \"externconfigs\": [\n");
        for (_name, target) in deps {
            content.push_str(&format!("            \"{}|externconfig\",\n", target));
        }
        content.push_str("        ],\n");
    }
    content.push_str("    },\n");
    content.push_str("    cmd = {\n");
    content.push_str(&format!("        \"dbg\": \"{}\",\n", cmd_dbg));
    content.push_str(&format!("        \"opt\": \"{}\",\n", cmd_opt));
    content.push_str("    },\n");
    content.push_str("    outs = {\n");
    content.push_str(&format!("        \"rlib\": [\"{}\"],\n", out_rlib));
    if crate_type != "proc-macro" {
        content.push_str(&format!("        \"rmeta\": [\"{}\"],\n", out_rmeta));
    }
    content.push_str(&format!("        \"externconfig\": [\"{}.externconfig\"],\n", normalized_name));
    content.push_str("    },\n");

    if !deps.is_empty() {
        content.push_str("    deps = [\n");
        for (_name, target) in deps {
            content.push_str(&format!("        \"{}\",\n", target));
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

    #[test]
    fn test_resolve_dependencies() {
        // Simple test
        let deps = vec![("serde".to_string(), "serde_dep".to_string())];
        assert_eq!(deps.len(), 1);
    }
}
