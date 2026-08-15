//! Resolve versions and features across the declared crate graph.
//!
//! The rust_repo declarations pin every (crate, version) in third_party —
//! they play the role go.mod plays for go-rules. This command computes the
//! rest of what cargo would: per-dependency version routing (semver matching
//! against the declared set) and unified features (cargo resolver v2
//! semantics), evaluated for one target triple. The output lock file is
//! checked in and consumed by `generate` for each subrepo, keeping builds
//! deterministic with no cargo anywhere.

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
    pub entries: PathBuf,

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
}

#[derive(Serialize, Deserialize)]
pub struct LockFile {
    pub target: String,
    pub crates: BTreeMap<String, LockEntry>,
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

struct CrateNode {
    subrepo: String,
    crate_name: String,
    version: Version,
    manifest: Manifest,
    deps: Vec<DepDecl>,
    requested_features: Vec<String>,
    activated: bool,
    enabled_features: BTreeSet<String>,
    enabled_optional_deps: BTreeSet<String>,
    // weak features (dep?/feat) waiting for the dep to be activated
    deferred: Vec<(String, String)>,
}

pub fn run(args: ResolveArgs) -> Result<()> {
    let entries: Vec<EntryInput> = serde_json::from_str(
        &fs::read_to_string(&args.entries)
            .with_context(|| format!("Failed to read {}", args.entries.display()))?,
    )
    .context("Failed to parse entries JSON")?;

    let lock = resolve_entries(&entries, &args.target)?;
    fs::write(&args.output, serde_json::to_string_pretty(&lock)? + "\n")
        .with_context(|| format!("Failed to write {}", args.output.display()))?;
    eprintln!(
        "please_rust resolve: {} crates resolved for {}",
        lock.crates.len(),
        args.target
    );
    Ok(())
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
        let manifest = Manifest::from_slice(&content)
            .with_context(|| format!("Failed to parse {}", e.manifest.display()))?;
        let version = Version::parse(&e.version)
            .with_context(|| format!("Bad version {} for {}", e.version, e.crate_name))?;
        let deps = collect_deps(&manifest, target_info);
        nodes.push(CrateNode {
            subrepo: e.subrepo.clone(),
            crate_name: e.crate_name.clone(),
            version,
            manifest,
            deps,
            requested_features: e.features.clone(),
            activated: false,
            enabled_features: BTreeSet::new(),
            enabled_optional_deps: BTreeSet::new(),
            deferred: Vec::new(),
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

    // Seed: every entry with requested features (or a root marker) is a root.
    // Requested features seed exactly (the BUILD declaration is the request);
    // roots without features still get activated so their mandatory deps flow.
    let mut work: Vec<Work> = Vec::new();
    for (i, n) in nodes.iter().enumerate() {
        if n.root_like() {
            work.push(Work::ActivateCrate {
                idx: i,
                default_features: false,
                features: n.requested_features.clone(),
            });
        }
    }
    // Entries explicitly marked root but with no features also activate
    for (i, e) in entries.iter().enumerate() {
        if e.root && !nodes[i].root_like() {
            work.push(Work::ActivateCrate {
                idx: i,
                default_features: false,
                features: vec![],
            });
        }
    }

    process(&mut nodes, &resolver, work)?;

    // Emit lock file
    let mut crates = BTreeMap::new();
    for n in &nodes {
        if !n.activated {
            continue;
        }
        let (deps, build_deps) = emit_deps(n, &nodes, &resolver)?;
        crates.insert(
            n.subrepo.clone(),
            LockEntry {
                crate_name: n.crate_name.clone(),
                version: n.version.to_string(),
                features: n.enabled_features.iter().cloned().collect(),
                deps,
                build_deps,
            },
        );
    }

    Ok(LockFile {
        target: target.to_string(),
        crates,
    })
}

impl CrateNode {
    fn root_like(&self) -> bool {
        !self.requested_features.is_empty()
    }
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
        // No semver match: a single declared version is used as-is (matches
        // the previous name-based routing); multiple versions is an error the
        // caller reports.
        if candidates.len() == 1 {
            return Some(candidates[0]);
        }
        None
    }
}

enum Work {
    ActivateCrate {
        idx: usize,
        default_features: bool,
        features: Vec<String>,
    },
    Feature {
        idx: usize,
        feature: String,
    },
}

fn process(nodes: &mut Vec<CrateNode>, resolver: &Resolver, mut work: Vec<Work>) -> Result<()> {
    while let Some(item) = work.pop() {
        match item {
            Work::ActivateCrate {
                idx,
                default_features,
                features,
            } => {
                let newly = !nodes[idx].activated;
                nodes[idx].activated = true;
                if newly {
                    // Mandatory deps always flow
                    let mandatory: Vec<(String, bool, Vec<String>, Option<VersionReq>)> = nodes[idx]
                        .deps
                        .iter()
                        .filter(|d| !d.optional)
                        .map(|d| (d.package.clone(), d.default_features, d.features.clone(), d.req.clone()))
                        .collect();
                    for (package, df, feats, req) in mandatory {
                        if let Some(child) = resolver.select(&package, req.as_ref(), nodes) {
                            work.push(Work::ActivateCrate {
                                idx: child,
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
                        // generate will warn if it's actually needed.
                    }
                }
                if default_features && nodes[idx].manifest.features.contains_key("default") {
                    work.push(Work::Feature {
                        idx,
                        feature: "default".to_string(),
                    });
                }
                for f in features {
                    work.push(Work::Feature { idx, feature: f });
                }
            }
            Work::Feature { idx, feature } => {
                enable_feature(nodes, resolver, idx, &feature, &mut work)?;
            }
        }
    }
    Ok(())
}

fn enable_feature(
    nodes: &mut Vec<CrateNode>,
    resolver: &Resolver,
    idx: usize,
    feature: &str,
    work: &mut Vec<Work>,
) -> Result<()> {
    if nodes[idx].enabled_features.contains(feature) {
        return Ok(());
    }

    // A [features] table entry expands to its constituents
    if let Some(items) = nodes[idx].manifest.features.get(feature).cloned() {
        nodes[idx].enabled_features.insert(feature.to_string());
        for item in items {
            if let Some(dep_name) = item.strip_prefix("dep:") {
                activate_dep(nodes, resolver, idx, dep_name, work)?;
            } else if let Some((dep_name, dep_feat)) = item.split_once("?/") {
                // Weak: only applies if the dep is otherwise activated
                let dep_name = dep_name.to_string();
                let dep_feat = dep_feat.to_string();
                if nodes[idx].enabled_optional_deps.contains(&dep_name)
                    || nodes[idx]
                        .deps
                        .iter()
                        .any(|d| d.name == dep_name && !d.optional)
                {
                    forward_feature(nodes, resolver, idx, &dep_name, &dep_feat, work)?;
                } else {
                    nodes[idx].deferred.push((dep_name, dep_feat));
                }
            } else if let Some((dep_name, dep_feat)) = item.split_once('/') {
                let dep_name = dep_name.to_string();
                let dep_feat = dep_feat.to_string();
                activate_dep(nodes, resolver, idx, &dep_name, work)?;
                forward_feature(nodes, resolver, idx, &dep_name, &dep_feat, work)?;
            } else {
                work.push(Work::Feature {
                    idx,
                    feature: item,
                });
            }
        }
        return Ok(());
    }

    // Not a named feature: an implicit optional-dep feature, or an unknown
    // cfg the crate checks. Both become an enabled feature cfg; the former
    // also activates the dep.
    nodes[idx].enabled_features.insert(feature.to_string());
    let is_optional_dep = nodes[idx]
        .deps
        .iter()
        .any(|d| d.name == feature && d.optional);
    if is_optional_dep {
        activate_dep(nodes, resolver, idx, feature, work)?;
    }
    Ok(())
}

/// Activate an optional (or any) dependency of nodes[idx] by declared name.
fn activate_dep(
    nodes: &mut Vec<CrateNode>,
    resolver: &Resolver,
    idx: usize,
    dep_name: &str,
    work: &mut Vec<Work>,
) -> Result<()> {
    if nodes[idx].enabled_optional_deps.contains(dep_name) {
        return Ok(());
    }
    let decl = match nodes[idx].deps.iter().find(|d| d.name == dep_name) {
        Some(d) => (
            d.package.clone(),
            d.default_features,
            d.features.clone(),
            d.req.clone(),
        ),
        None => return Ok(()), // e.g. a platform-filtered dep
    };
    nodes[idx].enabled_optional_deps.insert(dep_name.to_string());

    let (package, df, feats, req) = decl;
    if let Some(child) = resolver.select(&package, req.as_ref(), nodes) {
        work.push(Work::ActivateCrate {
            idx: child,
            default_features: df,
            features: feats,
        });
    }

    // Flush weak features now that the dep is active
    let deferred: Vec<(String, String)> = std::mem::take(&mut nodes[idx].deferred);
    let (matching, rest): (Vec<_>, Vec<_>) =
        deferred.into_iter().partition(|(d, _)| d == dep_name);
    nodes[idx].deferred = rest;
    for (d, f) in matching {
        forward_feature(nodes, resolver, idx, &d, &f, work)?;
    }
    Ok(())
}

/// Enable a feature on the resolved target of nodes[idx]'s dep `dep_name`.
fn forward_feature(
    nodes: &mut Vec<CrateNode>,
    resolver: &Resolver,
    idx: usize,
    dep_name: &str,
    dep_feat: &str,
    work: &mut Vec<Work>,
) -> Result<()> {
    let decl = match nodes[idx].deps.iter().find(|d| d.name == dep_name) {
        Some(d) => (d.package.clone(), d.req.clone()),
        None => return Ok(()),
    };
    if let Some(child) = resolver.select(&decl.0, decl.1.as_ref(), nodes) {
        work.push(Work::Feature {
            idx: child,
            feature: dep_feat.to_string(),
        });
    }
    Ok(())
}

/// Final dep lists for the lock entry: mandatory deps plus activated optionals.
fn emit_deps(
    n: &CrateNode,
    nodes: &[CrateNode],
    resolver: &Resolver,
) -> Result<(Vec<LockDep>, Vec<LockDep>)> {
    let mut deps = Vec::new();
    let mut build_deps = Vec::new();
    let mut seen: BTreeSet<(String, bool)> = BTreeSet::new();
    for d in &n.deps {
        if d.optional && !n.enabled_optional_deps.contains(&d.name) {
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
        let lock_dep = LockDep {
            name: d.name.clone(),
            crate_name: nodes[child].crate_name.clone(),
            subrepo: nodes[child].subrepo.clone(),
        };
        match d.kind {
            DepKind::Normal => deps.push(lock_dep),
            DepKind::Build => build_deps.push(lock_dep),
        }
    }
    Ok((deps, build_deps))
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
