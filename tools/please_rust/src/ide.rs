//! `rust-project.json` generation, for rust-analyzer without cargo.
//!
//! rust-analyzer understands two kinds of project: a cargo workspace, which
//! it drives by running cargo, and a `rust-project.json` describing the crate
//! graph directly. The second exists for build systems like this one, and it
//! is what rules_rust emits too.
//!
//! Almost nothing here is derived: the resolved lock already carries the
//! graph, each crate's manifest carries its edition and root module, and the
//! build scripts have already written the cfgs they set. This assembles what
//! is on disk rather than recomputing it.

use anyhow::{Context, Result};
use clap::Args;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Args)]
pub struct IdeArgs {
    /// A resolved lock, as `rust_resolve` produces it. Repeatable, because a
    /// repo can declare third-party crates in more than one place and each
    /// `rust_resolve` produces its own lock.
    ///
    /// Two forms. `<package>=<third_party_dir>=<path>` says which build
    /// package the lock's crates are declared in, which is how a dep's label
    /// picks the lock it belongs to, and where that lock's subrepos are
    /// checked out. A bare `<path>` is the old single-lock form and pairs
    /// with `--third-party-dir`.
    #[arg(long)]
    pub lock: Vec<String>,

    /// Directory holding the generated crate subrepos, one per declaration.
    /// Applies to a `--lock` given in the bare form.
    #[arg(long, default_value = "plz-out/gen/third_party/crates")]
    pub third_party_dir: PathBuf,

    /// A rustup-shaped toolchain root: `bin/rustc` beside `lib/rustlib`.
    ///
    /// Note this is `<toolchain>_rustc` and *not* `<toolchain>_sysroot`,
    /// which is the rust-std component and has no `bin/`. rust-analyzer runs
    /// `<sysroot>/bin/rustc --print cfg` to learn the target's cfgs, so
    /// pointing it at the rlibs makes it log `failed to get rustc cfgs` and
    /// analyse everything without `unix`, `target_pointer_width` and the
    /// rest.
    #[arg(long)]
    pub sysroot: Option<PathBuf>,

    /// The standard library's sources, which is what go-to-definition into
    /// std resolves against. `<toolchain>_sysroot_src` produces it.
    #[arg(long)]
    pub sysroot_src: Option<PathBuf>,

    /// Per-crate JSON emitted by first-party rules. Those rules live only in
    /// plz's parser - nothing in this tool ever sees a rust_library - so each
    /// one writes what it knows and this merges them.
    #[arg(long = "first-party", num_args = 0..)]
    pub first_party: Vec<PathBuf>,

    /// Fragments from crates in a subrepo, as `<checkout>=<fragment>`.
    ///
    /// A fragment describes its paths relative to the repo it was declared
    /// in, which for a subrepo is not the repo the project file sits at the
    /// root of. The caller pairs each with where that subrepo is checked out,
    /// and everything in it hangs off that.
    #[arg(long = "subrepo-crate")]
    pub subrepo_crate: Vec<String>,

    /// Where to write the project file
    #[arg(long, default_value = "rust-project.json")]
    pub output: PathBuf,

    /// Write every file this project will point at, as `subrepo<TAB>path`.
    ///
    /// Naming a path is not the same as it existing, and the difference is
    /// always silent: rust-analyzer degrades and says nothing useful. The
    /// caller builds whatever is missing before handing the project over.
    ///
    /// Emitted rather than listed by kind, because listing by kind is a list
    /// to keep right - it was wrong four times, once per artifact, before this
    /// existed.
    #[arg(long)]
    pub emit_inputs: Option<PathBuf>,

    /// Speak rust-analyzer's discover protocol on stdout instead of writing a
    /// file.
    ///
    /// `rust-analyzer.workspace.discoverConfig` names a command to run when a
    /// project is opened, and re-runs it when a watched file changes. That is
    /// the same shape go-rules uses for gopls, where a GOPACKAGESDRIVER binary
    /// answers queries instead of a file being generated and kept in step:
    /// nothing to run by hand and nothing to go stale.
    #[arg(long)]
    pub discover: bool,

    /// The build file to report as the one this project came from. Only
    /// meaningful with --discover; rust-analyzer keys a workspace on it.
    ///
    /// Must be absolute. rust-analyzer calls `AbsPathBuf::try_from` on it and
    /// *panics* on a relative one - taking the language server down with it -
    /// so this is made absolute here rather than trusted, whatever the
    /// protocol's own example looks like.
    #[arg(long, default_value = "BUILD")]
    pub buildfile: String,
}

/// What a first-party rule knows about itself, which is everything
/// rust-project.json wants and none of which reaches this tool any other way.
#[derive(serde::Deserialize)]
struct FirstParty {
    display_name: String,
    /// This crate's own build label. Deps name labels, and two crates can
    /// share a display name - a corpus with a probe crate per third-party
    /// crate has hundreds - so matching them by name makes a crate its own
    /// dependency and rust-analyzer reports a cycle.
    #[serde(default)]
    label: String,
    /// Repo-relative; made absolute against `--repo-root`.
    root_module: String,
    edition: String,
    #[serde(default)]
    features: Vec<String>,
    /// The crate's Cargo.toml, if it has one, for CARGO_PKG_* - which is what
    /// `clap::command!()` reads. Repo-relative.
    #[serde(default)]
    manifest: Option<String>,
    #[serde(default)]
    is_proc_macro: bool,
    /// Build labels, resolved against the third-party subrepos and against
    /// the other fragments.
    #[serde(default)]
    deps: Vec<FirstPartyDep>,
}

#[derive(serde::Deserialize)]
struct FirstPartyDep {
    name: String,
    label: String,
}

/// The subrepo a third-party dep label names: `//third_party/crates:serde`
/// is the subrepo `serde`, which is how the lock keys it.
fn label_subrepo(label: &str) -> Option<String> {
    label.rsplit(':').next().map(|t| t.to_string())
}

/// The build package a label names, without the leading slashes or the
/// target: `//third_party/crates:serde` is declared in `third_party/crates`.
fn label_package(label: &str) -> String {
    let body = label.split_once(':').map(|(p, _)| p).unwrap_or(label);
    body.trim_start_matches('/').to_string()
}

#[derive(Serialize)]
struct Project {
    #[serde(skip_serializing_if = "Option::is_none")]
    sysroot: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sysroot_src: Option<String>,
    /// The standard library, described rather than discovered.
    ///
    /// Listing core and std among the ordinary crates does not work: they are
    /// then crates that happen to be called "core" and "std", and
    /// rust-analyzer only attaches lang items to the crate it believes *is*
    /// the sysroot - so `Sized` is unsatisfied for `char` and `Iterator` has
    /// no impls, while every import and macro around them resolves fine.
    ///
    /// Letting it discover them instead means it runs `cargo metadata` over
    /// the stdlib sources with a nightly-only `-Z` flag, against whatever
    /// cargo is on PATH. This is the third option rust-analyzer offers, and
    /// the only one that is both correct and hermetic. Paths inside are
    /// relative to `sysroot_src`.
    #[serde(skip_serializing_if = "Option::is_none")]
    sysroot_project: Option<SysrootProject>,
    crates: Vec<CrateEntry>,
}

/// rust-analyzer's discover protocol: JSON objects, one per line.
///
/// Provisional and versioned only by the analyzer, so it is spelled out here
/// rather than derived from anything: `kind` is the tag and the variants are
/// snake_case.
#[derive(Serialize)]
#[serde(tag = "kind")]
#[serde(rename_all = "snake_case")]
enum DiscoverData<'a> {
    Finished {
        buildfile: &'a str,
        project: &'a Project,
    },
    Error {
        error: String,
        source: Option<String>,
    },
}

#[derive(Serialize)]
struct SysrootProject {
    crates: Vec<CrateEntry>,
}

/// Which directories a crate's files live in.
///
/// Omitted for almost every crate, where rust-analyzer derives it from the
/// root module's parent and is right. It is needed when a crate's sources are
/// not all in one place: a build script writes generated code somewhere else,
/// and rust-analyzer will not load a file from a directory the crate does not
/// claim, however correct the path in `include!` is.
#[derive(Serialize)]
struct CrateSource {
    include_dirs: Vec<String>,
    exclude_dirs: Vec<String>,
}

#[derive(Serialize)]
struct CrateEntry {
    display_name: String,
    root_module: String,
    edition: String,
    deps: Vec<DepEntry>,
    cfg: Vec<String>,
    env: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<CrateSource>,
    is_proc_macro: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    proc_macro_dylib_path: Option<String>,
    /// Third-party crates are not workspace members: rust-analyzer uses this
    /// to decide what to check on save and what to only index.
    is_workspace_member: bool,
}

#[derive(Serialize, Clone)]
struct DepEntry {
    /// Index into the crates array. rust-project.json addresses deps
    /// positionally, which is why the graph has to be walked twice - once to
    /// assign indices and once to resolve them.
    #[serde(rename = "crate")]
    krate: usize,
    name: String,
}

/// The standard library, as crates rather than as a directory to discover.
///
/// rust-analyzer can find these itself from `sysroot_src`, but it does so by
/// running `cargo metadata` over the stdlib sources - picking up whatever
/// cargo is on PATH, which in a build system whose point is not depending on
/// ambient tooling is the wrong answer, and which fails outright when that
/// cargo is older than the toolchain. Naming them explicitly is what
/// rules_rust does and needs nothing from the environment.
const SYSROOT_CRATES: &[(&str, &[&str])] = &[
    ("core", &[]),
    ("alloc", &["core"]),
    ("panic_unwind", &["core", "alloc"]),
    ("panic_abort", &["core", "alloc"]),
    ("std", &["core", "alloc", "panic_unwind", "panic_abort"]),
    ("proc_macro", &["core", "std"]),
    ("test", &["core", "std", "proc_macro"]),
];

/// The crates the standard library is itself built from.
///
/// std does not stand alone. `std::collections::HashMap` wraps hashbrown's,
/// and with hashbrown undescribed rust-analyzer cannot infer `HashMap::new()`
/// at all - every `let mut m = HashMap::new()` in the repo reports "type
/// annotations needed", in ordinary first-party code, with nothing to point
/// at. cargo learns this graph by resolving the stdlib workspace; `rust-src`
/// ships the manifests and the vendored sources beside them, so it can be
/// read instead of run.
///
/// Only unconditional `[dependencies]` are followed. Target-gated ones (libc,
/// on unix) would need the target's cfgs evaluated here, and nothing so far
/// needs them.
fn extend_with_sysroot_deps(
    src: &Path,
    sys: &mut Vec<CrateEntry>,
    at: &mut BTreeMap<String, usize>,
) {
    // Seeded with the crates already described, whose manifests sit beside
    // their sources; discovered crates are appended as they are found.
    // Each entry carries the features it is built with, because that is what
    // decides which of its own optional dependencies are on. hashbrown is the
    // case that matters: it reaches core and alloc only through its `core`
    // and `alloc` features, which `rustc-dep-of-std` turns on.
    let mut queue: Vec<(String, PathBuf, Vec<String>)> = SYSROOT_CRATES
        .iter()
        .map(|(n, _)| (n.to_string(), src.join(n), Vec::new()))
        .collect();
    let mut seen: Vec<String> = queue.iter().map(|(n, _, _)| n.clone()).collect();
    let mut edges: Vec<(String, String, String)> = Vec::new();
    let mut i = 0;
    while i < queue.len() {
        let (name, dir, features) = queue[i].clone();
        i += 1;
        let Ok(bytes) = std::fs::read(dir.join("Cargo.toml")) else {
            continue;
        };
        let Ok(manifest) = crate::resolve::parse_manifest(&bytes) else {
            continue;
        };
        for (dep_name, dep) in &manifest.dependencies {
            if dep.optional() && !optional_dep_enabled(dep_name, &features, &manifest) {
                continue;
            }
            let key = dep_name.replace('-', "_");
            edges.push((name.clone(), key.clone(), dep_name.clone()));
            if seen.contains(&key) {
                continue;
            }
            // A renamed dependency's sources are under its real package name;
            // std's own crates rename core and alloc to workspace shims.
            let package = dep.package().unwrap_or(dep_name);
            let Some(dep_dir) = locate_sysroot_dep(src, &dir, package, dep) else {
                continue;
            };
            let Ok(dep_bytes) = std::fs::read(dep_dir.join("Cargo.toml")) else {
                continue;
            };
            let Ok(dep_manifest) = crate::resolve::parse_manifest(&dep_bytes) else {
                continue;
            };
            let root = dep_manifest
                .lib
                .as_ref()
                .and_then(|l| l.path.clone())
                .unwrap_or_else(|| "src/lib.rs".to_string());
            let Ok(rel_dir) = dep_dir.strip_prefix(src) else {
                continue;
            };
            // A bare `dep = "1"` has no detail block and takes defaults.
            let defaults = dep.detail().is_none_or(|d| d.default_features);
            let dep_features = expand_features(&dep_manifest, dep.req_features(), defaults);
            seen.push(key.clone());
            at.insert(key.clone(), sys.len());
            sys.push(CrateEntry {
                display_name: key.clone(),
                root_module: format!("{}/{}", rel_dir.display(), root),
                edition: dep_manifest
                    .package
                    .as_ref()
                    .map(|p| (p.edition() as u32).to_string())
                    .unwrap_or_else(|| "2021".to_string()),
                deps: Vec::new(),
                cfg: dep_features
                    .iter()
                    .map(|f| format!("feature=\"{}\"", f))
                    .collect(),
                env: BTreeMap::new(),
                source: None,
                is_proc_macro: false,
                proc_macro_dylib_path: None,
                is_workspace_member: false,
            });
            queue.push((key, dep_dir, dep_features));
        }
    }
    for (from, to, declared) in edges {
        let (Some(f), Some(t)) = (at.get(&from).copied(), at.get(&to).copied()) else {
            continue;
        };
        let name = declared.replace('-', "_");
        if !sys[f].deps.iter().any(|d| d.name == name) {
            sys[f].deps.push(DepEntry { krate: t, name });
        }
    }
}

/// Whether an optional dependency is switched on, which is the case when a
/// feature of that name is enabled or an enabled feature names it with `dep:`.
fn optional_dep_enabled(
    dep_name: &str,
    features: &[String],
    manifest: &cargo_toml::Manifest,
) -> bool {
    if features.iter().any(|f| f == dep_name) {
        return true;
    }
    let marker = format!("dep:{}", dep_name);
    features.iter().any(|f| {
        manifest
            .features
            .get(f)
            .is_some_and(|implied| implied.contains(&marker))
    })
}

/// Where a stdlib dependency's sources are: beside it for a path dependency,
/// and under `vendor/` for one that came from crates.io.
fn locate_sysroot_dep(
    src: &Path,
    from: &Path,
    name: &str,
    dep: &cargo_toml::Dependency,
) -> Option<PathBuf> {
    if let Some(rel) = dep.detail().and_then(|d| d.path.as_ref()) {
        let joined = from.join(rel);
        // The path is relative to the depending manifest and usually climbs
        // out of it, which strip_prefix later cannot handle unnormalised.
        return normalise(&joined).filter(|p| p.exists());
    }
    // The workspace shims (rustc-std-workspace-core and friends) sit at the
    // top of the source tree rather than under vendor/.
    let sibling = src.join(name);
    if sibling.is_dir() {
        return Some(sibling);
    }
    let vendor = src.join("vendor");
    let mut best: Option<PathBuf> = None;
    for entry in std::fs::read_dir(&vendor).ok()?.flatten() {
        let file = entry.file_name();
        let file = file.to_string_lossy();
        // `hashbrown-0.17.1`, and not `hashbrown-utils-0.1` - the version is
        // what follows the final hyphen before a digit.
        let Some(rest) = file.strip_prefix(name) else {
            continue;
        };
        if !rest.starts_with('-') || !rest[1..].starts_with(|c: char| c.is_ascii_digit()) {
            continue;
        }
        if best.as_ref().is_none_or(|b| {
            b.file_name().map(|n| n.to_string_lossy().to_string()) < Some(file.to_string())
        }) {
            best = Some(entry.path());
        }
    }
    best
}

/// `..` resolved textually. The sources are laid out by the toolchain, so
/// there is nothing to canonicalise against and no symlinks to chase.
fn normalise(p: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for part in p.components() {
        match part {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    Some(out)
}

/// The features a dependency is built with: those the dependent asked for,
/// plus `default` unless it opted out, closed over the crate's own feature
/// table. `dep:x` and `x/y` entries enable dependencies rather than cfgs, so
/// they are followed but not emitted.
fn expand_features(
    manifest: &cargo_toml::Manifest,
    asked: &[String],
    default: bool,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut queue: Vec<String> = asked.to_vec();
    if default && manifest.features.contains_key("default") {
        queue.push("default".to_string());
    }
    while let Some(f) = queue.pop() {
        if f.contains('/') || f.starts_with("dep:") || out.contains(&f) {
            continue;
        }
        out.push(f.clone());
        if let Some(implied) = manifest.features.get(&f) {
            queue.extend(implied.iter().cloned());
        }
    }
    out.sort();
    out
}

/// A crate's identity in the lock: its subrepo, and which unit of it. The
/// host unit of a dual crate is a different crate to rust-analyzer - it is
/// compiled for a different platform and may have different features.
/// A crate's identity across every lock: which lock it came from, its
/// subrepo, and which unit of it. The lock is part of the key because two
/// locks can each hold a `serde` and they are different crates.
type Key = (usize, String, bool);

/// One lock, and what is needed to place the crates in it.
struct LockSource {
    /// The build package its crates are declared in, e.g. `third_party/crates`.
    /// A first-party dep names `//third_party/crates:serde`, and this is how
    /// that label finds its lock.
    package: String,
    /// Where this lock's subrepos are checked out.
    third_party_dir: PathBuf,
    lock: crate::resolve::LockFile,
}

/// Which lock a dep's label belongs to, by the package its crates are
/// declared in. A lock given in the bare form has no package recorded and
/// matches anything, which is what a single-lock repo has always done.
fn lock_for_label(sources: &[LockSource], label: &str) -> Option<usize> {
    let pkg = label_package(label);
    sources
        .iter()
        .position(|s| s.package == pkg)
        .or_else(|| sources.iter().position(|s| s.package.is_empty()))
}

/// Parse a `--lock` value. `<package>=<dir>=<path>`, or a bare path paired
/// with `--third-party-dir`.
fn parse_lock_spec(spec: &str, fallback_dir: &Path) -> (String, PathBuf, PathBuf) {
    let parts: Vec<&str> = spec.splitn(3, '=').collect();
    if parts.len() == 3 {
        (
            parts[0].to_string(),
            PathBuf::from(parts[1]),
            PathBuf::from(parts[2]),
        )
    } else {
        (
            String::new(),
            fallback_dir.to_path_buf(),
            PathBuf::from(spec),
        )
    }
}

/// rust-analyzer panics rather than errors on a relative buildfile, so no
/// caller gets to pass one.
fn absolute(path: &str) -> String {
    let p = Path::new(path);
    if p.is_absolute() {
        return path.to_string();
    }
    match std::env::current_dir() {
        Ok(cwd) => cwd.join(p).display().to_string(),
        Err(_) => path.to_string(),
    }
}

fn read_fragment(path: &Path) -> Result<FirstParty> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

/// Move a fragment's paths from the repo it was declared in to the one the
/// project file sits at the root of.
///
/// `checkout` is where that subrepo lives, relative to this repo. Every path
/// the fragment carries is relative to the subrepo, so every one hangs off
/// it - the root module and the manifest CARGO_PKG_* is read from.
fn rebase(fp: &mut FirstParty, checkout: &str) {
    fn at(checkout: &str, p: &str) -> String {
        if checkout.is_empty() {
            p.to_string()
        } else {
            format!("{}/{}", checkout.trim_end_matches('/'), p)
        }
    }
    fp.root_module = at(checkout, &fp.root_module);
    fp.manifest = fp.manifest.as_deref().map(|m| at(checkout, m));
}

/// The package env a first-party crate compiles with, read from its manifest
/// the same way a compile reads it. Without this `clap::command!()` reports
/// that CARGO_PKG_VERSION is unset - in the editor only, since the compile
/// sets it.
fn first_party_env(fp: &FirstParty) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    env.insert("CARGO_CRATE_NAME".to_string(), fp.display_name.clone());
    let Some(path) = &fp.manifest else {
        return env;
    };
    let Ok(content) = std::fs::read(path) else {
        return env;
    };
    let Ok(manifest) = crate::resolve::parse_manifest(&content) else {
        return env;
    };
    if let Some(pkg) = &manifest.package {
        env.extend(crate::build_script::package_env(pkg));
    }
    if let Some(dir) = Path::new(path).parent() {
        env.insert("CARGO_MANIFEST_DIR".to_string(), dir.display().to_string());
    }
    env
}

pub fn run(args: IdeArgs) -> Result<()> {
    if !args.discover {
        return describe_project(args);
    }
    // A discover command that exits non-zero with nothing on stdout tells
    // rust-analyzer only that something went wrong. The protocol has a place
    // to say what.
    let buildfile = args.buildfile.clone();
    match describe_project(args) {
        Ok(()) => Ok(()),
        Err(e) => {
            let line = serde_json::to_string(&DiscoverData::Error {
                error: format!("{:#}", e),
                source: Some(buildfile),
            })?;
            println!("{}", line);
            Ok(())
        }
    }
}

fn describe_project(args: IdeArgs) -> Result<()> {
    let mut sources: Vec<LockSource> = Vec::new();
    for spec in &args.lock {
        let (package, third_party_dir, path) = parse_lock_spec(spec, &args.third_party_dir);
        let lock = crate::resolve::LockFile::load(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        sources.push(LockSource {
            package,
            third_party_dir,
            lock,
        });
    }

    // The sysroot is described as a nested project rather than as crates in
    // the main list, so rust-analyzer registers it as the sysroot and its
    // lang items attach. Indices inside it are its own.
    let mut sysroot_project = None;
    if let Some(src) = &args.sysroot_src {
        let mut sys: Vec<CrateEntry> = Vec::new();
        let mut at: BTreeMap<String, usize> = BTreeMap::new();
        for (name, _) in SYSROOT_CRATES {
            at.insert(name.to_string(), sys.len());
            sys.push(CrateEntry {
                display_name: name.to_string(),
                // Relative to sysroot_src, which is what rust-analyzer
                // absolutizes these against.
                root_module: format!("{}/src/lib.rs", name),
                edition: sysroot_edition(&src.join(name)),
                deps: Vec::new(),
                cfg: Vec::new(),
                env: BTreeMap::new(),
                source: None,
                is_proc_macro: false,
                proc_macro_dylib_path: None,
                is_workspace_member: false,
            });
        }
        for (name, deps) in SYSROOT_CRATES {
            let i = at[*name];
            sys[i].deps = deps
                .iter()
                .filter_map(|d| {
                    at.get(*d).map(|j| DepEntry {
                        krate: *j,
                        name: d.to_string(),
                    })
                })
                .collect();
        }
        // std is not self-contained, and the crates it is built from have to
        // be described too - see extend_with_sysroot_deps.
        extend_with_sysroot_deps(src, &mut sys, &mut at);
        sysroot_project = Some(SysrootProject { crates: sys });
    }
    let mut crates: Vec<CrateEntry> = Vec::new();
    let offset = 0usize;

    // Pass one: assign an index to every crate, so deps can name them.
    let mut order: Vec<(Key, &crate::resolve::LockEntry)> = Vec::new();
    for (li, src) in sources.iter().enumerate() {
        for (subrepo, entry) in &src.lock.crates {
            order.push(((li, subrepo.clone(), false), entry));
        }
        for (subrepo, entry) in &src.lock.host_crates {
            order.push(((li, subrepo.clone(), true), entry));
        }
    }
    let index: BTreeMap<Key, usize> = order
        .iter()
        .enumerate()
        .map(|(i, (k, _))| (k.clone(), i + offset))
        .collect();

    // What each crate is imported by, so a dep can be named the way the
    // depending source writes it rather than the way the package is named.
    let idents: BTreeMap<Key, String> = order
        .iter()
        .map(|(k, e)| (k.clone(), ident_of(e)))
        .collect();

    // Pass two: describe each one.
    let mut skipped = Vec::new();
    // What the project will point at, and which crate has to be built for it
    // to be there.
    let mut inputs: Vec<(String, String)> = Vec::new();
    for ((li, subrepo, _host), entry) in &order {
        match describe(
            &sources[*li].third_party_dir,
            *li,
            subrepo,
            entry,
            &index,
            &idents,
        ) {
            Ok(c) => {
                // The buildable label, because with more than one lock a
                // subrepo name does not say which package declares it.
                let target = if sources[*li].package.is_empty() {
                    subrepo.clone()
                } else {
                    format!("//{}:{}", sources[*li].package, subrepo)
                };
                inputs.push((target.clone(), c.root_module.clone()));
                if let Some(dylib) = &c.proc_macro_dylib_path {
                    inputs.push((target, dylib.clone()));
                }
                crates.push(c)
            }
            Err(e) => {
                skipped.push(format!("{}: {:#}", subrepo, e));
                // A crate that cannot be described must still occupy its
                // index, or every dep after it points at the wrong crate.
                crates.push(placeholder(entry));
            }
        }
    }

    if let Some(path) = &args.emit_inputs {
        let text: String = inputs
            .iter()
            .map(|(subrepo, file)| format!("{}\t{}\n", subrepo, file))
            .collect();
        std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))?;
    }

    // First-party crates go after the third-party ones so the indices
    // assigned above stay valid.
    // The third element is the subrepo a crate came from, empty for this one.
    // It scopes label lookups and decides workspace membership in one.
    let mut first: Vec<(String, FirstParty, String)> = Vec::new();
    for path in &args.first_party {
        first.push((
            path.display().to_string(),
            read_fragment(path)?,
            String::new(),
        ));
    }
    for spec in &args.subrepo_crate {
        let (checkout, path) = spec
            .split_once('=')
            .with_context(|| format!("--subrepo-crate wants <checkout>=<fragment>, got {spec}"))?;
        let path = Path::new(path);
        let mut fp = read_fragment(path)?;
        rebase(&mut fp, checkout);
        // Never a workspace member, however much of it you own. rust-analyzer
        // typechecks members on save, and a subrepo is checked in its own
        // repo - having its errors appear in this one's problems panel is
        // noise about code this checkout cannot fix.
        first.push((path.display().to_string(), fp, checkout.to_string()));
    }
    // Keyed by label, and scoped: a subrepo's labels are relative to itself,
    // so `//foo:bar` there and `//foo:bar` here are different crates.
    let mut by_label: BTreeMap<(String, String), usize> = BTreeMap::new();
    // What each one is imported by, for the same reason as the third-party
    // idents: a label names a build target and source names a crate.
    let mut ident_by_label: BTreeMap<(String, String), String> = BTreeMap::new();
    for (i, (_, fp, scope)) in first.iter().enumerate() {
        if !fp.label.is_empty() {
            by_label.insert((scope.clone(), fp.label.clone()), crates.len() + i);
            ident_by_label.insert(
                (scope.clone(), fp.label.clone()),
                fp.display_name.replace('-', "_"),
            );
        }
    }
    // A dep that resolves to nothing is dropped, and the only symptom is an
    // import rust-analyzer cannot follow. Say so instead.
    let mut unresolved: Vec<(String, String)> = Vec::new();
    for (_, fp, scope) in &first {
        let mut deps = Vec::new();
        for d in &fp.deps {
            let name = d.name.replace('-', "_");
            // A first-party dep names another fragment in the same repo;
            // anything else is a declaration in the lock.
            if let Some(i) = by_label.get(&(scope.clone(), d.label.clone())) {
                // The dep's own name, not the label's. A generated proto
                // crate is built by a target called _widget_proto#rust and
                // imported as widget_proto.
                let name = ident_by_label
                    .get(&(scope.clone(), d.label.clone()))
                    .cloned()
                    .unwrap_or(name);
                deps.push(DepEntry { krate: *i, name });
            } else if let Some(i) = lock_for_label(&sources, &d.label)
                .zip(label_subrepo(&d.label))
                .and_then(|(li, s)| index.get(&(li, s, false)))
            {
                // The crate's own name rather than the label's: a label
                // carries the package, and rustls-webpki builds webpki.
                let resolved = crates[*i].display_name.clone();
                deps.push(DepEntry {
                    krate: *i,
                    name: resolved,
                });
            } else {
                unresolved.push((fp.display_name.clone(), d.label.clone()));
            }
        }
        crates.push(CrateEntry {
            display_name: fp.display_name.clone(),
            root_module: fp.root_module.clone(),
            edition: fp.edition.clone(),
            deps,
            cfg: fp
                .features
                .iter()
                .map(|f| format!("feature=\"{}\"", f))
                // `test`, on every crate being worked on, because that is what
                // rust-analyzer does under cargo: `cargo.unsetTest` defaults to
                // ["core"], so a workspace member's `#[cfg(test)] mod tests` is
                // live code and not grey text. It also settles a collision -
                // a rust_library and the rust_test over the same root are two
                // crates sharing one file, and rust-analyzer applies whichever
                // it saw first to both.
                .chain(std::iter::once("test".to_string()))
                .collect(),
            env: first_party_env(fp),
            source: None,
            is_proc_macro: fp.is_proc_macro,
            proc_macro_dylib_path: None,
            // This is the code being worked on, which is what the flag means:
            // rust-analyzer checks these on save and only indexes the rest.
            // A subrepo is checked in its own repo, so it never qualifies.
            is_workspace_member: scope.is_empty(),
        });
    }

    let project = Project {
        sysroot: args.sysroot.as_ref().map(|p| rel(p)),
        sysroot_src: args.sysroot_src.as_ref().map(|p| rel(p)),
        sysroot_project,
        crates,
    };
    if args.discover {
        // One line, because the protocol is JSONL and a pretty-printed object
        // is a hundred lines of syntax error to a line-oriented reader.
        let line = serde_json::to_string(&DiscoverData::Finished {
            buildfile: &absolute(&args.buildfile),
            project: &project,
        })?;
        println!("{}", line);
    } else {
        let json = serde_json::to_string_pretty(&project)?;
        std::fs::write(&args.output, json + "\n")
            .with_context(|| format!("writing {}", args.output.display()))?;
    }

    // One line per crate is unreadable when the whole lock is stale, and that
    // is exactly when it happens: a lock built by a please_rust older than the
    // fields it records leaves every crate without a root module, and the real
    // message drowns in a thousand copies of itself.
    let stale = skipped
        .iter()
        .filter(|s| s.contains("no root module recorded"))
        .count();
    if stale > 0 {
        eprintln!(
            "ide: {} of {} crates have no root module. The lock was written by an \
             older please_rust than this one - rebuild it, and check the tool the \
             lock rule uses rather than the one this ran as. Note that `plz -o` does \
             not reach a nested plz: set PLEASE_RUST_TOOL in .plzconfig, or pass \
             PLZ_OVERRIDES in the environment.",
            stale,
            project.crates.len()
        );
    }
    for s in skipped
        .iter()
        .filter(|s| !s.contains("no root module recorded"))
    {
        eprintln!("ide: could not describe {}", s);
    }
    for (krate, label) in &unresolved {
        eprintln!(
            "ide: {} depends on {}, which is in no lock passed to --lock - \
             imports from it will not resolve",
            krate, label
        );
    }
    if args.discover {
        eprintln!("ide: described {} crates", project.crates.len());
    } else {
        eprintln!(
            "ide: wrote {} crates to {}",
            project.crates.len(),
            args.output.display()
        );
    }
    Ok(())
}

/// An entry that keeps the indices aligned when a crate cannot be read. It is
/// deliberately minimal rather than absent: rust-analyzer tolerates a crate
/// it cannot open far better than it tolerates deps pointing one crate to the
/// left of where they should.
fn placeholder(entry: &crate::resolve::LockEntry) -> CrateEntry {
    CrateEntry {
        display_name: ident_of(entry),
        root_module: String::new(),
        edition: "2021".to_string(),
        deps: Vec::new(),
        cfg: Vec::new(),
        env: BTreeMap::new(),
        source: None,
        is_proc_macro: false,
        proc_macro_dylib_path: None,
        is_workspace_member: false,
    }
}

fn describe(
    third_party: &Path,
    lock_index: usize,
    subrepo: &str,
    entry: &crate::resolve::LockEntry,
    index: &BTreeMap<Key, usize>,
    idents: &BTreeMap<Key, String>,
) -> Result<CrateEntry> {
    // Everything but the build-script cfgs comes from the lock, so this works
    // inside a build sandbox where the crate sources are not staged. Reading
    // each subrepo's Cargo.toml instead looked fine from a shell at the repo
    // root and produced 196 placeholders when the rule ran it.
    anyhow::ensure!(
        !entry.root_module.is_empty(),
        "no root module recorded; re-run sync to refresh the lock"
    );
    let dir = third_party.join(subrepo);
    let root = dir.join(&entry.root_module);

    let ident = ident_of(entry);

    let mut cfg: Vec<String> = entry
        .features
        .iter()
        .map(|f| format!("feature=\"{}\"", f))
        .collect();
    // Only available once the crate has been built, and worth having when it
    // has: libc gates dozens of items on cfgs its build script sets.
    cfg.extend(buildscript_cfgs(&dir));

    let mut deps = Vec::new();
    for d in &entry.deps {
        let is_host = d.target_name.ends_with("_host");
        // A lock's deps name subrepos in the same lock, so the crate is
        // looked up in the lock it came from.
        let key = (lock_index, d.subrepo.clone(), is_host);
        if let Some(i) = index.get(&key) {
            // A dependent that renamed the crate writes the rename, and one
            // that did not writes the crate's own name. Neither is the
            // package name whenever a manifest sets [lib] name: the package
            // sha-1 builds sha1, and `extern crate sha1` finds nothing if the
            // dep is recorded as sha_1.
            let declared = d.name.replace('-', "_");
            let name = if declared != d.crate_name.replace('-', "_") {
                declared
            } else {
                idents.get(&key).cloned().unwrap_or(declared)
            };
            deps.push(DepEntry { krate: *i, name });
        }
    }

    let proc_macro_dylib_path = if entry.is_proc_macro {
        let tag = entry.version.replace(['.', '+'], "_");
        Some(rel(&dir.join(format!(
            "lib{}-{}{}",
            ident,
            tag,
            std::env::consts::DLL_SUFFIX
        ))))
    } else {
        None
    };

    // The env a crate reads about itself. The full set needs the manifest,
    // which is not staged here; these are the ones crates actually use, and
    // they are all in the lock.
    let mut env = BTreeMap::new();
    env.insert("CARGO_PKG_NAME".to_string(), entry.crate_name.clone());
    env.insert("CARGO_PKG_VERSION".to_string(), entry.version.clone());
    let mut parts = entry.version.split(['.', '+', '-']);
    for key in [
        "CARGO_PKG_VERSION_MAJOR",
        "CARGO_PKG_VERSION_MINOR",
        "CARGO_PKG_VERSION_PATCH",
    ] {
        env.insert(key.to_string(), parts.next().unwrap_or("0").to_string());
    }

    // A crate whose build script generated sources needs two things, and
    // neither works alone: OUT_DIR so `include!(concat!(env!("OUT_DIR"), ..))`
    // names the right file, and the directory claimed by the crate so
    // rust-analyzer will load it.
    let source = buildscript_out_dir(&dir).map(|out| {
        // OUT_DIR absolute, because `include!` concatenates it as a raw string
        // and resolves the result against the including file. Everything else
        // stays repo-relative, as the rest of the file is.
        env.insert("OUT_DIR".to_string(), out.display().to_string());
        CrateSource {
            include_dirs: vec![rel(&dir), under_repo(&out)],
            exclude_dirs: Vec::new(),
        }
    });

    Ok(CrateEntry {
        display_name: ident,
        root_module: rel(&root),
        edition: if entry.edition.is_empty() {
            "2021".to_string()
        } else {
            entry.edition.clone()
        },
        deps,
        cfg,
        env,
        source,
        is_proc_macro: entry.is_proc_macro,
        proc_macro_dylib_path,
        is_workspace_member: false,
    })
}

/// The cfgs a build script set, which are frequently the difference between
/// a crate that analyses and one that is half red. libc alone sets dozens.
/// Already parsed and written next to the crate by `build-script`, so this
/// reads rather than recomputes.
/// The name source imports the crate by, which is the `[lib] name` when a
/// manifest sets one and the package name otherwise. The compile path makes
/// the same choice in generate.rs; these two disagreeing is what makes a
/// crate build and not resolve.
fn ident_of(entry: &crate::resolve::LockEntry) -> String {
    if entry.lib_name.is_empty() {
        entry.crate_name.replace('-', "_")
    } else {
        entry.lib_name.replace('-', "_")
    }
}

fn buildscript_cfgs(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().and_then(|s| s.to_str()) != Some("buildscript") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&p) else {
            continue;
        };
        for line in text.lines() {
            if let Some(cfg) = line.strip_prefix("rustc-cfg=") {
                out.push(cfg.to_string());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Where a crate's build script wrote its generated sources, absolute.
///
/// The build script file records it relative to itself. The absolute path in
/// the comment beside it points into plz-out/tmp and is gone once the build
/// finishes, so it is the relative one that survives.
///
/// Absolute because `include!(concat!(env!("OUT_DIR"), "/x.rs"))` concatenates
/// a raw string and resolves the result against the including file. A relative
/// OUT_DIR produces a path relative to the crate's own source directory, which
/// is not where the generated code is.
fn buildscript_out_dir(dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().and_then(|s| s.to_str()) != Some("buildscript") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&p) else {
            continue;
        };
        for line in text.lines() {
            if let Some(rel) = line.strip_prefix("out-dir=") {
                if let Some(resolved) =
                    crate::compile::resolve_out_dir(Path::new(rel.trim()), Some(&p))
                {
                    return Some(resolved);
                }
            }
        }
    }
    None
}

/// The edition a sysroot crate declares, defaulting to the current one. Only
/// the `edition = "..."` line is wanted and a full TOML parse of the stdlib's
/// manifests is not, so this reads the line.
fn sysroot_edition(dir: &Path) -> String {
    let Ok(text) = std::fs::read_to_string(dir.join("Cargo.toml")) else {
        return "2024".to_string();
    };
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("edition") {
            if let Some(v) = rest.split('"').nth(1) {
                return v.to_string();
            }
        }
    }
    "2024".to_string()
}

/// Paths go out exactly as they came in, which means repo-relative, which
/// means the file has to sit at the repo root - rust-analyzer resolves them
/// against its working directory and that is where it is launched.
///
/// Deliberately not canonicalised. An absolute path would bake this machine's
/// checkout location into a build output, which is neither cacheable across
/// machines nor reproducible; verified that rust-analyzer accepts relative
/// ones before choosing this.
fn rel(p: &Path) -> String {
    p.display().to_string()
}

/// An absolute path inside the repo, said the way the rest of the file says
/// paths. Anything outside is left alone, since there is nothing to say it
/// relative to.
fn under_repo(p: &Path) -> String {
    if let Ok(cwd) = std::env::current_dir() {
        if let Ok(stripped) = p.strip_prefix(&cwd) {
            return stripped.display().to_string();
        }
    }
    p.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// std reaches hashbrown, and hashbrown reaches core and alloc, only
    /// through features. Skipping optional deps outright leaves hashbrown
    /// with no core, and `HashMap::new()` stops inferring in every crate in
    /// the repo.
    #[test]
    fn an_optional_dep_a_feature_turns_on_is_followed() {
        let manifest = crate::resolve::parse_manifest(
            br#"
[package]
name = "hashbrown"
version = "0.17.1"

[features]
rustc-dep-of-std = ["core", "alloc"]
with-serde = ["dep:serde_core"]

[dependencies]
core = { version = "1.0", optional = true, package = "rustc-std-workspace-core" }
alloc = { version = "1.0", optional = true, package = "rustc-std-workspace-alloc" }
serde_core = { version = "1.0", optional = true }
rayon = { version = "1.0", optional = true }
"#,
        )
        .unwrap();
        let on = expand_features(&manifest, &["rustc-dep-of-std".to_string()], false);
        assert!(optional_dep_enabled("core", &on, &manifest));
        assert!(optional_dep_enabled("alloc", &on, &manifest));
        // Named by no enabled feature, so still off.
        assert!(!optional_dep_enabled("rayon", &on, &manifest));
        // `dep:` names the dependency without being a cfg of its own.
        let serde_on = expand_features(&manifest, &["with-serde".to_string()], false);
        assert!(optional_dep_enabled("serde_core", &serde_on, &manifest));
        assert!(!serde_on.contains(&"dep:serde_core".to_string()));
    }

    /// Features are a closure, and `default` is in it unless opted out -
    /// getting this wrong silently under-describes a crate rather than
    /// failing, which is why it is asserted rather than eyeballed.
    #[test]
    fn features_close_over_the_table() {
        let manifest = crate::resolve::parse_manifest(
            br#"
[package]
name = "x"
version = "1.0.0"

[features]
default = ["a"]
a = ["b"]
b = []
c = ["other/thing"]
"#,
        )
        .unwrap();
        let mut with = expand_features(&manifest, &[], true);
        with.sort();
        assert_eq!(with, vec!["a", "b", "default"]);
        assert!(expand_features(&manifest, &[], false).is_empty());
        // A `crate/feature` entry enables something elsewhere, and is not a
        // cfg here.
        assert_eq!(
            expand_features(&manifest, &["c".to_string()], false),
            vec!["c"]
        );
    }

    /// Path dependencies climb out of their own directory, and the result is
    /// then made relative to sysroot_src - which cannot be done with `..`
    /// still in it.
    #[test]
    fn a_path_dependency_is_normalised_before_it_is_made_relative() {
        let src = Path::new("/src");
        assert_eq!(
            normalise(&Path::new("/src/std").join("../alloc")).unwrap(),
            Path::new("/src/alloc")
        );
        assert!(normalise(&src.join("core/./src/.."))
            .unwrap()
            .ends_with("core"));
    }

    /// A fragment from a subrepo describes its root relative to the repo it
    /// was declared in. Everything in the project file is relative to the
    /// repo it sits at the root of, so the two have to be reconciled or the
    /// crate points at a path that does not exist and fails silently.
    #[test]
    fn a_subrepo_fragment_is_rebased_onto_the_host_repo() {
        let mut fp = FirstParty {
            display_name: "greeter".to_string(),
            label: "//greeter:greeter".to_string(),
            root_module: "greeter/src/lib.rs".to_string(),
            edition: "2021".to_string(),
            features: Vec::new(),
            manifest: Some("greeter/Cargo.toml".to_string()),
            is_proc_macro: false,
            deps: Vec::new(),
        };
        rebase(&mut fp, "plz-out/subrepos/plugins/rust");
        assert_eq!(
            fp.root_module,
            "plz-out/subrepos/plugins/rust/greeter/src/lib.rs"
        );
        // The manifest is a path too, and CARGO_PKG_* comes from it.
        assert_eq!(
            fp.manifest.as_deref(),
            Some("plz-out/subrepos/plugins/rust/greeter/Cargo.toml")
        );
    }

    /// A crate in the host repo is its own rebase, so the same path in means
    /// the same path out and no prefix is invented.
    #[test]
    fn rebasing_a_host_repo_fragment_changes_nothing() {
        let mut fp = FirstParty {
            display_name: "greeter".to_string(),
            label: "//greeter:greeter".to_string(),
            root_module: "greeter/src/lib.rs".to_string(),
            edition: "2021".to_string(),
            features: Vec::new(),
            manifest: Some("greeter/Cargo.toml".to_string()),
            is_proc_macro: false,
            deps: Vec::new(),
        };
        rebase(&mut fp, "");
        assert_eq!(fp.root_module, "greeter/src/lib.rs");
        assert_eq!(fp.manifest.as_deref(), Some("greeter/Cargo.toml"));
    }

    /// A first-party crate and a third-party crate can share a name - a corpus
    /// with a probe crate per crate it exercises has hundreds of collisions -
    /// and the probe depends on the crate it is named after. Matching deps by
    /// name makes it depend on itself, which rust-analyzer reports as a cycle
    /// and which drops the real dependency.
    #[test]
    fn a_crate_sharing_a_name_with_its_dependency_does_not_depend_on_itself() {
        let fp = FirstParty {
            display_name: "actix_http".to_string(),
            label: "//probes/actix_http:actix_http".to_string(),
            root_module: "probes/actix_http/lib.rs".to_string(),
            edition: "2021".to_string(),
            features: Vec::new(),
            manifest: None,
            is_proc_macro: false,
            deps: vec![FirstPartyDep {
                name: "actix_http".to_string(),
                label: "//third_party/crates:actix_http".to_string(),
            }],
        };
        // What run() keys on: the label, scoped by subrepo - not the name.
        let mut by_label: BTreeMap<(String, String), usize> = BTreeMap::new();
        by_label.insert((String::new(), fp.label.clone()), 7);
        assert!(by_label
            .get(&(String::new(), fp.deps[0].label.clone()))
            .is_none());
        // And the name alone would have matched, which is the bug.
        assert_eq!(fp.display_name, fp.deps[0].name);
    }

    /// rust-analyzer reads the discover protocol as JSONL and keys on `kind`.
    /// The shape is the analyzer's, not ours, and it is provisional - so it is
    /// asserted here rather than left to be noticed when an editor silently
    /// ignores a line it cannot parse.
    #[test]
    fn discover_output_is_the_shape_rust_analyzer_reads() {
        let project = Project {
            sysroot: Some("plz-out/bin/rustc".to_string()),
            sysroot_src: None,
            sysroot_project: None,
            crates: Vec::new(),
        };
        let finished = serde_json::to_string(&DiscoverData::Finished {
            buildfile: "BUILD",
            project: &project,
        })
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&finished).unwrap();
        assert_eq!(v["kind"], "finished");
        assert_eq!(v["buildfile"], "BUILD");
        assert_eq!(v["project"]["sysroot"], "plz-out/bin/rustc");
        // One line: the reader is line-oriented, and a pretty-printed object
        // is a hundred lines of syntax error to it.
        assert!(!finished.contains('\n'));

        let failed = serde_json::to_string(&DiscoverData::Error {
            error: "no lock".to_string(),
            source: Some("BUILD".to_string()),
        })
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&failed).unwrap();
        assert_eq!(v["kind"], "error");
        assert_eq!(v["error"], "no lock");
    }

    /// A build script that generated sources records where it put them, and
    /// the crate reaches them through `include!(concat!(env!("OUT_DIR"), ..))`.
    /// Without OUT_DIR that include resolves to nothing, and in this repo it
    /// took 26 crates with it: proc-macro2 generates code that way, so every
    /// `#[derive(Deserialize)]` downstream failed to infer.
    #[test]
    fn the_out_dir_a_build_script_wrote_to_is_read_back() {
        let dir = std::env::temp_dir().join(format!("please_rust_outdir_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("crunchy_out")).unwrap();
        std::fs::write(
            dir.join("crunchy.buildscript"),
            // The absolute path in the comment points into plz-out/tmp and is
            // gone after the build; the relative one is what survives.
            "# OUT_DIR=/somewhere/transient/crunchy_out
out-dir=crunchy_out
rustc-cfg=fake
",
        )
        .unwrap();

        let found = buildscript_out_dir(&dir).expect("out-dir should be read");
        assert!(found.is_absolute(), "OUT_DIR must be absolute: {:?}", found);
        assert!(found.ends_with("crunchy_out"), "{:?}", found);

        // Reading the out dir must not disturb the cfgs beside it.
        assert_eq!(buildscript_cfgs(&dir), vec!["fake".to_string()]);

        // A crate with no build script has no out dir, rather than a wrong one.
        let plain = dir.join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        assert!(buildscript_out_dir(&plain).is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn buildscript_cfgs_are_read_not_recomputed() {
        let dir = std::env::temp_dir().join(format!("please_rust_ide_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("libc.buildscript"),
            "# comment\nout-dir=libc_out\nrustc-cfg=freebsd11\nrustc-cfg=libc_union\nmetadata=x=y\n",
        )
        .unwrap();
        assert_eq!(
            buildscript_cfgs(&dir),
            vec!["freebsd11".to_string(), "libc_union".to_string()]
        );
        // A directory with no build script is the common case, not an error.
        let empty = dir.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(buildscript_cfgs(&empty).is_empty());
    }

    /// Every crate occupies an index whether or not it could be read,
    /// because deps address crates positionally - one absent entry would
    /// silently repoint every dep after it.
    #[test]
    fn an_unreadable_crate_still_takes_its_index() {
        let entry = crate::resolve::LockEntry {
            crate_name: "broken-crate".to_string(),
            version: "1.0.0".to_string(),
            ..Default::default()
        };
        let p = placeholder(&entry);
        assert_eq!(p.display_name, "broken_crate");
        assert!(p.root_module.is_empty());
    }

    /// A repo can declare third-party crates in more than one place, and each
    /// rust_resolve produces its own lock. Two locks can hold a crate of the
    /// same name, so a dep's label picks the lock by the package that
    /// declares it.
    #[test]
    fn a_dep_finds_the_lock_its_package_declares() {
        let (pkg, dir, path) = parse_lock_spec(
            "test/patch=plz-out/gen/test/patch=/tmp/l.json",
            Path::new("fb"),
        );
        assert_eq!(pkg, "test/patch");
        assert_eq!(dir, PathBuf::from("plz-out/gen/test/patch"));
        assert_eq!(path, PathBuf::from("/tmp/l.json"));

        // The old single-lock form takes its directory from the flag.
        let (pkg, dir, path) = parse_lock_spec("/tmp/only.json", Path::new("fallback/dir"));
        assert!(pkg.is_empty());
        assert_eq!(dir, PathBuf::from("fallback/dir"));
        assert_eq!(path, PathBuf::from("/tmp/only.json"));

        assert_eq!(
            label_package("//third_party/crates:serde"),
            "third_party/crates"
        );
        assert_eq!(label_package("//test/patch:patched"), "test/patch");

        let sources = vec![
            LockSource {
                package: "third_party/crates".to_string(),
                third_party_dir: PathBuf::from("plz-out/gen/third_party/crates"),
                lock: Default::default(),
            },
            LockSource {
                package: "test/patch".to_string(),
                third_party_dir: PathBuf::from("plz-out/gen/test/patch"),
                lock: Default::default(),
            },
        ];
        assert_eq!(lock_for_label(&sources, "//test/patch:patched"), Some(1));
        assert_eq!(
            lock_for_label(&sources, "//third_party/crates:serde"),
            Some(0)
        );
        // A dep in a package no lock declares belongs to no lock, which is
        // what the warning is about.
        assert_eq!(lock_for_label(&sources, "//elsewhere:thing"), None);

        // One lock in the old form answers for everything, as it always has.
        let legacy = vec![LockSource {
            package: String::new(),
            third_party_dir: PathBuf::from("plz-out/gen/third_party/crates"),
            lock: Default::default(),
        }];
        assert_eq!(lock_for_label(&legacy, "//anywhere:thing"), Some(0));
    }

    /// A manifest that sets `[lib] name` renames the crate without renaming
    /// the package, and source imports the lib name. The compile path has
    /// always known this; the project file did not, so `extern crate sha1`
    /// against the package sha-1 built and did not resolve.
    #[test]
    fn a_crate_is_named_the_way_source_imports_it() {
        let package_only = crate::resolve::LockEntry {
            crate_name: "sha-1".to_string(),
            ..Default::default()
        };
        assert_eq!(ident_of(&package_only), "sha_1");

        let with_lib = crate::resolve::LockEntry {
            crate_name: "sha-1".to_string(),
            lib_name: "sha1".to_string(),
            ..Default::default()
        };
        assert_eq!(ident_of(&with_lib), "sha1");
        assert_eq!(placeholder(&with_lib).display_name, "sha1");

        // rustls-webpki builds webpki, and md-5 builds md5.
        for (package, lib, want) in [
            ("rustls-webpki", "webpki", "webpki"),
            ("md-5", "md5", "md5"),
            (
                "new_debug_unreachable",
                "debug_unreachable",
                "debug_unreachable",
            ),
            // The overwhelming majority: no [lib] name, so nothing recorded.
            ("serde", "", "serde"),
        ] {
            let e = crate::resolve::LockEntry {
                crate_name: package.to_string(),
                lib_name: lib.to_string(),
                ..Default::default()
            };
            assert_eq!(ident_of(&e), want, "{}", package);
        }
    }
}
