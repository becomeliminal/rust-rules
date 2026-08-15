//! Resolve versions and features across the declared crate graph.
//!
//! The rust_repo declarations pin every (crate, version) in third_party —
//! they play the role go.mod plays for go-rules. This command computes the
//! rest of what cargo would: per-dependency version routing (semver matching
//! against the declared set) and unified features (cargo resolver v2
//! semantics), evaluated for one target triple.
//!
//! Like cargo, resolution distinguishes two build units: TARGET (code linked
//! into final artifacts) and HOST (proc-macros, build scripts, and their
//! transitive dependencies, which run in the compiler / at build time).
//! Features unify separately per unit; a crate needed by both with different
//! feature sets gets a distinct `<crate>_host` build target.

use anyhow::{bail, Context, Result};
use cargo_toml::Manifest;
use clap::Args;
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::PathBuf;

#[derive(Args)]
pub struct ResolveArgs {
    /// JSON file describing the declared crates:
    /// [{subrepo, crate_name, version, manifest, features, root}]
    #[arg(long)]
    pub entries: Option<PathBuf>,

    /// Inline entry: subrepo|crate|version|feat1,feat2|root|default_features
    /// (manifests are read from --manifest-dir/<subrepo>.manifest.toml).
    /// This is how the generated rust_resolve build rule invokes resolution.
    #[arg(long = "entry")]
    pub inline_entries: Vec<String>,

    /// Directory holding <subrepo>.manifest.toml files for --entry mode
    #[arg(long, default_value = ".")]
    pub manifest_dir: PathBuf,

    /// Target triple to resolve for
    #[arg(long, default_value = "x86_64-unknown-linux-gnu")]
    pub target: String,

    /// Output lock file path
    #[arg(long)]
    pub output: PathBuf,
}

#[derive(Deserialize, Clone)]
pub struct EntryInput {
    pub subrepo: String,
    pub crate_name: String,
    pub version: String,
    pub manifest: PathBuf,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub root: bool,
    /// Cargo semantics for a direct dependency: default features are enabled
    /// unless explicitly opted out.
    #[serde(default = "entry_default_true")]
    pub default_features: bool,
}

fn entry_default_true() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct LockEntry {
    pub crate_name: String,
    pub version: String,
    pub features: Vec<String>,
    pub deps: Vec<LockDep>,
    pub build_deps: Vec<LockDep>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct LockDep {
    /// Name as declared by the dependent (the rename, if any)
    pub name: String,
    pub crate_name: String,
    pub subrepo: String,
    /// Build target inside the subrepo (crate name, or crate_host for the
    /// host variant of a dual-unit crate)
    #[serde(default)]
    pub target_name: String,
}

#[derive(Serialize, Deserialize)]
pub struct LockFile {
    pub target: String,
    /// Primary entry per subrepo (target unit; host unit if host-only)
    pub crates: BTreeMap<String, LockEntry>,
    /// Host-unit variants for crates needed by both units with different
    /// features (built as <crate>_host)
    #[serde(default)]
    pub host_crates: BTreeMap<String, LockEntry>,
}

/// Parse a manifest, normalizing deprecated underscore key spellings that
/// cargo accepts but cargo_toml's kebab-case parsing silently ignores
/// (e.g. tonic 0.11 ships `default_features = false`).
pub fn parse_manifest(bytes: &[u8]) -> Result<Manifest, cargo_toml::Error> {
    let text = String::from_utf8_lossy(bytes)
        .replace("default_features =", "default-features =")
        .replace("dev_dependencies]", "dev-dependencies]")
        .replace("build_dependencies]", "build-dependencies]");
    Manifest::from_slice(text.as_bytes())
}

impl LockFile {
    pub fn load(path: &std::path::Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read lock file {}", path.display()))?;
        serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse lock file {}", path.display()))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DepKind {
    Normal,
    Build,
}

/// Cargo's two build units. Host code (proc-macros, build scripts and their
/// dependencies) runs in the compiler; target code links into artifacts.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Unit {
    Target = 0,
    Host = 1,
}

const UNITS: [Unit; 2] = [Unit::Target, Unit::Host];

/// A dependency declaration extracted from a manifest, pre-filtered by target cfg.
struct DepDecl {
    name: String,    // declared name (may be a rename)
    package: String, // real crate name
    req: Option<VersionReq>,
    optional: bool,
    default_features: bool,
    features: Vec<String>,
    kind: DepKind,
}

#[derive(Default)]
struct UnitState {
    activated: bool,
    enabled_features: BTreeSet<String>,
    enabled_optional_deps: BTreeSet<String>,
    // weak features (dep?/feat) waiting for the dep to be activated
    deferred: Vec<(String, String)>,
}

struct CrateNode {
    subrepo: String,
    crate_name: String,
    version: Version,
    manifest: Manifest,
    deps: Vec<DepDecl>,
    requested_features: Vec<String>,
    is_proc_macro: bool,
    units: [UnitState; 2],
}

impl CrateNode {
    fn unit(&self, u: Unit) -> &UnitState {
        &self.units[u as usize]
    }
    fn unit_mut(&mut self, u: Unit) -> &mut UnitState {
        &mut self.units[u as usize]
    }
    /// Proc-macros only ever build for the host.
    fn normalize_unit(&self, u: Unit) -> Unit {
        if self.is_proc_macro {
            Unit::Host
        } else {
            u
        }
    }
}

pub fn run(args: ResolveArgs) -> Result<()> {
    let entries: Vec<EntryInput> = if let Some(path) = &args.entries {
        serde_json::from_str(
            &fs::read_to_string(path)
                .with_context(|| format!("Failed to read {}", path.display()))?,
        )
        .context("Failed to parse entries JSON")?
    } else {
        args.inline_entries
            .iter()
            .map(|e| parse_inline_entry(e, &args.manifest_dir))
            .collect::<Result<Vec<_>>>()?
    };

    let lock = resolve_entries(&entries, &args.target)?;
    fs::write(&args.output, serde_json::to_string_pretty(&lock)? + "\n")
        .with_context(|| format!("Failed to write {}", args.output.display()))?;
    eprintln!(
        "please_rust resolve: {} crates ({} dual host variants) resolved for {}",
        lock.crates.len(),
        lock.host_crates.len(),
        args.target
    );
    Ok(())
}

fn parse_inline_entry(entry: &str, manifest_dir: &std::path::Path) -> Result<EntryInput> {
    let parts: Vec<&str> = entry.split('|').collect();
    if parts.len() != 6 {
        bail!("bad --entry (want subrepo|crate|version|features|root|default_features): {}", entry);
    }
    Ok(EntryInput {
        subrepo: parts[0].to_string(),
        crate_name: parts[1].to_string(),
        version: parts[2].to_string(),
        manifest: manifest_dir.join(format!("{}.manifest.toml", parts[0])),
        features: if parts[3].is_empty() {
            vec![]
        } else {
            parts[3].split(',').map(|s| s.trim().to_string()).collect()
        },
        root: parts[4] == "true",
        default_features: parts[5] != "false",
    })
}

/// Resolve the declared crate graph; shared by the resolve and sync commands.
pub fn resolve_entries(entries: &[EntryInput], target: &str) -> Result<LockFile> {
    let target_info = cfg_expr::targets::get_builtin_target_by_triple(target)
        .with_context(|| format!("Unknown target triple {}", target))?;

    // Build nodes
    let mut nodes: Vec<CrateNode> = Vec::new();
    for e in entries {
        let content = fs::read(&e.manifest)
            .with_context(|| format!("Failed to read {}", e.manifest.display()))?;
        let manifest = parse_manifest(&content)
            .with_context(|| format!("Failed to parse {}", e.manifest.display()))?;
        let version = Version::parse(&e.version)
            .with_context(|| format!("Bad version {} for {}", e.version, e.crate_name))?;
        let deps = collect_deps(&manifest, target_info);
        let is_proc_macro = manifest
            .lib
            .as_ref()
            .map(|l| l.proc_macro || l.crate_type.contains(&"proc-macro".to_string()))
            .unwrap_or(false);
        nodes.push(CrateNode {
            subrepo: e.subrepo.clone(),
            crate_name: e.crate_name.clone(),
            version,
            manifest,
            deps,
            requested_features: e.features.clone(),
            is_proc_macro,
            units: [UnitState::default(), UnitState::default()],
        });
    }

    // Index: crate name -> node indices, highest version first
    let mut index: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, n) in nodes.iter().enumerate() {
        index.entry(n.crate_name.clone()).or_default().push(i);
    }
    for v in index.values_mut() {
        v.sort_by(|a, b| nodes[*b].version.cmp(&nodes[*a].version));
    }

    let resolver = Resolver { index };

    // Seed roots with cargo's direct-dependency semantics: the requested
    // feature list plus default features, unless the entry opts out with
    // default_features = False. Indirect entries never seed — their features
    // are derived from their dependents.
    let mut work: Vec<Work> = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        if e.root {
            let unit = nodes[i].normalize_unit(Unit::Target);
            work.push(Work::ActivateCrate {
                idx: i,
                unit,
                default_features: e.default_features,
                features: nodes[i].requested_features.clone(),
            });
        }
    }

    process(&mut nodes, &resolver, work)?;

    // Emit the lock. Per subrepo:
    // - active in target unit only -> crates
    // - active in host unit only (proc-macros, pure build deps) -> crates
    // - active in both with identical features -> crates (shared artifact;
    //   valid while host triple == target triple)
    // - active in both with different features -> crates + host_crates
    let mut dual: BTreeSet<usize> = BTreeSet::new();
    for (i, n) in nodes.iter().enumerate() {
        if n.unit(Unit::Target).activated
            && n.unit(Unit::Host).activated
            && n.unit(Unit::Target).enabled_features != n.unit(Unit::Host).enabled_features
        {
            dual.insert(i);
        }
    }

    let mut crates = BTreeMap::new();
    let mut host_crates = BTreeMap::new();
    for (i, n) in nodes.iter().enumerate() {
        let target_active = n.unit(Unit::Target).activated;
        let host_active = n.unit(Unit::Host).activated;
        if !target_active && !host_active {
            continue;
        }
        let primary_unit = if target_active { Unit::Target } else { Unit::Host };
        crates.insert(
            n.subrepo.clone(),
            emit_entry(n, primary_unit, &nodes, &resolver, &dual)?,
        );
        if dual.contains(&i) {
            host_crates.insert(
                n.subrepo.clone(),
                emit_entry(n, Unit::Host, &nodes, &resolver, &dual)?,
            );
        }
    }

    Ok(LockFile {
        target: target.to_string(),
        crates,
        host_crates,
    })
}

struct Resolver {
    index: HashMap<String, Vec<usize>>,
}

impl Resolver {
    /// Pick the declared node satisfying a dependency requirement.
    fn select(&self, package: &str, req: Option<&VersionReq>, nodes: &[CrateNode]) -> Option<usize> {
        let candidates = self.index.get(package)?;
        if let Some(req) = req {
            for &i in candidates {
                if req.matches(&nodes[i].version) {
                    return Some(i);
                }
            }
        }
        // No semver match: a single declared version is used as-is; multiple
        // versions with no match is an error the caller reports.
        if candidates.len() == 1 {
            return Some(candidates[0]);
        }
        None
    }
}

enum Work {
    ActivateCrate {
        idx: usize,
        unit: Unit,
        default_features: bool,
        features: Vec<String>,
    },
    Feature {
        idx: usize,
        unit: Unit,
        feature: String,
    },
}

/// The unit a dependency edge lands in: build-deps always go to host; a
/// host-unit crate's normal deps stay host; proc-macro children are host.
fn edge_unit(parent_unit: Unit, kind: DepKind) -> Unit {
    match kind {
        DepKind::Build => Unit::Host,
        DepKind::Normal => parent_unit,
    }
}

fn process(nodes: &mut Vec<CrateNode>, resolver: &Resolver, mut work: Vec<Work>) -> Result<()> {
    while let Some(item) = work.pop() {
        match item {
            Work::ActivateCrate {
                idx,
                unit,
                default_features,
                features,
            } => {
                let unit = nodes[idx].normalize_unit(unit);
                let newly = !nodes[idx].unit(unit).activated;
                nodes[idx].unit_mut(unit).activated = true;
                if newly {
                    // Mandatory deps always flow
                    let mandatory: Vec<(String, bool, Vec<String>, Option<VersionReq>, DepKind)> =
                        nodes[idx]
                            .deps
                            .iter()
                            .filter(|d| !d.optional)
                            .map(|d| {
                                (
                                    d.package.clone(),
                                    d.default_features,
                                    d.features.clone(),
                                    d.req.clone(),
                                    d.kind,
                                )
                            })
                            .collect();
                    for (package, df, feats, req, kind) in mandatory {
                        if let Some(child) = resolver.select(&package, req.as_ref(), nodes) {
                            work.push(Work::ActivateCrate {
                                idx: child,
                                unit: edge_unit(unit, kind),
                                default_features: df,
                                features: feats,
                            });
                        } else if resolver.index.contains_key(&package) {
                            bail!(
                                "{}: no declared version of {} satisfies {:?}",
                                nodes[idx].crate_name,
                                package,
                                req.map(|r| r.to_string())
                            );
                        }
                        // Unknown package: not declared (e.g. platform-only);
                        // emit warns if it's actually needed.
                    }
                }
                if default_features && nodes[idx].manifest.features.contains_key("default") {
                    work.push(Work::Feature {
                        idx,
                        unit,
                        feature: "default".to_string(),
                    });
                }
                for f in features {
                    work.push(Work::Feature {
                        idx,
                        unit,
                        feature: f,
                    });
                }
            }
            Work::Feature { idx, unit, feature } => {
                let unit = nodes[idx].normalize_unit(unit);
                enable_feature(nodes, resolver, idx, unit, &feature, &mut work)?;
            }
        }
    }
    Ok(())
}

fn enable_feature(
    nodes: &mut Vec<CrateNode>,
    resolver: &Resolver,
    idx: usize,
    unit: Unit,
    feature: &str,
    work: &mut Vec<Work>,
) -> Result<()> {
    if nodes[idx].unit(unit).enabled_features.contains(feature) {
        return Ok(());
    }

    // A [features] table entry expands to its constituents
    if let Some(items) = nodes[idx].manifest.features.get(feature).cloned() {
        nodes[idx]
            .unit_mut(unit)
            .enabled_features
            .insert(feature.to_string());
        for item in items {
            if let Some(dep_name) = item.strip_prefix("dep:") {
                activate_dep(nodes, resolver, idx, unit, dep_name, work)?;
            } else if let Some((dep_name, dep_feat)) = item.split_once("?/") {
                // Weak: only applies if the dep is otherwise activated
                let dep_name = dep_name.to_string();
                let dep_feat = dep_feat.to_string();
                if nodes[idx].unit(unit).enabled_optional_deps.contains(&dep_name)
                    || nodes[idx]
                        .deps
                        .iter()
                        .any(|d| d.name == dep_name && !d.optional)
                {
                    forward_feature(nodes, resolver, idx, unit, &dep_name, &dep_feat, work)?;
                } else {
                    nodes[idx].unit_mut(unit).deferred.push((dep_name, dep_feat));
                }
            } else if let Some((dep_name, dep_feat)) = item.split_once('/') {
                let dep_name = dep_name.to_string();
                let dep_feat = dep_feat.to_string();
                activate_dep(nodes, resolver, idx, unit, &dep_name, work)?;
                forward_feature(nodes, resolver, idx, unit, &dep_name, &dep_feat, work)?;
            } else {
                work.push(Work::Feature {
                    idx,
                    unit,
                    feature: item,
                });
            }
        }
        return Ok(());
    }

    // Not a named feature: an implicit optional-dep feature, or an unknown
    // cfg the crate checks. Cargo's namespaced-features rule: once any
    // feature uses dep:x, the implicit feature x no longer exists — the dep
    // can be activated but no feature cfg is set for it.
    let is_optional_dep = nodes[idx]
        .deps
        .iter()
        .any(|d| d.name == feature && d.optional);
    let namespaced = is_optional_dep
        && nodes[idx]
            .manifest
            .features
            .values()
            .flatten()
            .any(|item| item.strip_prefix("dep:") == Some(feature));
    if !namespaced {
        nodes[idx]
            .unit_mut(unit)
            .enabled_features
            .insert(feature.to_string());
    }
    if is_optional_dep {
        activate_dep(nodes, resolver, idx, unit, feature, work)?;
    }
    Ok(())
}

/// Activate an optional (or any) dependency of nodes[idx] by declared name.
fn activate_dep(
    nodes: &mut Vec<CrateNode>,
    resolver: &Resolver,
    idx: usize,
    unit: Unit,
    dep_name: &str,
    work: &mut Vec<Work>,
) -> Result<()> {
    if nodes[idx].unit(unit).enabled_optional_deps.contains(dep_name) {
        return Ok(());
    }
    let decl = match nodes[idx].deps.iter().find(|d| d.name == dep_name) {
        Some(d) => (
            d.package.clone(),
            d.default_features,
            d.features.clone(),
            d.req.clone(),
            d.kind,
        ),
        None => return Ok(()), // e.g. a platform-filtered dep
    };
    nodes[idx]
        .unit_mut(unit)
        .enabled_optional_deps
        .insert(dep_name.to_string());

    let (package, df, feats, req, kind) = decl;
    if let Some(child) = resolver.select(&package, req.as_ref(), nodes) {
        work.push(Work::ActivateCrate {
            idx: child,
            unit: edge_unit(unit, kind),
            default_features: df,
            features: feats,
        });
    }

    // Flush weak features now that the dep is active
    let deferred: Vec<(String, String)> = std::mem::take(&mut nodes[idx].unit_mut(unit).deferred);
    let (matching, rest): (Vec<_>, Vec<_>) =
        deferred.into_iter().partition(|(d, _)| d == dep_name);
    nodes[idx].unit_mut(unit).deferred = rest;
    for (d, f) in matching {
        forward_feature(nodes, resolver, idx, unit, &d, &f, work)?;
    }
    Ok(())
}

/// Enable a feature on the resolved target of nodes[idx]'s dep `dep_name`.
fn forward_feature(
    nodes: &mut Vec<CrateNode>,
    resolver: &Resolver,
    idx: usize,
    unit: Unit,
    dep_name: &str,
    dep_feat: &str,
    work: &mut Vec<Work>,
) -> Result<()> {
    let decl = match nodes[idx].deps.iter().find(|d| d.name == dep_name) {
        Some(d) => (d.package.clone(), d.req.clone(), d.kind),
        None => return Ok(()),
    };
    if let Some(child) = resolver.select(&decl.0, decl.1.as_ref(), nodes) {
        work.push(Work::Feature {
            idx: child,
            unit: edge_unit(unit, decl.2),
            feature: dep_feat.to_string(),
        });
    }
    Ok(())
}

/// Build the lock entry for one (crate, unit): mandatory deps plus activated
/// optionals, each routed to its subrepo and unit-appropriate build target.
fn emit_entry(
    n: &CrateNode,
    unit: Unit,
    nodes: &[CrateNode],
    resolver: &Resolver,
    dual: &BTreeSet<usize>,
) -> Result<LockEntry> {
    let mut deps = Vec::new();
    let mut build_deps = Vec::new();
    let mut seen: BTreeSet<(String, bool)> = BTreeSet::new();
    for d in &n.deps {
        if d.optional && !n.unit(unit).enabled_optional_deps.contains(&d.name) {
            continue;
        }
        let child = match resolver.select(&d.package, d.req.as_ref(), nodes) {
            Some(c) => c,
            None => {
                if resolver.index.contains_key(&d.package) {
                    bail!(
                        "{}: no declared version of {} satisfies {:?}",
                        n.crate_name,
                        d.package,
                        d.req.as_ref().map(|r| r.to_string())
                    );
                }
                eprintln!(
                    "warning: {}: dependency {} is not declared, skipping",
                    n.crate_name, d.package
                );
                continue;
            }
        };
        if !seen.insert((d.name.clone(), d.kind == DepKind::Build)) {
            continue;
        }
        let child_unit = nodes[child].normalize_unit(edge_unit(unit, d.kind));
        let crate_norm = nodes[child].crate_name.replace('-', "_");
        // Route to the host variant only when the child is genuinely dual and
        // this edge is a host edge
        let target_name = if child_unit == Unit::Host && dual.contains(&child) {
            format!("{}_host", crate_norm)
        } else {
            crate_norm
        };
        let lock_dep = LockDep {
            name: d.name.clone(),
            crate_name: nodes[child].crate_name.clone(),
            subrepo: nodes[child].subrepo.clone(),
            target_name,
        };
        match d.kind {
            DepKind::Normal => deps.push(lock_dep),
            DepKind::Build => build_deps.push(lock_dep),
        }
    }
    Ok(LockEntry {
        crate_name: n.crate_name.clone(),
        version: n.version.to_string(),
        features: n.unit(unit).enabled_features.iter().cloned().collect(),
        deps,
        build_deps,
    })
}

/// Extract dependency declarations, filtering platform-specific tables by cfg.
fn collect_deps(manifest: &Manifest, target_info: &cfg_expr::targets::TargetInfo) -> Vec<DepDecl> {
    let mut out = Vec::new();
    let mut add = |name: &str, dep: &cargo_toml::Dependency, kind: DepKind| {
        let package = dep.package().unwrap_or(name).to_string();
        if package.starts_with("rustc-std-workspace") {
            return;
        }
        let req_str = dep.req();
        let req = VersionReq::parse(req_str).ok();
        let (optional, default_features, features) = match dep.detail() {
            Some(d) => (d.optional, d.default_features, d.features.clone()),
            None => (false, true, vec![]),
        };
        out.push(DepDecl {
            name: name.to_string(),
            package,
            req,
            optional,
            default_features,
            features,
            kind,
        });
    };

    for (name, dep) in &manifest.dependencies {
        add(name, dep, DepKind::Normal);
    }
    for (name, dep) in &manifest.build_dependencies {
        add(name, dep, DepKind::Build);
    }
    for (cfg, tdeps) in &manifest.target {
        if !cfg_applies(cfg, target_info) {
            continue;
        }
        for (name, dep) in &tdeps.dependencies {
            add(name, dep, DepKind::Normal);
        }
        for (name, dep) in &tdeps.build_dependencies {
            add(name, dep, DepKind::Build);
        }
    }
    out
}

fn cfg_applies(cfg: &str, target_info: &cfg_expr::targets::TargetInfo) -> bool {
    if cfg.starts_with("cfg(") {
        match cfg_expr::Expression::parse(cfg) {
            Ok(expr) => expr.eval(|pred| match pred {
                cfg_expr::expr::Predicate::Target(tp) => tp.matches(target_info),
                // Feature predicates in [target] cfgs are rare and ill-defined;
                // cargo treats them as false.
                _ => false,
            }),
            Err(_) => false,
        }
    } else {
        // A literal target triple
        cfg == target_info.triple.as_str()
    }
}
