use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Generate and merge rust_crate BUILD definitions from Cargo.toml files.
///
/// Straddle Carrier converts Cargo.toml dependencies into Please BUILD file
/// rust_crate definitions. It uses `cargo metadata` to resolve the complete dependency tree
/// with all activated features, ensuring compatibility with the Please build system.
#[derive(Parser, Debug)]
#[command(name = "straddle_carrier")]
#[command(version = "0.1.0")]
#[command(about = "Generate and merge rust_crate BUILD definitions")]
#[command(long_about = "Straddle Carrier converts Cargo.toml dependencies into Please BUILD file \
    rust_crate definitions. It uses `cargo metadata` to resolve the complete dependency tree \
    with all activated features, ensuring compatibility with the Please build system.")]
#[command(arg_required_else_help = true)]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>,

    // Legacy mode (for backwards compatibility)
    /// Path to the Cargo.toml file to analyze
    #[arg(short, long, value_name = "FILE")]
    cargo_toml: Option<PathBuf>,

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

#[derive(Subcommand, Debug)]
enum Commands {
    /// Generate BUILD definitions from a Cargo.toml
    Generate {
        /// Path to the Cargo.toml file to analyze
        #[arg(short, long, value_name = "FILE")]
        cargo_toml: PathBuf,

        /// Output file for the package BUILD (binary/library rules)
        #[arg(short, long, value_name = "FILE")]
        output: Option<PathBuf>,

        /// Output file for third-party crate definitions
        #[arg(long, value_name = "FILE")]
        third_party_output: Option<PathBuf>,

        /// Keep the generated Cargo.lock
        #[arg(long, default_value = "false")]
        keep_lockfile: bool,
    },

    /// Merge crate definitions from two sources
    Merge {
        /// The existing BUILD file to update
        #[arg(long, value_name = "FILE")]
        old_source: PathBuf,

        /// The new source (BUILD file or Cargo.toml)
        #[arg(long, value_name = "FILE")]
        new_source: PathBuf,

        /// Merge mode
        #[arg(long, value_enum)]
        mode: MergeMode,

        /// Don't create a backup of the old file
        #[arg(long, default_value = "false")]
        no_backup: bool,

        /// Output file (defaults to old_source for in-place update)
        #[arg(short, long, value_name = "FILE")]
        output: Option<PathBuf>,
    },
}

/// Merge mode for combining crate definitions
#[derive(Debug, Clone, Copy, ValueEnum)]
enum MergeMode {
    /// Replace old dependencies with new ones completely
    Override,
    /// Only bump versions within semver, add features, never downgrade or remove
    UpdateOrExpandOnly,
    /// For conflicting crates, append version suffix to new ones
    Parallel,
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

    match args.command {
        Some(Commands::Generate { cargo_toml, output, third_party_output, keep_lockfile: _ }) => {
            run_generate(&cargo_toml, output.as_ref(), third_party_output.as_ref())
        }
        Some(Commands::Merge { old_source, new_source, mode, no_backup, output }) => {
            run_merge(&old_source, &new_source, mode, no_backup, output.as_ref())
        }
        None => {
            // Legacy mode: use top-level arguments
            if let Some(cargo_toml) = args.cargo_toml {
                run_generate(&cargo_toml, args.output.as_ref(), args.third_party_output.as_ref())
            } else {
                eprintln!("Error: No command specified. Use --help for usage.");
                std::process::exit(1);
            }
        }
    }
}

fn run_generate(cargo_toml_path: &PathBuf, output: Option<&PathBuf>, third_party_output: Option<&PathBuf>) -> Result<()> {
    // Read the Cargo.toml to get the root package name
    let cargo_toml_content = fs::read_to_string(cargo_toml_path)
        .with_context(|| format!("Failed to read Cargo.toml at {:?}", cargo_toml_path))?;
    let cargo_toml: CargoToml = toml::from_str(&cargo_toml_content)
        .with_context(|| "Failed to parse Cargo.toml")?;

    // Get cargo metadata with resolved features
    let resolved_packages = get_cargo_metadata(cargo_toml_path)?;

    // Determine the project root directory
    let project_dir = cargo_toml_path.parent().unwrap_or(Path::new("."));

    // Generate the BUILD file contents
    let (package_build, third_party_build) = generate_build_files(&resolved_packages, &cargo_toml, project_dir)?;

    // Output the package BUILD file
    if let Some(output_path) = output {
        fs::write(output_path, &package_build)
            .with_context(|| format!("Failed to write output to {:?}", output_path))?;
        eprintln!("Wrote package BUILD to {:?}", output_path);
    } else {
        println!("# === Package BUILD ===");
        print!("{}", package_build);
    }

    // Output the third-party crates BUILD file
    if let Some(tp_path) = third_party_output {
        fs::write(tp_path, &third_party_build)
            .with_context(|| format!("Failed to write third-party output to {:?}", tp_path))?;
        eprintln!("Wrote third-party BUILD to {:?}", tp_path);
    } else {
        println!("\n# === Third-party crates (add to third_party/rust/BUILD) ===");
        print!("{}", third_party_build);
    }

    Ok(())
}

/// A parsed rust_crate definition from a BUILD file
#[derive(Debug, Clone)]
struct CrateDefinition {
    name: String,
    crate_name: String,
    version: String,
    edition: Option<String>,
    features: Vec<String>,
    deps: Vec<String>,
    crate_type: Option<String>,
    build_root: Option<String>,
    /// The raw text of the entire rust_crate(...) block
    raw_text: String,
}

impl CrateDefinition {
    /// Parse a semantic version into (major, minor, patch)
    fn parse_semver(&self) -> Option<(u64, u64, u64)> {
        parse_semver(&self.version)
    }
}

fn parse_semver(version: &str) -> Option<(u64, u64, u64)> {
    // Strip any build metadata (e.g., "1.0.1+wasi-0.2.4" -> "1.0.1")
    let version = version.split('+').next().unwrap_or(version);
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() >= 3 {
        let major = parts[0].parse().ok()?;
        let minor = parts[1].parse().ok()?;
        let patch = parts[2].parse().ok()?;
        Some((major, minor, patch))
    } else if parts.len() == 2 {
        let major = parts[0].parse().ok()?;
        let minor = parts[1].parse().ok()?;
        Some((major, minor, 0))
    } else {
        None
    }
}

/// Parse rust_crate definitions from a BUILD file
fn parse_build_file(content: &str) -> Result<Vec<CrateDefinition>> {
    let mut crates = Vec::new();
    let mut i = 0;
    let chars: Vec<char> = content.chars().collect();

    while i < chars.len() {
        // Look for "rust_crate("
        if content[i..].starts_with("rust_crate(") {
            let start = i;
            i += "rust_crate(".len();

            // Find matching closing paren
            let mut paren_depth = 1;
            while i < chars.len() && paren_depth > 0 {
                match chars[i] {
                    '(' => paren_depth += 1,
                    ')' => paren_depth -= 1,
                    _ => {}
                }
                i += 1;
            }

            let raw_text = content[start..i].to_string();
            if let Some(crate_def) = parse_single_crate(&raw_text) {
                crates.push(crate_def);
            }
        } else {
            i += 1;
        }
    }

    Ok(crates)
}

/// Parse a single rust_crate(...) block
fn parse_single_crate(text: &str) -> Option<CrateDefinition> {
    // Extract name = "..."
    let name = extract_string_field(text, "name")?;
    let crate_name = extract_string_field(text, "crate").unwrap_or_else(|| name.clone());
    let version = extract_string_field(text, "version").unwrap_or_else(|| "0.0.0".to_string());
    let edition = extract_string_field(text, "edition");
    let crate_type = extract_string_field(text, "crate_type");
    let build_root = extract_string_field(text, "build_root");
    let features = extract_list_field(text, "features");
    let deps = extract_list_field(text, "deps");

    Some(CrateDefinition {
        name,
        crate_name,
        version,
        edition,
        features,
        deps,
        crate_type,
        build_root,
        raw_text: text.to_string(),
    })
}

/// Extract a string field like: name = "value"
fn extract_string_field(text: &str, field: &str) -> Option<String> {
    let pattern = format!(r#"{}\s*=\s*""#, field);
    let re = regex_lite::Regex::new(&pattern).ok()?;
    
    if let Some(m) = re.find(text) {
        let start = m.end();
        let rest = &text[start..];
        if let Some(end) = rest.find('"') {
            return Some(rest[..end].to_string());
        }
    }
    None
}

/// Extract a list field like: features = ["a", "b"]
fn extract_list_field(text: &str, field: &str) -> Vec<String> {
    let pattern = format!(r#"{}\s*=\s*\["#, field);
    let re = match regex_lite::Regex::new(&pattern) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };

    if let Some(m) = re.find(text) {
        let start = m.end();
        let rest = &text[start..];
        
        // Find matching ]
        let mut depth = 1;
        let mut end = 0;
        for (i, c) in rest.chars().enumerate() {
            match c {
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i;
                        break;
                    }
                }
                _ => {}
            }
        }

        let list_content = &rest[..end];
        // Extract all quoted strings
        let string_re = regex_lite::Regex::new(r#""([^"]*)""#).unwrap();
        return string_re
            .captures_iter(list_content)
            .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
            .collect();
    }

    Vec::new()
}

/// Generate a rust_crate block from a CrateDefinition
fn generate_crate_block(crate_def: &CrateDefinition) -> String {
    let mut output = String::new();
    output.push_str("rust_crate(\n");
    output.push_str(&format!("    name = \"{}\",\n", crate_def.name));
    output.push_str(&format!("    crate = \"{}\",\n", crate_def.crate_name));
    output.push_str(&format!("    version = \"{}\",\n", crate_def.version));
    
    if let Some(edition) = &crate_def.edition {
        output.push_str(&format!("    edition = \"{}\",\n", edition));
    }
    
    if let Some(crate_type) = &crate_def.crate_type {
        output.push_str(&format!("    crate_type = \"{}\",\n", crate_type));
    }

    if !crate_def.features.is_empty() {
        output.push_str("    features = [\n");
        for feature in &crate_def.features {
            output.push_str(&format!("        \"{}\",\n", feature));
        }
        output.push_str("    ],\n");
    }

    if !crate_def.deps.is_empty() {
        output.push_str("    deps = [\n");
        for dep in &crate_def.deps {
            output.push_str(&format!("        \"{}\",\n", dep));
        }
        output.push_str("    ],\n");
    }

    if let Some(build_root) = &crate_def.build_root {
        output.push_str(&format!("    build_root = \"{}\",\n", build_root));
    }

    output.push_str(")\n");
    output
}

fn run_merge(
    old_source: &PathBuf,
    new_source: &PathBuf,
    mode: MergeMode,
    no_backup: bool,
    output: Option<&PathBuf>,
) -> Result<()> {
    // Read old BUILD file
    let old_content = fs::read_to_string(old_source)
        .with_context(|| format!("Failed to read old source: {:?}", old_source))?;

    // Determine if new_source is a Cargo.toml or BUILD file
    let new_source_str = new_source.to_string_lossy();
    let new_crates = if new_source_str.ends_with("Cargo.toml") {
        // Generate from Cargo.toml
        let resolved_packages = get_cargo_metadata(new_source)?;
        let cargo_toml_content = fs::read_to_string(new_source)
            .with_context(|| format!("Failed to read Cargo.toml at {:?}", new_source))?;
        let cargo_toml: CargoToml = toml::from_str(&cargo_toml_content)
            .with_context(|| "Failed to parse Cargo.toml")?;
        
        // Convert resolved packages to CrateDefinitions
        resolved_packages
            .iter()
            .filter(|p| !p.is_local && p.name != cargo_toml.package.name)
            .map(|pkg| CrateDefinition {
                name: crate_name_to_rule_name(&pkg.name),
                crate_name: pkg.name.clone(),
                version: pkg.version.clone(),
                edition: Some("2021".to_string()),
                features: pkg.features.clone(),
                deps: pkg.dependencies.iter().map(|d| format!(":{}", crate_name_to_rule_name(d))).collect(),
                crate_type: None,
                build_root: None,
                raw_text: String::new(),
            })
            .collect()
    } else {
        // Parse as BUILD file
        let new_content = fs::read_to_string(new_source)
            .with_context(|| format!("Failed to read new source: {:?}", new_source))?;
        parse_build_file(&new_content)?
    };

    // Parse old BUILD file
    let old_crates = parse_build_file(&old_content)?;

    // Merge based on mode
    let merged_crates = merge_crates(&old_crates, &new_crates, mode)?;

    // Build the output content
    // Preserve any non-rust_crate content from the old file (like subinclude, comments, etc.)
    let mut output_content = String::new();
    
    // Extract header content (everything before first rust_crate)
    if let Some(first_crate_pos) = old_content.find("rust_crate(") {
        output_content.push_str(&old_content[..first_crate_pos]);
    }

    // Add merged crates
    for crate_def in &merged_crates {
        output_content.push_str(&generate_crate_block(crate_def));
        output_content.push('\n');
    }

    // Determine output path
    let output_path = output.unwrap_or(old_source);

    // Create backup if needed
    if !no_backup && output_path == old_source {
        let backup_path = format!("{}.backup", old_source.display());
        fs::write(&backup_path, &old_content)
            .with_context(|| format!("Failed to create backup at {}", backup_path))?;
        eprintln!("Created backup at {}", backup_path);
    }

    // Write output
    fs::write(output_path, &output_content)
        .with_context(|| format!("Failed to write output to {:?}", output_path))?;
    
    eprintln!("Wrote merged BUILD to {:?}", output_path);
    eprintln!("Merged {} crates ({} from old, {} from new)", 
        merged_crates.len(), old_crates.len(), new_crates.len());

    Ok(())
}

fn merge_crates(
    old_crates: &[CrateDefinition],
    new_crates: &[CrateDefinition],
    mode: MergeMode,
) -> Result<Vec<CrateDefinition>> {
    let mut result: Vec<CrateDefinition> = Vec::new();
    
    // Build a map of old crates by crate name (not rule name)
    let mut old_by_crate: HashMap<String, CrateDefinition> = HashMap::new();
    for c in old_crates {
        old_by_crate.insert(c.crate_name.clone(), c.clone());
    }

    // Build a map of new crates by crate name
    let mut new_by_crate: HashMap<String, CrateDefinition> = HashMap::new();
    for c in new_crates {
        new_by_crate.insert(c.crate_name.clone(), c.clone());
    }

    match mode {
        MergeMode::Override => {
            // Start with old crates, override with new ones
            let mut seen: HashSet<String> = HashSet::new();
            
            for new_crate in new_crates {
                result.push(new_crate.clone());
                seen.insert(new_crate.crate_name.clone());
            }
            
            // Add old crates that aren't in new
            for old_crate in old_crates {
                if !seen.contains(&old_crate.crate_name) {
                    result.push(old_crate.clone());
                }
            }
        }

        MergeMode::UpdateOrExpandOnly => {
            // Keep all old crates, update versions only if new is higher (within semver)
            let mut seen: HashSet<String> = HashSet::new();

            for old_crate in old_crates {
                let mut updated = old_crate.clone();
                
                if let Some(new_crate) = new_by_crate.get(&old_crate.crate_name) {
                    // Check if we should update
                    if let (Some(old_ver), Some(new_ver)) = (old_crate.parse_semver(), new_crate.parse_semver()) {
                        // Only update if same major version and new is higher
                        if old_ver.0 == new_ver.0 && new_ver > old_ver {
                            updated.version = new_crate.version.clone();
                        }
                    }
                    
                    // Always expand features (add new ones, never remove)
                    let old_features: HashSet<String> = old_crate.features.iter().cloned().collect();
                    for f in &new_crate.features {
                        if !old_features.contains(f) {
                            updated.features.push(f.clone());
                        }
                    }

                    // Expand deps too
                    let old_deps: HashSet<String> = old_crate.deps.iter().cloned().collect();
                    for d in &new_crate.deps {
                        if !old_deps.contains(d) {
                            updated.deps.push(d.clone());
                        }
                    }
                }
                
                result.push(updated);
                seen.insert(old_crate.crate_name.clone());
            }

            // Add new crates that aren't in old
            for new_crate in new_crates {
                if !seen.contains(&new_crate.crate_name) {
                    result.push(new_crate.clone());
                }
            }
        }

        MergeMode::Parallel => {
            // Keep all old crates, add new conflicting ones with version suffix
            for old_crate in old_crates {
                result.push(old_crate.clone());
            }

            for new_crate in new_crates {
                if old_by_crate.contains_key(&new_crate.crate_name) {
                    // Conflict: add with version suffix
                    let mut suffixed = new_crate.clone();
                    let version_suffix = new_crate.version.replace('.', "_");
                    suffixed.name = format!("{}.{}", new_crate.name, version_suffix);
                    result.push(suffixed);
                } else {
                    result.push(new_crate.clone());
                }
            }
        }
    }

    // Sort by crate name for consistent output
    result.sort_by(|a, b| a.crate_name.cmp(&b.crate_name));

    Ok(result)
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

