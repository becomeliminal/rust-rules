use anyhow::{Context, Result};
use clap::Parser;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Generate rust_crate BUILD definitions from a Cargo.toml file.
///
/// This tool takes a Cargo.toml, runs cargo metadata to get resolved dependencies
/// with features, and outputs rust_crate definitions compatible with the please build system.
#[derive(Parser, Debug)]
#[command(name = "straddle_carrier")]
#[command(version = "0.1.0")]
#[command(about = "Generate rust_crate BUILD definitions from Cargo.toml")]
#[command(long_about = "Straddle Carrier converts Cargo.toml dependencies into Please BUILD file \
    rust_crate definitions. It uses `cargo metadata` to resolve the complete dependency tree \
    with all activated features, ensuring compatibility with the Please build system.")]
#[command(arg_required_else_help = true)]
struct Args {
    /// Path to the Cargo.toml file to analyze
    #[arg(short, long, value_name = "FILE")]
    cargo_toml: PathBuf,

    /// Output file for the package BUILD (binary/library rules).
    /// If not specified, prints to stdout.
    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,

    /// Output file for third-party crate definitions.
    /// If not specified, prints to stdout.
    #[arg(long, value_name = "FILE")]
    third_party_output: Option<PathBuf>,

    /// Keep the generated Cargo.lock (don't delete it)
    #[arg(long, default_value = "false")]
    keep_lockfile: bool,
}

/// Represents a resolved package from cargo metadata
#[derive(Debug, Clone)]
struct ResolvedPackage {
    name: String,
    version: String,
    is_local: bool,
    features: Vec<String>,
    dependencies: Vec<String>,
}

/// Represents the Cargo.toml [package] section
#[derive(Debug, Deserialize)]
struct CargoTomlPackage {
    name: String,
    #[serde(default = "default_edition")]
    edition: String,
}

fn default_edition() -> String {
    "2021".to_string()
}

/// Represents a dependency in Cargo.toml
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Dependency {
    Simple(String),
    Detailed(DetailedDependency),
}

#[derive(Debug, Deserialize)]
struct DetailedDependency {
    #[allow(dead_code)]
    version: Option<String>,
    #[allow(dead_code)]
    path: Option<String>,
    #[allow(dead_code)]
    git: Option<String>,
}

/// Represents a minimal Cargo.toml structure
#[derive(Debug, Deserialize)]
struct CargoToml {
    package: CargoTomlPackage,
    #[serde(default)]
    dependencies: HashMap<String, Dependency>,
    #[serde(default)]
    lib: Option<CargoLib>,
    #[serde(default)]
    bin: Option<Vec<CargoBin>>,
}

#[derive(Debug, Deserialize)]
struct CargoLib {
    #[allow(dead_code)]
    name: Option<String>,
    #[allow(dead_code)]
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CargoBin {
    name: String,
    #[allow(dead_code)]
    path: Option<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Read the Cargo.toml to get the root package name
    let cargo_toml_content = fs::read_to_string(&args.cargo_toml)
        .with_context(|| format!("Failed to read Cargo.toml at {:?}", args.cargo_toml))?;
    let cargo_toml: CargoToml = toml::from_str(&cargo_toml_content)
        .with_context(|| "Failed to parse Cargo.toml")?;

    // Get cargo metadata with resolved features
    let resolved_packages = get_cargo_metadata(&args.cargo_toml)?;

    // Determine the project root directory
    let project_dir = args.cargo_toml.parent().unwrap_or(Path::new("."));

    // Generate the BUILD file contents
    let (package_build, third_party_build) = generate_build_files(&resolved_packages, &cargo_toml, project_dir)?;

    // Output the package BUILD file
    if let Some(output_path) = args.output {
        fs::write(&output_path, &package_build)
            .with_context(|| format!("Failed to write output to {:?}", output_path))?;
        eprintln!("Wrote package BUILD to {:?}", output_path);
    } else {
        println!("# === Package BUILD ===");
        print!("{}", package_build);
    }

    // Output the third-party crates BUILD file
    if let Some(tp_path) = args.third_party_output {
        fs::write(&tp_path, &third_party_build)
            .with_context(|| format!("Failed to write third-party output to {:?}", tp_path))?;
        eprintln!("Wrote third-party BUILD to {:?}", tp_path);
    } else {
        println!("\n# === Third-party crates (add to third_party/rust/BUILD) ===");
        print!("{}", third_party_build);
    }

    Ok(())
}

/// Cargo metadata JSON structures
#[derive(Debug, Deserialize)]
struct CargoMetadata {
    packages: Vec<MetadataPackage>,
    resolve: Option<MetadataResolve>,
}

#[derive(Debug, Deserialize)]
struct MetadataPackage {
    name: String,
    version: String,
    id: String,
    source: Option<String>,
    #[serde(default)]
    dependencies: Vec<MetadataDep>,
}

#[derive(Debug, Deserialize)]
struct MetadataDep {
    name: String,
    #[serde(default)]
    uses_default_features: bool,
    #[serde(default)]
    features: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct MetadataResolve {
    nodes: Vec<MetadataNode>,
}

#[derive(Debug, Deserialize)]
struct MetadataNode {
    id: String,
    #[serde(default)]
    features: Vec<String>,
    #[serde(default)]
    deps: Vec<MetadataNodeDep>,
}

#[derive(Debug, Deserialize)]
struct MetadataNodeDep {
    name: String,
    pkg: String,
}

/// Get cargo metadata with resolved features
fn get_cargo_metadata(cargo_toml_path: &PathBuf) -> Result<Vec<ResolvedPackage>> {
    // Run cargo metadata
    let output = Command::new("cargo")
        .arg("metadata")
        .arg("--format-version=1")
        .arg("--manifest-path")
        .arg(cargo_toml_path)
        .output()
        .context("Failed to run cargo metadata")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("cargo metadata failed: {}", stderr);
    }

    let metadata: CargoMetadata = serde_json::from_slice(&output.stdout)
        .context("Failed to parse cargo metadata JSON")?;

    // Build a map from package id to package info
    let mut pkg_map: HashMap<String, &MetadataPackage> = HashMap::new();
    for pkg in &metadata.packages {
        pkg_map.insert(pkg.id.clone(), pkg);
    }

    // Build a map from package id to resolved features and dependencies
    let mut resolved: Vec<ResolvedPackage> = Vec::new();

    if let Some(resolve) = &metadata.resolve {
        for node in &resolve.nodes {
            if let Some(pkg) = pkg_map.get(&node.id) {
                let is_local = pkg.source.is_none();
                let deps: Vec<String> = node.deps.iter().map(|d| d.name.clone()).collect();
                
                resolved.push(ResolvedPackage {
                    name: pkg.name.clone(),
                    version: pkg.version.clone(),
                    is_local,
                    features: node.features.clone(),
                    dependencies: deps,
                });
            }
        }
    }

    eprintln!("Got metadata for {} packages", resolved.len());
    Ok(resolved)
}

/// Determine what kind of targets exist in the project
struct ProjectTargets {
    has_lib: bool,
    has_bin: bool,
    bin_names: Vec<String>,
}

fn detect_project_targets(cargo_toml: &CargoToml, project_dir: &Path) -> ProjectTargets {
    // Check for library
    let has_lib = cargo_toml.lib.is_some() || project_dir.join("src/lib.rs").exists();

    // Check for binaries
    let has_default_bin = project_dir.join("src/main.rs").exists();
    let mut bin_names: Vec<String> = Vec::new();

    if let Some(bins) = &cargo_toml.bin {
        for bin in bins {
            bin_names.push(bin.name.clone());
        }
    }

    // If no explicit bins but src/main.rs exists, the binary name is the package name
    let has_bin = has_default_bin || !bin_names.is_empty();
    if has_default_bin && bin_names.is_empty() {
        bin_names.push(cargo_toml.package.name.clone());
    }

    ProjectTargets {
        has_lib,
        has_bin,
        bin_names,
    }
}

/// Generate BUILD file contents from resolved packages
/// Returns (package_build, third_party_build)
fn generate_build_files(packages: &[ResolvedPackage], cargo_toml: &CargoToml, project_dir: &Path) -> Result<(String, String)> {
    let mut package_output = String::new();
    let mut third_party_output = String::new();
    let root_package_name = &cargo_toml.package.name;
    let edition = &cargo_toml.package.edition;

    // Add the subinclude to package BUILD
    package_output.push_str("subinclude(\"//build_defs:rust\")\n\n");

    // Build a map of package name -> rule name for dependency resolution
    let mut package_to_rule: HashMap<String, String> = HashMap::new();
    for pkg in packages {
        // Skip the root package
        if pkg.name == *root_package_name {
            continue;
        }
        // Skip local packages
        if pkg.is_local {
            continue;
        }
        let rule_name = crate_name_to_rule_name(&pkg.name);
        package_to_rule.insert(pkg.name.clone(), rule_name);
    }

    // Get the direct dependencies from Cargo.toml
    let direct_deps: HashSet<String> = cargo_toml
        .dependencies
        .keys()
        .cloned()
        .collect();

    // Detect project targets
    let targets = detect_project_targets(cargo_toml, project_dir);

    // Generate rust_library if it has a lib.rs
    if targets.has_lib {
        let lib_name = crate_name_to_rule_name(root_package_name);
        package_output.push_str("rust_library(\n");
        package_output.push_str(&format!("    name = \"{}\",\n", lib_name));
        package_output.push_str("    root = \"src/lib.rs\",\n");
        package_output.push_str(&format!("    edition = \"{}\",\n", edition));

        // Add direct dependencies (pointing to //third_party/rust:xxx)
        let lib_deps: Vec<String> = direct_deps
            .iter()
            .filter_map(|dep| package_to_rule.get(dep).map(|rule| format!("\"//third_party/rust:{}\",", rule)))
            .collect();

        if !lib_deps.is_empty() {
            package_output.push_str("    deps = [\n");
            for dep in lib_deps {
                package_output.push_str(&format!("        {}\n", dep));
            }
            package_output.push_str("    ],\n");
        }

        package_output.push_str(")\n\n");
    }

    // Generate rust_binary for each binary
    if targets.has_bin {
        for bin_name in &targets.bin_names {
            let rule_name = crate_name_to_rule_name(bin_name);
            package_output.push_str("rust_binary(\n");
            package_output.push_str(&format!("    name = \"{}\",\n", rule_name));
            package_output.push_str("    main = \"src/main.rs\",\n");
            package_output.push_str(&format!("    edition = \"{}\",\n", edition));

            // Add direct dependencies (pointing to //third_party/rust:xxx)
            let mut bin_deps: Vec<String> = direct_deps
                .iter()
                .filter_map(|dep| package_to_rule.get(dep).map(|rule| format!("\"//third_party/rust:{}\",", rule)))
                .collect();

            // If there's a library, the binary depends on it (local reference)
            if targets.has_lib {
                let lib_rule = crate_name_to_rule_name(root_package_name);
                bin_deps.insert(0, format!("\":{}\",", lib_rule));
            }

            if !bin_deps.is_empty() {
                package_output.push_str("    deps = [\n");
                for dep in bin_deps {
                    package_output.push_str(&format!("        {}\n", dep));
                }
                package_output.push_str("    ],\n");
            }

            package_output.push_str(")\n\n");
        }
    }

    // Generate rust_crate definitions for third-party dependencies
    for pkg in packages {
        // Skip the root package
        if pkg.name == *root_package_name {
            continue;
        }
        // Skip local packages
        if pkg.is_local {
            continue;
        }

        let rule_name = crate_name_to_rule_name(&pkg.name);
        let crate_name = &pkg.name;
        let version = &pkg.version;

        // Determine edition (default to 2021 as a reasonable default)
        let crate_edition = "2021";

        third_party_output.push_str("rust_crate(\n");
        third_party_output.push_str(&format!("    name = \"{}\",\n", rule_name));
        third_party_output.push_str(&format!("    crate = \"{}\",\n", crate_name));
        third_party_output.push_str(&format!("    version = \"{}\",\n", version));
        third_party_output.push_str(&format!("    edition = \"{}\",\n", crate_edition));

        // Add features if any
        if !pkg.features.is_empty() {
            third_party_output.push_str("    features = [\n");
            for feature in &pkg.features {
                third_party_output.push_str(&format!("        \"{}\",\n", feature));
            }
            third_party_output.push_str("    ],\n");
        }

        // Add dependencies (local references within third_party/rust)
        if !pkg.dependencies.is_empty() {
            let deps: Vec<String> = pkg
                .dependencies
                .iter()
                .filter_map(|dep_name| {
                    package_to_rule.get(dep_name).map(|rule| format!("\":{}\",", rule))
                })
                .collect();

            if !deps.is_empty() {
                third_party_output.push_str("    deps = [\n");
                for dep in deps {
                    third_party_output.push_str(&format!("        {}\n", dep));
                }
                third_party_output.push_str("    ],\n");
            }
        }

        third_party_output.push_str(")\n\n");
    }

    Ok((package_output, third_party_output))
}

/// Convert a crate name (with hyphens) to a rule name (with underscores)
fn crate_name_to_rule_name(crate_name: &str) -> String {
    crate_name.replace('-', "_")
}

