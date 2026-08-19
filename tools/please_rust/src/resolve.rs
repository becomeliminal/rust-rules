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
    #[arg(long, default_value_t = crate::build_script::running_triple())]
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

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct LockEntry {
    pub crate_name: String,
    pub version: String,
    pub features: Vec<String>,
    pub deps: Vec<LockDep>,
    pub build_deps: Vec<LockDep>,
    /// The manifest's links key; such crates export build-script metadata
    /// to direct dependents as DEP_<LINKS>_<KEY> env vars
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub links: Option<String>,
    /// This entry is a host unit: a proc macro, or a crate reached only as a
    /// build dependency. It runs on the machine doing the building, so it is
    /// compiled for the host triple however the rest of the graph is
    /// targeted. Absent means the target unit, which is almost everything.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub host: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
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

#[derive(Serialize, Deserialize, Debug)]
pub struct LockFile {
    pub target: String,
    /// Primary entry per subrepo (target unit; host unit if host-only)
    pub crates: BTreeMap<String, LockEntry>,
    /// Host-unit variants for crates needed by both units with different
    /// features (built as <crate>_host)
    #[serde(default)]
    pub host_crates: BTreeMap<String, LockEntry>,
    /// Dependencies resolution needed but which are not declared. Empty in a
    /// healthy graph; `lock` uses these to add what a newly enabled feature
    /// turned on.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<MissingDep>,
}

/// Parse a manifest, normalizing deprecated underscore key spellings that
/// cargo accepts but cargo_toml's kebab-case parsing silently ignores
/// (e.g. tonic 0.11 ships `default_features = false`).
pub fn parse_manifest(bytes: &[u8]) -> Result<Manifest, cargo_toml::Error> {
    let text = String::from_utf8_lossy(bytes)
        .replace("default_features =", "default-features =")
        .replace("dev_dependencies]", "dev-dependencies]")
        .replace("build_dependencies]", "build-dependencies]")
        // cargo_toml 0.20 only knows resolver versions 1 and 2; version 3
        // (cargo >=1.84) is v2 unification plus MSRV-aware selection, which
        // we implement ourselves, so parse it as 2.
        .replace("resolver = \"3\"", "resolver = \"2\"");
    Manifest::from_slice(text.as_bytes())
}

/// A dependency resolution needed but which is not declared. Lock uses these
/// to heal the declaration set automatically.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MissingDep {
    /// Crate that wanted it
    pub requirer: String,
    /// Real package name
    pub package: String,
    /// Requirement string, if the manifest gave one
    pub req: Option<String>,
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
        bail!(
            "bad --entry (want subrepo|crate|version|features|root|default_features): {}",
            entry
        );
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
    let mut missing: Vec<MissingDep> = Vec::new();
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

    process(&mut nodes, &resolver, work, &mut missing)?;

    // Emit the lock. Per subrepo:
    // - active in target unit only -> crates
    // - active in host unit only (proc-macros, pure build deps) -> crates
    // - active in both with identical features -> crates (shared artifact;
    //   valid while host triple == target triple)
    // - active in both with different features -> crates + host_crates
    //
    // Cross-compiling, that third case stops holding: identical features do
    // not make a darwin rlib usable by a build script running on linux. So
    // when the triples differ, anything reached both ways is dual on those
    // grounds alone. Resolve runs on the host, so its own triple is the host
    // triple by definition and native builds are unaffected.
    let cross = target != crate::build_script::running_triple();
    let mut dual: BTreeSet<usize> = BTreeSet::new();
    for (i, n) in nodes.iter().enumerate() {
        if n.unit(Unit::Target).activated
            && n.unit(Unit::Host).activated
            && (cross
                || n.unit(Unit::Target).enabled_features != n.unit(Unit::Host).enabled_features)
        {
            dual.insert(i);
        }
    }

    // Duality is contagious. Sharing one artifact between units is only sound
    // if everything it links is also shared: quote's features are identical in
    // both units, but proc-macro2's are not, so a single quote would embed the
    // target proc-macro2 while syn's host unit embeds the host one - and two
    // proc_macro2::TokenStream types that are not the same type is exactly
    // what rustc then complains about. Anything reaching a dual crate is
    // itself dual.
    loop {
        let mut added = false;
        for i in 0..nodes.len() {
            if dual.contains(&i)
                || !nodes[i].unit(Unit::Target).activated
                || !nodes[i].unit(Unit::Host).activated
            {
                continue;
            }
            let reaches_dual = nodes[i].deps.iter().any(|d| {
                if d.optional
                    && !nodes[i]
                        .unit(Unit::Host)
                        .enabled_optional_deps
                        .contains(&d.name)
                {
                    return false;
                }
                resolver
                    .select(&d.package, d.req.as_ref(), &nodes)
                    .is_some_and(|c| dual.contains(&c))
            });
            if reaches_dual {
                dual.insert(i);
                added = true;
            }
        }
        if !added {
            break;
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
        let primary_unit = if target_active {
            Unit::Target
        } else {
            Unit::Host
        };
        let mut entry = emit_entry(n, primary_unit, &nodes, &resolver, &dual, &mut missing)?;
        // Nothing else records this: a pure build dependency is emitted under
        // its own name with no _host twin, so without the flag generation has
        // no way to know it must not be built for the target.
        entry.host = primary_unit == Unit::Host;
        crates.insert(n.subrepo.clone(), entry);
        if dual.contains(&i) {
            host_crates.insert(
                n.subrepo.clone(),
                emit_entry(n, Unit::Host, &nodes, &resolver, &dual, &mut missing)?,
            );
        }
    }

    missing.sort();
    missing.dedup();
    for m in &missing {
        eprintln!(
            "warning: {}: dependency {} is not declared, skipping",
            m.requirer, m.package
        );
    }

    Ok(LockFile {
        target: target.to_string(),
        crates,
        host_crates,
        missing,
    })
}

struct Resolver {
    index: HashMap<String, Vec<usize>>,
}

impl Resolver {
    /// Pick the declared node satisfying a dependency requirement.
    ///
    /// A requirement that nothing declared satisfies returns None rather than
    /// falling back to whatever single version happens to be declared: routing
    /// a `^0.2` dependency onto a declared 0.4 produces a baffling compile
    /// error deep in the crate, where returning None reports it as missing and
    /// lets `lock` declare the version that is actually needed.
    fn select(
        &self,
        package: &str,
        req: Option<&VersionReq>,
        nodes: &[CrateNode],
    ) -> Option<usize> {
        let candidates = self.index.get(package)?;
        if let Some(req) = req {
            for &i in candidates {
                if req.matches(&nodes[i].version) {
                    return Some(i);
                }
            }
            return None;
        }
        // No requirement given (a bare declaration): a single declared version
        // is unambiguous.
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

fn process(
    nodes: &mut [CrateNode],
    resolver: &Resolver,
    mut work: Vec<Work>,
    missing: &mut Vec<MissingDep>,
) -> Result<()> {
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
                        } else {
                            // Either undeclared, or declared only at versions
                            // this requirement rules out. Both are gaps `lock`
                            // can close by declaring the right version, so
                            // record rather than fail.
                            missing.push(MissingDep {
                                requirer: nodes[idx].crate_name.clone(),
                                package: package.clone(),
                                req: req.map(|r| r.to_string()),
                            });
                        }
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
    nodes: &mut [CrateNode],
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
                if nodes[idx]
                    .unit(unit)
                    .enabled_optional_deps
                    .contains(&dep_name)
                    || nodes[idx]
                        .deps
                        .iter()
                        .any(|d| d.name == dep_name && !d.optional)
                {
                    forward_feature(nodes, resolver, idx, unit, &dep_name, &dep_feat, work)?;
                } else {
                    nodes[idx]
                        .unit_mut(unit)
                        .deferred
                        .push((dep_name, dep_feat));
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
    nodes: &mut [CrateNode],
    resolver: &Resolver,
    idx: usize,
    unit: Unit,
    dep_name: &str,
    work: &mut Vec<Work>,
) -> Result<()> {
    if nodes[idx]
        .unit(unit)
        .enabled_optional_deps
        .contains(dep_name)
    {
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

    // Activating an optional dependency also enables the feature of the same
    // name. Two shapes of that: the implicit feature every optional dep gets,
    // which dep: syntax removes; and an explicit feature the crate declares
    // itself, which dep: syntax does not remove and which has items of its
    // own to run. opentelemetry-http declares `reqwest = ["dep:reqwest"]` and
    // reaches the dep through `reqwest/blocking`, so it needs the second.
    // sec1 gates half its API on the first.
    let is_optional = nodes[idx]
        .deps
        .iter()
        .any(|d| d.name == dep_name && d.optional);
    let explicit = nodes[idx].manifest.features.contains_key(dep_name);
    let namespaced = nodes[idx]
        .manifest
        .features
        .values()
        .flatten()
        .any(|item| item.strip_prefix("dep:") == Some(dep_name));
    if is_optional && (explicit || !namespaced) {
        work.push(Work::Feature {
            idx,
            unit,
            feature: dep_name.to_string(),
        });
    }

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
    let (matching, rest): (Vec<_>, Vec<_>) = deferred.into_iter().partition(|(d, _)| d == dep_name);
    nodes[idx].unit_mut(unit).deferred = rest;
    for (d, f) in matching {
        forward_feature(nodes, resolver, idx, unit, &d, &f, work)?;
    }
    Ok(())
}

/// Enable a feature on the resolved target of nodes[idx]'s dep `dep_name`.
fn forward_feature(
    nodes: &mut [CrateNode],
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
    missing: &mut Vec<MissingDep>,
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
                missing.push(MissingDep {
                    requirer: n.crate_name.clone(),
                    package: d.package.clone(),
                    req: d.req.as_ref().map(|r| r.to_string()),
                });
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
        links: n.manifest.package.as_ref().and_then(|p| p.links.clone()),
        // Set by the caller, which is what knows whether this is the entry's
        // primary unit or its _host twin.
        host: false,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// Writes manifests to a scratch dir and builds EntryInputs for them.
    struct Graph {
        dir: PathBuf,
        entries: Vec<EntryInput>,
    }

    impl Graph {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "please_rust_resolve_test_{}_{}",
                std::process::id(),
                name
            ));
            fs::create_dir_all(&dir).unwrap();
            Graph {
                dir,
                entries: Vec::new(),
            }
        }

        fn krate(
            &mut self,
            subrepo: &str,
            name: &str,
            version: &str,
            manifest_body: &str,
        ) -> &mut Self {
            let manifest = format!(
                "[package]\nname = \"{}\"\nversion = \"{}\"\nedition = \"2021\"\n{}",
                name, version, manifest_body
            );
            let path = self.dir.join(format!("{}.toml", subrepo));
            fs::write(&path, manifest).unwrap();
            self.entries.push(EntryInput {
                subrepo: subrepo.to_string(),
                crate_name: name.to_string(),
                version: version.to_string(),
                manifest: path,
                features: vec![],
                root: false,
                default_features: true,
            });
            self
        }

        fn root(&mut self, subrepo: &str, features: &[&str], default_features: bool) -> &mut Self {
            let e = self
                .entries
                .iter_mut()
                .find(|e| e.subrepo == subrepo)
                .unwrap();
            e.root = true;
            e.features = features.iter().map(|s| s.to_string()).collect();
            e.default_features = default_features;
            self
        }

        fn resolve(&self) -> LockFile {
            resolve_entries(&self.entries, &crate::build_script::running_triple()).unwrap()
        }

        fn resolve_for(&self, target: &str) -> LockFile {
            resolve_entries(&self.entries, target).unwrap()
        }
    }

    /// A triple this machine is not, so `resolve_for` genuinely cross-
    /// compiles wherever the suite runs. Hardcoding one would silently stop
    /// testing anything on the platform it names.
    fn foreign_triple() -> &'static str {
        if crate::build_script::running_triple().contains("apple") {
            "x86_64-unknown-linux-gnu"
        } else {
            "aarch64-apple-darwin"
        }
    }

    /// A build script runs on the machine doing the building, so a crate
    /// reached only as a build dependency is a host unit and must be compiled
    /// for the host triple however the rest of the graph is targeted. Nothing
    /// else records that: such a crate is emitted under its own name with no
    /// _host twin, so without the flag generation has no way to know, and
    /// cross-compiling produced a build script whose own dependency was built
    /// for the wrong platform - rustc reports E0461.
    #[test]
    fn a_build_only_dependency_is_a_host_unit() {
        let mut g = Graph::new("build_only_host");
        g.krate(
            "app",
            "app",
            "1.0.0",
            "[build-dependencies]\nhelper = \"1\"\n",
        )
        .krate("helper", "helper", "1.0.0", "")
        .root("app", &[], true);
        let lock = g.resolve();
        assert!(
            lock.crates["helper"].host,
            "a pure build dep is a host unit"
        );
        assert!(!lock.crates["app"].host, "the crate being built is not");
    }

    /// Sharing one artifact between the host and target units is sound only
    /// while the two triples are the same. Identical features do not make a
    /// darwin rlib usable by a build script running on linux, so cross-
    /// compiling has to split what a native build may share.
    #[test]
    fn cross_compiling_splits_an_artifact_a_native_build_shares() {
        let mut g = Graph::new("cross_split");
        g.krate(
            "app",
            "app",
            "1.0.0",
            "[dependencies]\nshared = \"1\"\n\n[build-dependencies]\nshared = \"1\"\n",
        )
        .krate("shared", "shared", "1.0.0", "")
        .root("app", &[], true);

        // Same features in both units, so natively one artifact serves both.
        let native = g.resolve();
        assert!(
            !native.host_crates.contains_key("shared"),
            "native build should share the artifact, got {:?}",
            native.host_crates.keys().collect::<Vec<_>>()
        );

        let cross = g.resolve_for(foreign_triple());
        assert!(
            cross.host_crates.contains_key("shared"),
            "cross-compiling must emit a host unit of its own"
        );
    }

    fn features(lock: &LockFile, subrepo: &str) -> Vec<String> {
        lock.crates.get(subrepo).unwrap().features.clone()
    }

    fn dep_names(lock: &LockFile, subrepo: &str) -> Vec<String> {
        lock.crates
            .get(subrepo)
            .unwrap()
            .deps
            .iter()
            .map(|d| d.name.clone())
            .collect()
    }

    #[test]
    fn mandatory_deps_activate() {
        let mut g = Graph::new("mandatory");
        g.krate("a", "a", "1.0.0", "[dependencies]\nb = \"1\"\n")
            .krate("b", "b", "1.2.0", "")
            .root("a", &[], true);
        let lock = g.resolve();
        assert!(lock.crates.contains_key("a"));
        assert!(lock.crates.contains_key("b"));
        assert_eq!(dep_names(&lock, "a"), vec!["b"]);
    }

    /// criterion depends on plotters with default-features = false and three
    /// named features. Enabling plotters' defaults from that edge pulls its
    /// font backend, then font-kit, then a system fontconfig - a chain
    /// nothing asked for.
    #[test]
    fn an_optional_dep_edge_can_disable_defaults() {
        let mut g = Graph::new("optional_no_defaults");
        g.krate(
            "a", "a", "1.0.0",
            "[dependencies.b]\nversion = \"1\"\noptional = true\ndefault-features = false\nfeatures = [\"svg\"]\n\n\
             [features]\ndefault = [\"with_b\"]\nwith_b = [\"dep:b\"]\n",
        )
        .krate("b", "b", "1.0.0", "[features]\ndefault = [\"ttf\"]\nttf = []\nsvg = []\n")
        .root("a", &[], true);
        let lock = g.resolve();
        let feats = features(&lock, "b");
        assert!(feats.contains(&"svg".to_string()), "{:?}", feats);
        assert!(!feats.contains(&"default".to_string()), "{:?}", feats);
        assert!(!feats.contains(&"ttf".to_string()), "{:?}", feats);
    }

    #[test]
    fn non_root_does_not_activate() {
        let mut g = Graph::new("nonroot");
        g.krate("a", "a", "1.0.0", "").krate("b", "b", "1.0.0", "");
        g.root("a", &[], true);
        let lock = g.resolve();
        assert!(lock.crates.contains_key("a"));
        assert!(!lock.crates.contains_key("b"));
    }

    #[test]
    fn default_features_expand() {
        let mut g = Graph::new("defaults");
        g.krate(
            "a",
            "a",
            "1.0.0",
            "[features]\ndefault = [\"std\"]\nstd = []\n",
        )
        .root("a", &[], true);
        let lock = g.resolve();
        let f = features(&lock, "a");
        assert!(f.contains(&"default".to_string()));
        assert!(f.contains(&"std".to_string()));
    }

    #[test]
    fn default_features_opt_out() {
        let mut g = Graph::new("no_defaults");
        g.krate(
            "a",
            "a",
            "1.0.0",
            "[features]\ndefault = [\"std\"]\nstd = []\nextra = []\n",
        )
        .root("a", &["extra"], false);
        let lock = g.resolve();
        let f = features(&lock, "a");
        assert!(!f.contains(&"std".to_string()));
        assert!(f.contains(&"extra".to_string()));
    }

    #[test]
    fn edge_default_features_flow_to_deps() {
        let mut g = Graph::new("edge_defaults");
        g.krate("a", "a", "1.0.0", "[dependencies]\nb = \"1\"\n")
            .krate(
                "b",
                "b",
                "1.0.0",
                "[features]\ndefault = [\"fast\"]\nfast = []\n",
            )
            .root("a", &[], true);
        let lock = g.resolve();
        assert!(features(&lock, "b").contains(&"fast".to_string()));
    }

    #[test]
    fn edge_no_default_features() {
        let mut g = Graph::new("edge_no_defaults");
        g.krate(
            "a",
            "a",
            "1.0.0",
            "[dependencies.b]\nversion = \"1\"\ndefault-features = false\n",
        )
        .krate(
            "b",
            "b",
            "1.0.0",
            "[features]\ndefault = [\"fast\"]\nfast = []\n",
        )
        .root("a", &[], true);
        let lock = g.resolve();
        assert!(!features(&lock, "b").contains(&"fast".to_string()));
    }

    #[test]
    fn optional_dep_not_activated_without_feature() {
        let mut g = Graph::new("optional_off");
        g.krate(
            "a",
            "a",
            "1.0.0",
            "[dependencies.b]\nversion = \"1\"\noptional = true\n",
        )
        .krate("b", "b", "1.0.0", "")
        .root("a", &[], true);
        let lock = g.resolve();
        assert!(!lock.crates.contains_key("b"));
        assert!(dep_names(&lock, "a").is_empty());
    }

    #[test]
    fn dep_colon_activates_optional() {
        let mut g = Graph::new("dep_colon");
        g.krate(
            "a", "a", "1.0.0",
            "[dependencies.b]\nversion = \"1\"\noptional = true\n\n[features]\nwith_b = [\"dep:b\"]\n",
        )
        .krate("b", "b", "1.0.0", "")
        .root("a", &["with_b"], true);
        let lock = g.resolve();
        assert!(lock.crates.contains_key("b"));
        assert_eq!(dep_names(&lock, "a"), vec!["b"]);
        // Namespaced: dep:b means no implicit feature cfg "b"
        assert!(!features(&lock, "a").contains(&"b".to_string()));
        assert!(features(&lock, "a").contains(&"with_b".to_string()));
    }

    /// `dep:` removes the *implicit* feature of an optional dependency, but
    /// not an explicit one the crate declares itself - and that explicit
    /// feature has items to run. opentelemetry-http declares
    /// `reqwest = ["dep:reqwest"]` and reaches the dep through
    /// `reqwest/blocking`; cargo enables `reqwest` there and so must we.
    #[test]
    fn an_explicit_feature_named_after_the_dep_still_runs() {
        let mut g = Graph::new("explicit_dep_feature");
        g.krate(
            "a",
            "a",
            "1.0.0",
            "[dependencies.b]\nversion = \"1\"\noptional = true\n\n\
             [features]\nb = [\"dep:b\"]\nb_blocking = [\"dep:b\", \"b/inner\"]\n",
        )
        .krate("b", "b", "1.0.0", "[features]\ninner = []\n")
        .root("a", &["b_blocking"], true);
        let lock = g.resolve();
        let feats = features(&lock, "a");
        assert!(feats.contains(&"b".to_string()), "{:?}", feats);
        assert!(feats.contains(&"b_blocking".to_string()), "{:?}", feats);
        assert!(features(&lock, "b").contains(&"inner".to_string()));
    }

    /// Reaching an optional dependency through `b/feat` activates it, and
    /// activating an optional dependency also sets the implicit feature of
    /// the same name. sec1 gates half its API on exactly that, so linking
    /// pkcs8 without setting feature = "pkcs8" produced a crate whose own
    /// modules did not exist.
    #[test]
    fn slash_feature_sets_the_implicit_dep_feature() {
        let mut g = Graph::new("slash_implicit");
        g.krate(
            "a", "a", "1.0.0",
            "[dependencies.b]\nversion = \"1\"\noptional = true\n\n[features]\nuse_b = [\"b/inner\"]\n",
        )
        .krate("b", "b", "1.0.0", "[features]\ninner = []\n")
        .root("a", &["use_b"], true);
        let lock = g.resolve();
        assert!(lock.crates.contains_key("b"));
        assert!(
            features(&lock, "a").contains(&"b".to_string()),
            "{:?}",
            features(&lock, "a")
        );
        assert!(features(&lock, "b").contains(&"inner".to_string()));
    }

    #[test]
    fn implicit_optional_dep_feature() {
        let mut g = Graph::new("implicit");
        g.krate(
            "a",
            "a",
            "1.0.0",
            "[dependencies.b]\nversion = \"1\"\noptional = true\n",
        )
        .krate("b", "b", "1.0.0", "")
        .root("a", &["b"], true);
        let lock = g.resolve();
        assert!(lock.crates.contains_key("b"));
        // No dep: usage anywhere, so the implicit feature cfg exists
        assert!(features(&lock, "a").contains(&"b".to_string()));
    }

    #[test]
    fn strong_dep_slash_feature() {
        let mut g = Graph::new("strong_slash");
        g.krate(
            "a",
            "a",
            "1.0.0",
            "[dependencies.b]\nversion = \"1\"\noptional = true\n\n[features]\nf = [\"b/fast\"]\n",
        )
        .krate("b", "b", "1.0.0", "[features]\nfast = []\n")
        .root("a", &["f"], true);
        let lock = g.resolve();
        assert!(lock.crates.contains_key("b"));
        assert!(features(&lock, "b").contains(&"fast".to_string()));
    }

    #[test]
    fn weak_dep_feature_deferred() {
        let mut g = Graph::new("weak");
        g.krate(
            "a", "a", "1.0.0",
            "[dependencies.b]\nversion = \"1\"\noptional = true\n\n[features]\nf = [\"b?/fast\"]\ng = [\"dep:b\"]\n",
        )
        .krate("b", "b", "1.0.0", "[features]\nfast = []\n");

        // Weak alone: dep stays off
        g.root("a", &["f"], true);
        let lock = g.resolve();
        assert!(!lock.crates.contains_key("b"));

        // Weak + activation: feature applies
        g.root("a", &["f", "g"], true);
        let lock = g.resolve();
        assert!(lock.crates.contains_key("b"));
        assert!(features(&lock, "b").contains(&"fast".to_string()));
    }

    #[test]
    fn version_routing_semver() {
        let mut g = Graph::new("routing");
        g.krate("old", "x", "1.9.3", "")
            .krate("new", "x", "2.2.6", "")
            .krate("a", "a", "1.0.0", "[dependencies]\nx = \"1\"\n")
            .krate("b", "b", "1.0.0", "[dependencies]\nx = \"2\"\n")
            .root("a", &[], true)
            .root("b", &[], true);
        let lock = g.resolve();
        assert_eq!(lock.crates.get("a").unwrap().deps[0].subrepo, "old");
        assert_eq!(lock.crates.get("b").unwrap().deps[0].subrepo, "new");
    }

    #[test]
    fn renamed_dep_keeps_declared_name() {
        let mut g = Graph::new("rename");
        g.krate(
            "a",
            "a",
            "1.0.0",
            "[dependencies.libc_errno]\nversion = \"1\"\npackage = \"errno\"\n",
        )
        .krate("errno", "errno", "1.0.0", "")
        .root("a", &[], true);
        let lock = g.resolve();
        let dep = &lock.crates.get("a").unwrap().deps[0];
        assert_eq!(dep.name, "libc_errno");
        assert_eq!(dep.crate_name, "errno");
    }

    #[test]
    fn rustc_std_workspace_skipped() {
        let mut g = Graph::new("std_workspace");
        g.krate(
            "a",
            "a",
            "1.0.0",
            "[dependencies.alloc]\nversion = \"1\"\npackage = \"rustc-std-workspace-alloc\"\n",
        )
        .root("a", &[], true);
        let lock = g.resolve();
        assert!(dep_names(&lock, "a").is_empty());
    }

    #[test]
    fn platform_deps_filtered() {
        let mut g = Graph::new("platform");
        g.krate(
            "a", "a", "1.0.0",
            "[target.'cfg(unix)'.dependencies]\nu = \"1\"\n\n[target.'cfg(windows)'.dependencies]\nw = \"1\"\n",
        )
        .krate("u", "u", "1.0.0", "")
        .krate("w", "w", "1.0.0", "")
        .root("a", &[], true);
        let lock = g.resolve();
        assert!(lock.crates.contains_key("u"));
        assert!(!lock.crates.contains_key("w"));
    }

    #[test]
    fn build_deps_are_host_units() {
        let mut g = Graph::new("host_build");
        g.krate("a", "a", "1.0.0", "[build-dependencies]\nb = \"1\"\n")
            .krate("b", "b", "1.0.0", "")
            .root("a", &[], true);
        let lock = g.resolve();
        // b active (host-only), placed in primary map, no dual variant
        assert!(lock.crates.contains_key("b"));
        assert!(lock.host_crates.is_empty());
        assert_eq!(lock.crates.get("a").unwrap().build_deps[0].name, "b");
    }

    #[test]
    fn proc_macro_deps_resolve_in_host_unit() {
        let mut g = Graph::new("proc_macro_host");
        g.krate(
            "pm", "pm", "1.0.0",
            "[lib]\nproc-macro = true\n\n[dependencies.util]\nversion = \"1\"\nfeatures = [\"host_only\"]\n",
        )
        .krate("util", "util", "1.0.0", "[features]\nhost_only = []\ntarget_only = []\n")
        .krate(
            "a", "a", "1.0.0",
            "[dependencies]\npm = \"1\"\n\n[dependencies.util]\nversion = \"1\"\nfeatures = [\"target_only\"]\n",
        )
        .root("a", &[], true);
        let lock = g.resolve();
        // util needed by both units with different features -> dual
        assert!(lock.host_crates.contains_key("util"));
        assert!(features(&lock, "util").contains(&"target_only".to_string()));
        assert!(!features(&lock, "util").contains(&"host_only".to_string()));
        let host = lock.host_crates.get("util").unwrap();
        assert!(host.features.contains(&"host_only".to_string()));
        // pm's edge routes to the host variant
        let pm_dep = &lock.crates.get("pm").unwrap().deps[0];
        assert_eq!(pm_dep.target_name, "util_host");
        // a's edge routes to the target build
        let a_util = lock
            .crates
            .get("a")
            .unwrap()
            .deps
            .iter()
            .find(|d| d.name == "util")
            .unwrap();
        assert_eq!(a_util.target_name, "util");
    }

    #[test]
    fn identical_units_share_one_artifact() {
        let mut g = Graph::new("shared_units");
        g.krate(
            "pm",
            "pm",
            "1.0.0",
            "[lib]\nproc-macro = true\n\n[dependencies]\nutil = \"1\"\n",
        )
        .krate("util", "util", "1.0.0", "")
        .krate(
            "a",
            "a",
            "1.0.0",
            "[dependencies]\npm = \"1\"\nutil = \"1\"\n",
        )
        .root("a", &[], true);
        let lock = g.resolve();
        assert!(lock.host_crates.is_empty());
        let pm_dep = &lock.crates.get("pm").unwrap().deps[0];
        assert_eq!(pm_dep.target_name, "util");
    }

    #[test]
    fn undeclared_activated_optional_is_reported_as_missing() {
        // A feature turning on an optional dependency that nobody declared is
        // what `lock` heals automatically; resolution must name it rather
        // than silently dropping it.
        let lock = Graph::new("missing_opt")
            .krate(
                "host",
                "host",
                "1.0.0",
                "[features]\ndefault = []\nextra = [\"dep:helper\"]\n\n[dependencies.helper]\nversion = \"1\"\noptional = true\n",
            )
            .root("host", &["extra"], true)
            .resolve();
        assert_eq!(lock.missing.len(), 1, "missing: {:?}", lock.missing);
        assert_eq!(lock.missing[0].package, "helper");
        assert_eq!(lock.missing[0].requirer, "host");
    }

    #[test]
    fn incompatible_declared_version_is_not_silently_used() {
        // The regression: with only x 2.0 declared, a's requirement on ^1 must
        // not be routed to it. Reported as missing instead.
        let lock = Graph::new("misroute")
            .krate("a", "a", "1.0.0", "[dependencies]\nx = \"1\"\n")
            .krate("x", "x", "2.0.0", "")
            .root("a", &[], true)
            .resolve();
        assert_eq!(lock.missing.len(), 1, "missing: {:?}", lock.missing);
        assert_eq!(lock.missing[0].package, "x");
        assert_eq!(lock.missing[0].req.as_deref(), Some("^1"));
        assert!(!lock.crates.contains_key("x"));
    }

    #[test]
    fn a_closed_graph_reports_nothing_missing() {
        let lock = Graph::new("closed_graph")
            .krate("host", "host", "1.0.0", "[dependencies]\nhelper = \"1\"\n")
            .krate("helper", "helper", "1.2.0", "")
            .root("host", &[], true)
            .resolve();
        assert!(lock.missing.is_empty(), "missing: {:?}", lock.missing);
    }

    #[test]
    fn parse_manifest_accepts_resolver_three() {
        let m = parse_manifest(b"[package]\nname = \"t\"\nversion = \"1.0.0\"\nresolver = \"3\"\n")
            .unwrap();
        assert_eq!(m.package.as_ref().unwrap().name, "t");
    }

    #[test]
    fn parse_manifest_normalizes_underscore_keys() {
        let m = parse_manifest(
            b"[package]\nname = \"t\"\nversion = \"1.0.0\"\n\n[dependencies.x]\nversion = \"1\"\ndefault_features = false\n",
        )
        .unwrap();
        let dep = m.dependencies.get("x").unwrap();
        assert!(!dep.detail().unwrap().default_features);
    }

    #[test]
    fn cfg_applies_evaluates_target() {
        let info =
            cfg_expr::targets::get_builtin_target_by_triple("x86_64-unknown-linux-gnu").unwrap();
        assert!(cfg_applies("cfg(unix)", info));
        assert!(!cfg_applies("cfg(windows)", info));
        assert!(cfg_applies("cfg(target_os = \"linux\")", info));
        assert!(cfg_applies("x86_64-unknown-linux-gnu", info));
        assert!(!cfg_applies("aarch64-apple-darwin", info));
        assert!(!cfg_applies("cfg(invalid syntax", info));
    }

    #[test]
    fn inline_entry_parsing() {
        let e = parse_inline_entry("sub|my-crate|1.2.3|a,b|true|false", Path::new("/m")).unwrap();
        assert_eq!(e.subrepo, "sub");
        assert_eq!(e.crate_name, "my-crate");
        assert_eq!(e.features, vec!["a", "b"]);
        assert!(e.root);
        assert!(!e.default_features);
        assert_eq!(e.manifest, Path::new("/m/sub.manifest.toml"));
        assert!(parse_inline_entry("too|few|fields", Path::new("/m")).is_err());
    }
}

#[cfg(test)]
mod run_io_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn run_reads_entries_json_and_writes_lock() {
        let dir =
            std::env::temp_dir().join(format!("please_rust_resolve_run_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let manifest = dir.join("a.toml");
        fs::write(&manifest, "[package]\nname = \"a\"\nversion = \"1.0.0\"\n").unwrap();
        let entries = dir.join("entries.json");
        fs::write(
            &entries,
            format!(
                r#"[{{"subrepo": "a", "crate_name": "a", "version": "1.0.0", "manifest": "{}", "root": true}}]"#,
                manifest.display()
            ),
        )
        .unwrap();
        let output = dir.join("rust.lock");
        run(ResolveArgs {
            entries: Some(entries),
            inline_entries: vec![],
            manifest_dir: PathBuf::from("."),
            target: "x86_64-unknown-linux-gnu".to_string(),
            output: output.clone(),
        })
        .unwrap();
        let lock = LockFile::load(&output).unwrap();
        assert!(lock.crates.contains_key("a"));
    }

    #[test]
    fn run_accepts_inline_entries() {
        let dir =
            std::env::temp_dir().join(format!("please_rust_resolve_inline_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("a.manifest.toml"),
            "[package]\nname = \"a\"\nversion = \"1.0.0\"\n",
        )
        .unwrap();
        let output = dir.join("rust.lock");
        run(ResolveArgs {
            entries: None,
            inline_entries: vec!["a|a|1.0.0||true|true".to_string()],
            manifest_dir: dir.clone(),
            target: "x86_64-unknown-linux-gnu".to_string(),
            output: output.clone(),
        })
        .unwrap();
        assert!(LockFile::load(&output).unwrap().crates.contains_key("a"));
    }

    #[test]
    fn unsatisfiable_requirement_is_reported_not_misrouted() {
        let dir = std::env::temp_dir().join(format!(
            "please_rust_resolve_conflict_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("a.toml"),
            "[package]\nname = \"a\"\nversion = \"1.0.0\"\n\n[dependencies]\nx = \"3\"\n",
        )
        .unwrap();
        fs::write(
            dir.join("x1.toml"),
            "[package]\nname = \"x\"\nversion = \"1.0.0\"\n",
        )
        .unwrap();
        fs::write(
            dir.join("x2.toml"),
            "[package]\nname = \"x\"\nversion = \"2.0.0\"\n",
        )
        .unwrap();
        let mk = |sub: &str, name: &str, ver: &str, file: &str, root: bool| EntryInput {
            subrepo: sub.to_string(),
            crate_name: name.to_string(),
            version: ver.to_string(),
            manifest: dir.join(file),
            features: vec![],
            root,
            default_features: true,
        };
        // Nothing declared satisfies a's requirement on x. Rather than
        // routing it to an arbitrary declared version (which surfaces as a
        // baffling compile error) or failing outright, resolution reports the
        // gap so `lock` can declare the version that is needed.
        let lock = resolve_entries(
            &[
                mk("a", "a", "1.0.0", "a.toml", true),
                mk("x1", "x", "1.0.0", "x1.toml", false),
                mk("x2", "x", "2.0.0", "x2.toml", false),
            ],
            "x86_64-unknown-linux-gnu",
        )
        .unwrap();
        assert_eq!(lock.missing.len(), 1, "missing: {:?}", lock.missing);
        assert_eq!(lock.missing[0].package, "x");
        assert_eq!(lock.missing[0].requirer, "a");
        assert!(!lock.crates.contains_key("x1"));
        assert!(!lock.crates.contains_key("x2"));
    }

    #[test]
    fn lockfile_load_errors() {
        assert!(LockFile::load(Path::new("/nonexistent/rust.lock")).is_err());
        let dir = std::env::temp_dir().join(format!("please_rust_lock_bad_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let bad = dir.join("rust.lock");
        fs::write(&bad, "not json").unwrap();
        assert!(LockFile::load(&bad).is_err());
    }
}
