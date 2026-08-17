//! Maintain the rust_repo declarations and the resolved lock file.
//!
//! This is the puku analog for Rust: it owns the machine-maintained parts of
//! the third-party BUILD file. It can import a cargo-generated Cargo.lock to
//! add missing crates (with sha256 hashes from the lockfile's checksums, so
//! downloads verify hermetically), normalizes subrepo naming (plain crate
//! name for the newest declared version, `crate-x.y.z` for older duplicates),
//! fetches any missing crate tarballs via plz, and regenerates rust.lock via
//! the resolver. Network is only ever touched by plz's own download rules.

use anyhow::{bail, Context, Result};
use clap::Args;
use semver::Version;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::resolve::{resolve_entries, EntryInput};

#[derive(Args)]
pub struct SyncArgs {
    /// The third-party BUILD file containing rust_repo declarations
    #[arg(long, default_value = "third_party/rust/BUILD")]
    pub build_file: PathBuf,

    /// Third-party folder (package path of the BUILD file)
    #[arg(long, default_value = "third_party/rust")]
    pub third_party_folder: String,

    /// Directory containing extracted crates ({crate}-{version}/Cargo.toml)
    #[arg(long)]
    pub crate_store: Option<PathBuf>,

    /// A cargo-generated Cargo.lock to import: missing crates are added as
    /// indirect deps with hashes from the lockfile checksums
    #[arg(long)]
    pub import: Option<PathBuf>,

    /// A cargo workspace to import wholesale: writes a BUILD file next to
    /// every member (rust_library/rust_binary/rust_test), scaffolds the
    /// third-party BUILD if missing, and imports the workspace's Cargo.lock
    #[arg(long)]
    pub import_workspace: Option<PathBuf>,

    /// Target triple to resolve for
    #[arg(long, default_value_t = crate::build_script::running_triple())]
    pub target: String,

    /// Triples the declaration set must cover, comma-separated. Declarations
    /// are shared by everyone building the repo, so they have to name every
    /// crate any of those platforms needs; resolution itself still happens
    /// per-host, in the build graph.
    #[arg(long, default_value = "x86_64-unknown-linux-gnu,aarch64-unknown-linux-gnu,aarch64-apple-darwin,x86_64-apple-darwin")]
    pub targets: String,

    /// Where to write the resolved lock (defaults to rust.lock next to the BUILD file)
    #[arg(long)]
    pub lock_output: Option<PathBuf>,

    /// plz binary used to fetch missing crate downloads ("" disables)
    #[arg(long, default_value = "plz")]
    pub plz: String,

    /// Keep existing subrepo names instead of normalizing them
    #[arg(long)]
    pub no_rename: bool,

    /// Drop indirect declarations that no direct dependency activates
    #[arg(long)]
    pub prune: bool,

    /// Report what would change without writing anything
    #[arg(long)]
    pub dry_run: bool,
}

/// The triples a declaration set has to cover, always including the one
/// resolution is primarily for.
fn target_list(targets: &str, primary: &str) -> Vec<String> {
    let mut out: Vec<String> = targets
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if !out.iter().any(|t| t == primary) {
        out.insert(0, primary.to_string());
    }
    out
}

/// Scaffolds a minimal third-party BUILD (toolchain + rust_repo subinclude)
/// for a repo that doesn't have one yet, so a workspace import is a single
/// command on a fresh cargo repo.
fn scaffold_third_party_build(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        path,
        r#"subinclude("///rust//build_defs:rust")

rust_toolchain(
    name = "toolchain",
    version = "1.97.1",
    hashes = ["b4cdbc7cc6b0ee0a2666b1872769fdb2ad8393b28b63952f6493b4b400e4832b"],
    visibility = ["PUBLIC"],
)

subinclude("///rust//build_defs:rust_repo")
"#,
    )
    .with_context(|| format!("Failed to write {}", path.display()))?;
    eprintln!("import-workspace: scaffolded {}", path.display());
    Ok(())
}

/// Scaffolds .plzconfig and plugins/BUILD for a repo that has neither, so
/// `sync --import-workspace` on a bare cargo repo leaves `plz build //...`
/// one config-review away from working.
fn scaffold_plz_repo() -> Result<()> {
    if !Path::new(".plzconfig").exists() {
        fs::write(
            ".plzconfig",
            r#"[please]
version = 17.27.0

[Parse]
BlacklistDirs = target

[Plugin "rust"]
Target = //plugins:rust

; plz only aggregates coverage for known file extensions; .rs is not in
; its default list
[cover]
FileExtension = .rs
"#,
        )
        .context("Failed to write .plzconfig")?;
        eprintln!("import-workspace: scaffolded .plzconfig");
    }
    if !Path::new("plugins/BUILD").exists() {
        fs::create_dir_all("plugins")?;
        fs::write(
            "plugins/BUILD",
            r#"plugin_repo(
    name = "rust",
    owner = "becomeliminal",
    plugin = "rust-rules",
    revision = "master",  # pin to a release tag
)
"#,
        )
        .context("Failed to write plugins/BUILD")?;
        eprintln!("import-workspace: scaffolded plugins/BUILD");
    }
    Ok(())
}

/// One rust_repo declaration, as parsed from the BUILD file.
#[derive(Clone)]
struct Decl {
    name: Option<String>,
    crate_name: String,
    version: String,
    features: Vec<String>,
    hashes: Vec<String>,
    /// Raw arg lines we don't manage (install, visibility, ...), re-emitted verbatim
    passthrough: Vec<String>,
    /// Comment lines directly above the block
    leading_comments: Vec<String>,
    /// Line span [start, end] of the block in the original file (0-based, inclusive)
    span: Option<(usize, usize)>,
    /// True if this entry was added by --import this run
    imported: bool,
    /// True if this crate is a direct dependency (its features seed resolution)
    root: bool,
    /// Cargo semantics: roots enable default features unless opted out
    default_features: bool,
    /// Git forge source (owner/repo, revision) instead of crates.io
    git_repo: String,
    git_revision: String,
}

impl Decl {
    fn subrepo(&self) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| self.crate_name.replace('-', "_"))
    }
}

pub fn run(args: SyncArgs) -> Result<()> {
    run_reporting(args).map(|_| ())
}

/// sync, additionally reporting dependencies resolution wanted but which are
/// not declared. `lock` uses these to heal the declaration set.
pub fn run_reporting(args: SyncArgs) -> Result<Vec<crate::resolve::MissingDep>> {
    let mut args = args;

    // Workspace import: emit first-party BUILD files, scaffold the
    // third-party BUILD if this is a fresh repo, and chain the workspace's
    // Cargo.lock into the ordinary lockfile import below.
    if let Some(ws) = &args.import_workspace {
        let result = crate::workspace::import_workspace(ws, &args.third_party_folder)?;
        eprintln!(
            "import-workspace: {} members, {} BUILD files written",
            result.members, result.written
        );
        if !args.build_file.exists() {
            scaffold_third_party_build(&args.build_file)?;
        }
        scaffold_plz_repo()?;
        if args.import.is_none() {
            if let Some(lock) = result.lockfile {
                eprintln!("import-workspace: importing {}", lock.display());
                args.import = Some(lock);
            } else {
                eprintln!(
                    "import-workspace: no Cargo.lock found; declare third-party crates with `lock --add`"
                );
            }
        }
    }

    let build_text = fs::read_to_string(&args.build_file)
        .with_context(|| format!("Failed to read {}", args.build_file.display()))?;
    let lines: Vec<String> = build_text.lines().map(|s| s.to_string()).collect();

    let mut decls = parse_build(&lines)?;
    eprintln!("sync: {} rust_repo declarations parsed", decls.len());

    // Import a cargo lockfile: add anything not yet declared, and attach
    // hashes to existing entries that lack them.
    if let Some(import) = &args.import {
        import_cargo_lock(import, &mut decls)?;
    }

    // Naming normalization: newest declared version of a crate gets the plain
    // normalized name; older duplicates get `crate_norm-x.y.z`.
    let mut renames: BTreeMap<String, String> = BTreeMap::new();
    if !args.no_rename {
        renames = normalize_names(&mut decls)?;
        for (old, new) in &renames {
            eprintln!("sync: rename {} -> {}", old, new);
        }
    }

    // Make sure every crate's manifest is available, fetching via plz if allowed.
    let crate_store = args.crate_store.clone().unwrap_or_else(|| {
        repo_root(&args.build_file)
            .join("plz-out/gen")
            .join(&args.third_party_folder)
    });
    ensure_manifests(&args, &crate_store, &decls)?;

    // Resolve the graph.
    let entries: Vec<EntryInput> = decls
        .iter()
        .map(|d| EntryInput {
            subrepo: d.subrepo(),
            crate_name: d.crate_name.clone(),
            version: d.version.clone(),
            manifest: crate_store
                .join(format!("{}-{}", d.crate_name, d.version))
                .join("Cargo.toml"),
            features: d.features.clone(),
            root: d.root,
            default_features: d.default_features,
        })
        .collect();
    let mut lock = resolve_entries(&entries, &args.target)?;
    let mut missing_deps = lock.missing.clone();

    // Crates imported this run that did not activate for this target (e.g.
    // windows-only) are dropped again rather than declared dead weight; with
    // --prune, ALL inactive indirect declarations go.
    // A crate needed only on darwin is not dead weight on linux, so activity
    // is judged across every covered platform rather than this one.
    let mut active_anywhere: BTreeSet<String> = lock.crates.keys().cloned().collect();
    active_anywhere.extend(lock.host_crates.keys().cloned());
    for triple in target_list(&args.targets, &args.target) {
        if triple == args.target {
            continue;
        }
        match resolve_entries(&entries, &triple) {
            Ok(other) => {
                active_anywhere.extend(other.crates.keys().cloned());
                active_anywhere.extend(other.host_crates.keys().cloned());
            }
            Err(e) => eprintln!("sync: could not resolve for {}: {:#}", triple, e),
        }
    }

    let before = decls.len();
    let mut deleted_spans: Vec<(usize, usize)> = Vec::new();
    decls.retain(|d| {
        let active = active_anywhere.contains(&d.subrepo());
        let keep = if d.imported {
            active
        } else if args.prune && !d.root {
            active
        } else {
            true
        };
        if !keep {
            if let Some(span) = d.span {
                deleted_spans.push(span);
            }
            eprintln!("sync: - {} {}@{}", d.subrepo(), d.crate_name, d.version);
        }
        keep
    });
    if decls.len() != before {
        eprintln!(
            "sync: dropped {} imported crates that are unused on {}",
            before - decls.len(),
            args.target
        );
        let entries: Vec<EntryInput> = decls
            .iter()
            .map(|d| EntryInput {
                subrepo: d.subrepo(),
                crate_name: d.crate_name.clone(),
                version: d.version.clone(),
                manifest: crate_store
                    .join(format!("{}-{}", d.crate_name, d.version))
                    .join("Cargo.toml"),
                features: d.features.clone(),
                root: d.root,
                default_features: d.default_features,
            })
            .collect();
        lock = resolve_entries(&entries, &args.target)?;
        missing_deps = lock.missing.clone();
    }

    // Resolution lives in the build graph: sync maintains a rust_resolve
    // rule (same //third_party/rust:rust_lock label) instead of committing a
    // derived lock file. In-process resolution above exists only to validate
    // the graph and drive pruning.
    let new_build = write_resolve_block(&rewrite_build(&lines, &decls, &deleted_spans), &decls, &args.target);

    if args.dry_run {
        eprintln!(
            "sync (dry run): would write {} declarations, {} resolved crates",
            decls.len(),
            lock.crates.len()
        );
        return Ok(missing_deps);
    }

    fs::write(&args.build_file, new_build)
        .with_context(|| format!("Failed to write {}", args.build_file.display()))?;
    // A stale committed lock file from older revisions is superseded
    let old_lock = args.build_file.parent().unwrap_or(Path::new(".")).join("rust.lock");
    if old_lock.exists() {
        let _ = fs::remove_file(&old_lock);
        eprintln!("sync: removed stale {} (resolution now happens in the build graph)", old_lock.display());
    }
    // Declarations nothing reaches are built standalone with default
    // features, which is rarely what was meant: name them rather than
    // leaving the surprise to surface as a compile error inside an
    // unrelated crate.
    let unreachable: Vec<String> = decls
        .iter()
        .map(|d| d.subrepo())
        .filter(|s| !lock.crates.contains_key(s) && !lock.host_crates.contains_key(s))
        .collect();
    if !unreachable.is_empty() {
        let shown: Vec<&str> = unreachable.iter().take(5).map(|s| s.as_str()).collect();
        eprintln!(
            "sync: {} declarations are not reachable from any root and will build standalone with default features: {}{}. Run sync --prune to drop them.",
            unreachable.len(),
            shown.join(", "),
            if unreachable.len() > 5 { ", ..." } else { "" },
        );
    }
    eprintln!(
        "sync: wrote {} declarations to {} ({} crates resolve)",
        decls.len(),
        args.build_file.display(),
        lock.crates.len(),
    );
    Ok(missing_deps)
}

/// Rewrite (or append) the rust_resolve block encoding the declared graph.
fn write_resolve_block(build: &str, decls: &[Decl], target: &str) -> String {
    let mut block = String::new();
    block.push_str("# Machine-maintained by please_rust sync; resolution runs in the build graph.\n");
    block.push_str("rust_resolve(\n    name = \"rust_lock\",\n");
    // No target: the rule derives the host's, so the same declarations
    // resolve correctly for linux and mac developers alike. sync --target
    // still resolves for whatever was asked, it just is not written here.
    let _ = target;
    block.push_str("    entries = [\n");
    for d in decls {
        let features = if d.root { d.features.join(",") } else { String::new() };
        block.push_str(&format!(
            "        \"{}|{}|{}|{}|{}|{}\",\n",
            d.subrepo(),
            d.crate_name,
            d.version,
            features,
            d.root,
            d.default_features,
        ));
    }
    block.push_str("    ],\n)\n");

    // Replace an existing block (from "rust_resolve(" to its closing line) or append.
    let lines: Vec<&str> = build.lines().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut replaced = false;
    while i < lines.len() {
        let t = lines[i].trim_start();
        if !replaced && (t.starts_with("rust_resolve(") || (t.starts_with('#') && t.contains("Machine-maintained by please_rust sync"))) {
            // Skip the marker comment plus the block
            let mut j = i;
            while j < lines.len() && lines[j].trim_start().starts_with('#') {
                j += 1;
            }
            let mut depth = 0i32;
            loop {
                depth += lines[j].matches('(').count() as i32;
                depth -= lines[j].matches(')').count() as i32;
                j += 1;
                if depth == 0 || j >= lines.len() {
                    break;
                }
            }
            out.push_str(&block);
            i = j;
            replaced = true;
        } else {
            out.push_str(lines[i]);
            out.push('\n');
            i += 1;
        }
    }
    if !replaced {
        out.push('\n');
        out.push_str(&block);
    }
    out
}

/// Walk up from the BUILD file to the directory containing .plzconfig.
fn repo_root(build_file: &Path) -> PathBuf {
    let abs = build_file
        .canonicalize()
        .unwrap_or_else(|_| build_file.to_path_buf());
    let mut dir = abs.parent().unwrap_or(Path::new(".")).to_path_buf();
    loop {
        if dir.join(".plzconfig").exists() {
            return dir;
        }
        if !dir.pop() {
            return PathBuf::from(".");
        }
    }
}

const MANAGED_KEYS: &[&str] = &["name", "crate", "version", "features", "hashes", "dep_overrides", "indirect", "default_features", "git_repo", "git_revision"];

fn parse_build(lines: &[String]) -> Result<Vec<Decl>> {
    let mut decls = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim_start();
        if !trimmed.starts_with("rust_repo(") {
            i += 1;
            continue;
        }
        let start = i;
        // Contiguous comment lines directly above belong to this block
        let mut comment_start = start;
        while comment_start > 0 && lines[comment_start - 1].trim_start().starts_with('#') {
            comment_start -= 1;
        }
        let leading_comments: Vec<String> = lines[comment_start..start].to_vec();

        // Scan to the balanced close, ignoring comment lines
        let mut depth = 0i32;
        let mut end = start;
        let mut body = String::new();
        for (j, line) in lines.iter().enumerate().skip(start) {
            let t = line.trim_start();
            if !t.starts_with('#') {
                depth += line.matches('(').count() as i32;
                depth -= line.matches(')').count() as i32;
                body.push_str(line);
                body.push('\n');
            }
            if depth == 0 && j > start {
                end = j;
                break;
            }
            if depth == 0 && j == start && line.contains(')') {
                end = j;
                break;
            }
        }
        if depth != 0 {
            bail!("Unbalanced rust_repo( block starting at line {}", start + 1);
        }

        let get = |key: &str| -> Option<String> {
            let pat = format!("{} = \"", key);
            let idx = body.find(&pat)?;
            let rest = &body[idx + pat.len()..];
            rest.split('"').next().map(|s| s.to_string())
        };
        let getlist = |key: &str| -> Vec<String> {
            let pat = format!("{} = [", key);
            match body.find(&pat) {
                None => vec![],
                Some(idx) => {
                    let rest = &body[idx + pat.len()..];
                    let inner = rest.split(']').next().unwrap_or("");
                    inner
                        .split('"')
                        .skip(1)
                        .step_by(2)
                        .map(|s| s.to_string())
                        .collect()
                }
            }
        };

        let crate_name = get("crate")
            .with_context(|| format!("rust_repo at line {} missing crate", start + 1))?;
        let version = get("version")
            .with_context(|| format!("rust_repo at line {} missing version", start + 1))?;

        // Preserve args we don't manage, verbatim (install, visibility, ...)
        let mut passthrough = Vec::new();
        for line in &lines[start + 1..end] {
            let t = line.trim_start();
            if t.starts_with('#') {
                continue;
            }
            if let Some(eq) = t.find('=') {
                let key = t[..eq].trim();
                if key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_')
                    && !key.is_empty()
                    && !MANAGED_KEYS.contains(&key)
                {
                    passthrough.push(line.trim_end().trim_end_matches(',').to_string());
                }
            }
        }

        let indirect = body.contains("indirect = True");
        let no_default = body.contains("default_features = False");
        let name = get("name");
        decls.push(Decl {
            root: !indirect,
            default_features: !no_default,
            git_repo: get("git_repo").unwrap_or_default(),
            git_revision: get("git_revision").unwrap_or_default(),
            name,
            crate_name,
            version,
            features: getlist("features"),
            hashes: getlist("hashes"),
            passthrough,
            leading_comments,
            span: Some((comment_start, end)),
            imported: false,
        });
        i = end + 1;
    }
    Ok(decls)
}

fn import_cargo_lock(path: &Path, decls: &mut Vec<Decl>) -> Result<()> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let doc: toml::Value = content
        .parse()
        .with_context(|| format!("Failed to parse {}", path.display()))?;

    let declared: BTreeSet<(String, String)> = decls
        .iter()
        .map(|d| (d.crate_name.clone(), d.version.clone()))
        .collect();

    let packages = doc
        .get("package")
        .and_then(|p| p.as_array())
        .context("Cargo.lock has no [[package]] entries")?;

    let mut added = 0;
    for pkg in packages {
        let name = pkg.get("name").and_then(|v| v.as_str()).unwrap_or_default();
        let version = pkg
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let source = pkg.get("source").and_then(|v| v.as_str()).unwrap_or("");
        // Registry crates fetch from crates.io; git+ sources from a forge
        // archive (github-style /archive/ URLs) when the host supports it.
        let (mut git_repo, mut git_revision) = (String::new(), String::new());
        if let Some(rest) = source.strip_prefix("git+") {
            let (url, frag) = rest.split_once('#').unwrap_or((rest, ""));
            let url = url.split('?').next().unwrap_or(url);
            if let Some(path) = url.strip_prefix("https://github.com/") {
                git_repo = path.trim_end_matches(".git").to_string();
                git_revision = frag.to_string();
            } else {
                eprintln!(
                    "warning: {} uses a non-github git source ({}); declare it manually with rust_repo(download = ...)",
                    name, url
                );
                continue;
            }
            if git_revision.is_empty() {
                eprintln!("warning: {} git source has no pinned revision, skipping", name);
                continue;
            }
        } else if name.is_empty() || !source.contains("registry") {
            continue;
        }
        let checksum = pkg
            .get("checksum")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        if declared.contains(&(name.to_string(), version.to_string())) {
            // Attach the hash to an existing entry that lacks one
            if let Some(sum) = &checksum {
                for d in decls.iter_mut() {
                    if d.crate_name == name && d.version == version && d.hashes.is_empty() {
                        d.hashes = vec![sum.clone()];
                    }
                }
            }
            continue;
        }

        decls.push(Decl {
            name: None,
            crate_name: name.to_string(),
            version: version.to_string(),
            features: vec![],
            hashes: checksum.into_iter().collect(),
            passthrough: vec![],
            leading_comments: vec![],
            span: None,
            imported: true,
            root: false,
            default_features: true,
            git_repo: git_repo.clone(),
            git_revision: git_revision.clone(),
        });
        added += 1;
    }
    eprintln!("sync: imported {} new crates from {}", added, path.display());

    // The lockfile has no feature information; the workspace manifest next to
    // it declares the direct deps and their feature requests (cargo
    // semantics: listed features, plus default unless disabled).
    let manifest_path = path.parent().unwrap_or(Path::new(".")).join("Cargo.toml");
    if manifest_path.exists() {
        let mcontent = fs::read(&manifest_path)?;
        if let Ok(manifest) = crate::resolve::parse_manifest(&mcontent) {
            for (name, dep) in &manifest.dependencies {
                let package = dep.package().unwrap_or(name).to_string();
                let req = semver::VersionReq::parse(dep.req()).ok();
                let mut feats: Vec<String> = dep
                    .detail()
                    .map(|dd| dd.features.clone())
                    .unwrap_or_default();
                let default_on = dep.detail().map(|dd| dd.default_features).unwrap_or(true);
                if default_on {
                    feats.push("default".to_string());
                }
                for d in decls.iter_mut() {
                    if d.crate_name != package {
                        continue;
                    }
                    let matches = match (&req, Version::parse(&d.version)) {
                        (Some(r), Ok(v)) => r.matches(&v),
                        _ => true,
                    };
                    if matches && !d.root {
                        d.root = true;
                        for f in &feats {
                            if !d.features.contains(f) {
                                d.features.push(f.clone());
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// Newest declared version of a crate gets the plain normalized name; older
/// duplicates get `crate_norm-x.y.z`. Returns old->new subrepo renames.
fn normalize_names(decls: &mut [Decl]) -> Result<BTreeMap<String, String>> {
    let mut by_crate: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, d) in decls.iter().enumerate() {
        by_crate.entry(d.crate_name.clone()).or_default().push(i);
    }

    let mut renames = BTreeMap::new();
    for (crate_name, idxs) in &by_crate {
        let norm = crate_name.replace('-', "_");
        let mut versions: Vec<(Version, usize)> = Vec::new();
        for &i in idxs {
            let v = Version::parse(&decls[i].version)
                .with_context(|| format!("Bad version {} for {}", decls[i].version, crate_name))?;
            versions.push((v, i));
        }
        versions.sort_by(|a, b| b.0.cmp(&a.0));
        for (rank, (_, i)) in versions.iter().enumerate() {
            let new_name = if rank == 0 {
                norm.clone()
            } else {
                format!("{}-{}", norm, decls[*i].version)
            };
            let old = decls[*i].subrepo();
            if old != new_name {
                renames.insert(old, new_name.clone());
            }
            decls[*i].name = Some(new_name);
        }
    }
    Ok(renames)
}

fn ensure_manifests(args: &SyncArgs, crate_store: &Path, decls: &[Decl]) -> Result<()> {
    let missing: Vec<&Decl> = decls
        .iter()
        .filter(|d| {
            !crate_store
                .join(format!("{}-{}", d.crate_name, d.version))
                .join("Cargo.toml")
                .exists()
        })
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    if args.plz.is_empty() {
        bail!(
            "{} crates are not downloaded (e.g. {}-{}) and --plz is disabled",
            missing.len(),
            missing[0].crate_name,
            missing[0].version
        );
    }
    // The download targets must exist in the BUILD file before plz can build
    // them, so write an interim BUILD including the new declarations first.
    let build_text = fs::read_to_string(&args.build_file)?;
    let lines: Vec<String> = build_text.lines().map(|s| s.to_string()).collect();
    fs::write(&args.build_file, rewrite_build(&lines, decls, &[]))?;

    let targets: Vec<String> = missing
        .iter()
        .map(|d| format!("//{}:_{}#download", args.third_party_folder, d.subrepo()))
        .collect();
    eprintln!("sync: fetching {} missing crates via plz", targets.len());
    let status = Command::new(&args.plz)
        .arg("build")
        .args(&targets)
        .current_dir(repo_root(&args.build_file))
        .status()
        .with_context(|| format!("Failed to run {}", args.plz))?;
    if !status.success() {
        bail!("plz build of missing downloads failed");
    }
    Ok(())
}

/// Replace every parsed block with its canonical form in place; append
/// imported entries at the end; drop blocks whose declarations were removed.
fn rewrite_build(lines: &[String], decls: &[Decl], deleted: &[(usize, usize)]) -> String {
    // Map from original start line -> canonical replacement text
    let mut replacements: BTreeMap<usize, (usize, String)> = BTreeMap::new();
    let mut appended = String::new();

    for &(start, end) in deleted {
        replacements.insert(start, (end, String::new()));
    }
    for d in decls {
        match d.span {
            Some((start, end)) => {
                replacements.insert(start, (end, emit_decl(d)));
            }
            None => {
                appended.push_str(&emit_decl(d));
                appended.push('\n');
            }
        }
    }

    let mut out = String::new();
    let mut i = 0;
    while i < lines.len() {
        if let Some((end, text)) = replacements.get(&i) {
            out.push_str(text);
            i = end + 1;
        } else {
            out.push_str(&lines[i]);
            out.push('\n');
            i += 1;
        }
    }

    if !appended.is_empty() {
        out.push_str("\n# Added by please_rust sync\n");
        out.push_str(&appended);
    }
    // Single trailing newline
    while out.ends_with("\n\n") {
        out.pop();
    }
    out
}

fn emit_decl(d: &Decl) -> String {
    let mut s = String::new();
    for c in &d.leading_comments {
        s.push_str(c);
        s.push('\n');
    }
    s.push_str("rust_repo(\n");
    s.push_str(&format!("    name = \"{}\",\n", d.subrepo()));
    s.push_str(&format!("    crate = \"{}\",\n", d.crate_name));
    s.push_str(&format!("    version = \"{}\",\n", d.version));
    if d.root && !d.features.is_empty() {
        let feats: Vec<String> = d.features.iter().map(|f| format!("\"{}\"", f)).collect();
        s.push_str(&format!("    features = [{}],\n", feats.join(", ")));
    }
    if !d.hashes.is_empty() {
        let hs: Vec<String> = d.hashes.iter().map(|h| format!("\"{}\"", h)).collect();
        s.push_str(&format!("    hashes = [{}],\n", hs.join(", ")));
    }
    if !d.git_repo.is_empty() {
        s.push_str(&format!("    git_repo = \"{}\",\n", d.git_repo));
        s.push_str(&format!("    git_revision = \"{}\",\n", d.git_revision));
    }
    if !d.root {
        s.push_str("    indirect = True,\n");
    }
    if !d.default_features {
        s.push_str("    default_features = False,\n");
    }
    for p in &d.passthrough {
        s.push_str(&format!("    {},\n", p.trim()));
    }
    s.push_str(")\n");
    s
}

// ---------------------------------------------------------------------------
// please_rust lock: hermetic version resolution over the crates.io sparse
// index. Network happens only here, at lock time (the `go mod tidy` moment);
// index fetches shell out to curl so the tool itself needs no TLS stack
// (ureq/reqwest would pull in ring's C build scripts). Resolution is greedy
// max-satisfying with a preference for already-declared versions, erroring
// clearly on conflicts; a backtracking (PubGrub) solver can replace select()
// without changing anything else.
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct LockCmdArgs {
    /// The third-party BUILD file containing rust_repo declarations
    #[arg(long, default_value = "third_party/rust/BUILD")]
    pub build_file: PathBuf,

    /// Third-party folder (package path of the BUILD file)
    #[arg(long, default_value = "third_party/rust")]
    pub third_party_folder: String,

    /// Add a direct dependency: crate@req (e.g. serde@1, hex@0.4.3)
    #[arg(long = "add")]
    pub add: Vec<String>,

    /// Sparse index URL
    #[arg(long, default_value = "https://index.crates.io")]
    pub index_url: String,

    /// Index cache directory (default ~/.cache/please_rust/index)
    #[arg(long)]
    pub cache_dir: Option<PathBuf>,

    /// Only use the index cache, never the network
    #[arg(long)]
    pub offline: bool,

    /// Target triple
    #[arg(long, default_value_t = crate::build_script::running_triple())]
    pub target: String,

    /// Triples the declaration set must cover, comma-separated. Declarations
    /// are shared by everyone building the repo, so they have to name every
    /// crate any of those platforms needs; resolution itself still happens
    /// per-host, in the build graph.
    #[arg(long, default_value = "x86_64-unknown-linux-gnu,aarch64-unknown-linux-gnu,aarch64-apple-darwin,x86_64-apple-darwin")]
    pub targets: String,

    /// curl binary for index fetches
    #[arg(long, default_value = "curl")]
    pub curl: String,

    /// plz binary used to fetch missing crate downloads ("" disables)
    #[arg(long, default_value = "plz")]
    pub plz: String,

    /// Use the greedy resolver instead of PubGrub backtracking
    #[arg(long)]
    pub greedy: bool,

    /// Ignore rust-version when selecting releases (MSRV filtering is on by
    /// default, using the toolchain declared in the third-party BUILD file)
    #[arg(long)]
    pub ignore_msrv: bool,

    /// Features to enable on the crates being added (comma-separated).
    /// Optional dependencies these turn on are declared automatically.
    #[arg(long)]
    pub features: Option<String>,
}

#[derive(serde::Deserialize, Clone)]
struct IndexDep {
    name: String,
    req: String,
    #[serde(default)]
    features: Vec<String>,
    #[serde(default)]
    optional: bool,
    #[serde(default = "default_true")]
    default_features: bool,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    package: Option<String>,
}

fn default_true() -> bool {
    true
}

#[derive(serde::Deserialize, Clone)]
struct IndexVersion {
    vers: String,
    #[serde(default)]
    deps: Vec<IndexDep>,
    cksum: String,
    #[serde(default)]
    features: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    features2: Option<BTreeMap<String, Vec<String>>>,
    #[serde(default)]
    yanked: bool,
    #[serde(default)]
    rust_version: Option<String>,
}

impl IndexVersion {
    fn all_features(&self) -> BTreeMap<String, Vec<String>> {
        let mut f = self.features.clone();
        if let Some(f2) = &self.features2 {
            for (k, v) in f2 {
                f.entry(k.clone()).or_default().extend(v.iter().cloned());
            }
        }
        f
    }
}

struct Index {
    url: String,
    cache_dir: PathBuf,
    offline: bool,
    curl: String,
    cache: std::cell::RefCell<BTreeMap<String, Vec<IndexVersion>>>,
}

impl Index {
    fn path_for(name: &str) -> String {
        let n = name.to_lowercase();
        match n.len() {
            1 => format!("1/{}", n),
            2 => format!("2/{}", n),
            3 => format!("3/{}/{}", &n[..1], n),
            _ => format!("{}/{}/{}", &n[..2], &n[2..4], n),
        }
    }

    fn versions(&self, name: &str) -> Result<Vec<IndexVersion>> {
        if let Some(v) = self.cache.borrow().get(name) {
            return Ok(v.clone());
        }
        let rel = Self::path_for(name);
        let cache_file = self.cache_dir.join(&rel);
        let content = if cache_file.exists() {
            fs::read_to_string(&cache_file)?
        } else if self.offline {
            bail!("{} not in index cache and --offline is set", name)
        } else {
            let url = format!("{}/{}", self.url, rel);
            let out = Command::new(&self.curl)
                .args(["--fail", "--silent", "--show-error", "--location", &url])
                .output()
                .with_context(|| format!("Failed to run {}", self.curl))?;
            if !out.status.success() {
                bail!(
                    "index fetch of {} failed: {}",
                    url,
                    String::from_utf8_lossy(&out.stderr)
                );
            }
            let text = String::from_utf8(out.stdout).context("index response not utf-8")?;
            if let Some(parent) = cache_file.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&cache_file, &text)?;
            text
        };
        let mut versions = Vec::new();
        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<IndexVersion>(line) {
                Ok(v) => versions.push(v),
                Err(e) => eprintln!("warning: bad index line for {}: {}", name, e),
            }
        }
        self.cache.borrow_mut().insert(name.to_string(), versions.clone());
        Ok(versions)
    }
}

impl crate::pubgrub_solver::ReleaseSource for IndexSource<'_> {
    fn releases(&self, name: &str) -> Result<Vec<crate::pubgrub_solver::Release>> {
        let versions = self.index.versions(name)?;
        if versions.is_empty() {
            bail!("{} has no releases in the index", name);
        }
        Ok(versions
            .iter()
            .filter_map(|iv| {
                let version = Version::parse(&iv.vers).ok()?;
                let activated = default_activated_deps(iv);
                Some(crate::pubgrub_solver::Release {
                    version,
                    cksum: iv.cksum.clone(),
                    yanked: iv.yanked,
                    rust_version: iv
                        .rust_version
                        .as_deref()
                        .and_then(parse_rust_version),
                    deps: iv
                        .deps
                        .iter()
                        .map(|d| crate::pubgrub_solver::ReleaseDep {
                            package: d.package.clone().unwrap_or_else(|| d.name.clone()),
                            req: d.req.clone(),
                            optional: d.optional,
                            default_activated: activated.contains(&d.name),
                            kind: d.kind.clone().unwrap_or_else(|| "normal".to_string()),
                            target: d.target.clone(),
                        })
                        .collect(),
                })
            })
            .collect())
    }

    fn target_applies(&self, gate: &str) -> bool {
        target_applies(gate, self.target)
    }
}

pub struct IndexSource<'a> {
    index: &'a Index,
    target: &'a str,
}

/// `rust-version` may be given as "1.70" (no patch), which semver rejects.
fn parse_rust_version(s: &str) -> Option<Version> {
    if let Ok(v) = Version::parse(s) {
        return Some(v);
    }
    let parts: Vec<&str> = s.split('.').collect();
    match parts.len() {
        1 => Version::parse(&format!("{}.0.0", parts[0])).ok(),
        2 => Version::parse(&format!("{}.{}.0", parts[0], parts[1])).ok(),
        _ => None,
    }
}

pub fn lock(args: LockCmdArgs) -> Result<()> {
    let build_text = fs::read_to_string(&args.build_file)
        .with_context(|| format!("Failed to read {}", args.build_file.display()))?;
    let lines: Vec<String> = build_text.lines().map(|s| s.to_string()).collect();
    let mut decls = parse_build(&lines)?;

    let cache_dir = args.cache_dir.clone().unwrap_or_else(|| {
        dirs_cache().join("please_rust").join("index")
    });
    let index = Index {
        url: args.index_url.trim_end_matches('/').to_string(),
        cache_dir,
        offline: args.offline,
        curl: args.curl.clone(),
        cache: std::cell::RefCell::new(BTreeMap::new()),
    };

    // Already-pinned versions are preferred by selection.
    let mut chosen: BTreeMap<String, Version> = BTreeMap::new();
    for d in &decls {
        let v = Version::parse(&d.version)
            .with_context(|| format!("Bad version {} for {}", d.version, d.crate_name))?;
        // Highest pinned version of each crate is the preferred pick
        let e = chosen.entry(d.crate_name.clone()).or_insert_with(|| v.clone());
        if v > *e {
            *e = v;
        }
    }

    // Worklist of (package, req, requirer)
    let mut work: Vec<(String, String, String)> = Vec::new();
    let mut added_roots: Vec<(String, Version)> = Vec::new();
    for add in &args.add {
        let (name, req) = add
            .split_once('@')
            .with_context(|| format!("--add takes crate@req, got {}", add))?;
        work.push((name.to_string(), req.to_string(), "--add".to_string()));
    }

    if !args.greedy {
        return lock_with_pubgrub(&args, &index, decls, &build_text);
    }

    let mut newly: BTreeMap<String, (Version, String)> = BTreeMap::new(); // crate -> (version, cksum)
    let mut visited: BTreeSet<(String, String)> = BTreeSet::new();
    while let Some((package, req_str, requirer)) = work.pop() {
        if !visited.insert((package.clone(), req_str.clone())) {
            continue;
        }
        let req = semver::VersionReq::parse(&req_str)
            .with_context(|| format!("Bad requirement {} on {} (from {})", req_str, package, requirer))?;

        // Prefer an already-chosen version
        if let Some(v) = chosen.get(&package) {
            if req.matches(v) {
                continue;
            }
            // A second major version is legitimate; only bail if we cannot
            // find any distinct satisfying version below.
        }

        let versions = index.versions(&package)?;
        let mut best: Option<&IndexVersion> = None;
        for iv in &versions {
            if iv.yanked {
                continue;
            }
            if let Ok(v) = Version::parse(&iv.vers) {
                if req.matches(&v) {
                    match &best {
                        Some(b) => {
                            if v > Version::parse(&b.vers).unwrap() {
                                best = Some(iv);
                            }
                        }
                        None => best = Some(iv),
                    }
                }
            }
        }
        let best = best.with_context(|| {
            format!(
                "no version of {} satisfies {} (required by {})",
                package, req_str, requirer
            )
        })?;
        let version = Version::parse(&best.vers).unwrap();

        let is_new = !chosen
            .get(&package)
            .map(|v| *v == version)
            .unwrap_or(false)
            && !newly.contains_key(&package);
        if requirer == "--add" {
            added_roots.push((package.clone(), version.clone()));
        }
        if chosen.get(&package).map(|v| req.matches(v)).unwrap_or(false) {
            continue;
        }
        newly.insert(package.clone(), (version.clone(), best.cksum.clone()));

        if is_new {
            // Recurse: mandatory deps plus optionals activated by default
            // features (what a plain --add requests).
            let activated = default_activated_deps(best);
            for dep in &best.deps {
                let kind = dep.kind.as_deref().unwrap_or("normal");
                if kind == "dev" {
                    continue;
                }
                if let Some(t) = &dep.target {
                    if !target_applies(t, &args.target) {
                        continue;
                    }
                }
                let dep_package = dep.package.clone().unwrap_or_else(|| dep.name.clone());
                if dep_package.starts_with("rustc-std-workspace") {
                    continue;
                }
                if dep.optional && !activated.contains(&dep.name) {
                    continue;
                }
                work.push((
                    dep_package,
                    dep.req.clone(),
                    format!("{}@{}", package, version),
                ));
            }
        }
    }

    if newly.is_empty() {
        eprintln!("lock: nothing to do");
        return Ok(());
    }

    for (package, (version, cksum)) in &newly {
        let root = added_roots.iter().any(|(p, _)| p == package);
        eprintln!("lock: + {}@{}{}", package, version, if root { " (root)" } else { "" });
        decls.push(Decl {
            name: None,
            crate_name: package.clone(),
            version: version.to_string(),
            features: vec![],
            hashes: vec![cksum.clone()],
            passthrough: vec![],
            leading_comments: vec![],
            span: None,
            imported: !root,
            root,
            default_features: true,
            git_repo: String::new(),
            git_revision: String::new(),
        });
    }

    finish_lock(&args, decls).map(|_| ())
}

/// Hand over to sync for naming, downloads, feature resolution and writing.
/// Shared by the greedy and PubGrub paths.
fn finish_lock(args: &LockCmdArgs, mut decls: Vec<Decl>) -> Result<Vec<crate::resolve::MissingDep>> {
    let build_text = fs::read_to_string(&args.build_file)
        .with_context(|| format!("Failed to read {}", args.build_file.display()))?;
    let lines: Vec<String> = build_text.lines().map(|s| s.to_string()).collect();
    let _ = normalize_names(&mut decls)?;
    fs::write(&args.build_file, rewrite_build(&lines, &decls, &[]))?;
    run_reporting(SyncArgs {
        build_file: args.build_file.clone(),
        third_party_folder: args.third_party_folder.clone(),
        crate_store: None,
        import: None,
        import_workspace: None,
        target: args.target.clone(),
        targets: args.targets.clone(),
        lock_output: None,
        plz: args.plz.clone(),
        no_rename: false,
        prune: false,
        dry_run: false,
    })
}

fn dirs_cache() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(|h| PathBuf::from(h).join(".cache"))
                .unwrap_or_else(|| PathBuf::from(".cache"))
        })
}

/// Dep names activated by the crate's default features (index metadata).
/// PubGrub-backed lock: solve the whole declared set plus the requested
/// additions at once, then hand the result to the same declaration writer
/// the greedy path uses.
fn lock_with_pubgrub(
    args: &LockCmdArgs,
    index: &Index,
    decls: Vec<Decl>,
    build_text: &str,
) -> Result<()> {
    let missing = lock_round(args, index, decls, build_text)?;
    heal_missing(args, index, missing)
}

/// One solve-and-write pass; returns what resolution could not find.
fn lock_round(
    args: &LockCmdArgs,
    index: &Index,
    mut decls: Vec<Decl>,
    build_text: &str,
) -> Result<Vec<crate::resolve::MissingDep>> {
    // Fast path: if every requested addition is already satisfied by a
    // declaration, there is nothing to solve and nothing to fetch. This keeps
    // a no-op `lock --add` working offline with a cold index cache.
    let all_satisfied = !args.add.is_empty()
        && args.add.iter().all(|add| {
            let Some((name, req_str)) = add.split_once('@') else {
                return false;
            };
            let Ok(req) = semver::VersionReq::parse(req_str) else {
                return false;
            };
            decls.iter().any(|d| {
                d.crate_name == name
                    && Version::parse(&d.version).map(|v| req.matches(&v)).unwrap_or(false)
            })
        });
    if all_satisfied && args.features.is_none() {
        eprintln!("lock: nothing to do");
        return Ok(Vec::new());
    }

    // Solve once per platform the declaration set covers and declare the
    // union: a crate gated behind cfg(target_os = "macos") is invisible to a
    // linux solve, and a checked-in declaration set missing it leaves mac
    // developers unable to build.
    let triples = target_list(&args.targets, &args.target);
    let mut solution: BTreeMap<String, (Version, String)> = BTreeMap::new();
    for triple in &triples {
        let source = IndexSource { index, target: triple };
        let mut solver = crate::pubgrub_solver::Solver::new(&source);

    // Declared versions are preferences, not requirements: the solve is
    // driven by the additions, so `lock --add` never needs index entries for
    // unrelated crates (which would also break --offline).
    for d in &decls {
        let v = Version::parse(&d.version)
            .with_context(|| format!("Bad version {} for {}", d.version, d.crate_name))?;
        solver.pin(&d.crate_name, v);
    }
    for add in &args.add {
        let (name, req_str) = add
            .split_once('@')
            .with_context(|| format!("--add takes crate@req, got {}", add))?;
        let req = semver::VersionReq::parse(req_str)
            .with_context(|| format!("Bad requirement {} in --add", req_str))?;
        solver.require(name, req);
    }

    let toolchain = if args.ignore_msrv {
        None
    } else {
        let tc = crate::pubgrub_solver::toolchain_version(build_text);
        if tc.is_none() {
            eprintln!("lock: no rust_toolchain version found; MSRV filtering is off");
        }
        tc
    };
        solver.msrv(toolchain);

        // Later platforms only add what earlier ones could not see; a crate
        // both need is already pinned to one version by the first solve.
        for (key, value) in solver.solve()? {
            solution.entry(key).or_insert(value);
        }
    }

    // Fold the solution into the declarations: an existing crate in the same
    // compatibility bucket is upgraded in place (cargo's behaviour when a new
    // requirement needs a newer patch or minor), anything else is added.
    use crate::pubgrub_solver::Bucket;
    let mut newly: Vec<(String, Version, String)> = Vec::new();
    let mut upgraded: Vec<(String, String, Version)> = Vec::new();
    for (key, (version, cksum)) in &solution {
        let name = key.split('@').next().unwrap_or(key).to_string();
        let bucket = Bucket::of(version);
        let existing = decls.iter_mut().find(|d| {
            d.crate_name == name
                && Version::parse(&d.version)
                    .map(|v| Bucket::of(&v) == bucket)
                    .unwrap_or(false)
        });
        match existing {
            Some(d) => {
                if d.version != version.to_string() {
                    upgraded.push((name.clone(), d.version.clone(), version.clone()));
                    d.version = version.to_string();
                    d.hashes = vec![cksum.clone()];
                }
            }
            None => newly.push((name, version.clone(), cksum.clone())),
        }
    }
    newly.sort();
    upgraded.sort();

    for (name, from, to) in &upgraded {
        eprintln!("lock: ^ {} {} -> {}", name, from, to);
    }

    if newly.is_empty() && upgraded.is_empty() && args.features.is_none() {
        eprintln!("lock: nothing to do");
        return Ok(Vec::new());
    }

    let added_names: BTreeSet<String> = args
        .add
        .iter()
        .filter_map(|a| a.split_once('@').map(|(n, _)| n.to_string()))
        .collect();
    for (name, version, cksum) in &newly {
        let root = added_names.contains(name);
        eprintln!("lock: + {}@{}{}", name, version, if root { " (root)" } else { "" });
        decls.push(Decl {
            name: None,
            crate_name: name.clone(),
            version: version.to_string(),
            features: vec![],
            hashes: vec![cksum.clone()],
            passthrough: vec![],
            leading_comments: vec![],
            span: None,
            imported: true,
            root,
            default_features: true,
            git_repo: String::new(),
            git_revision: String::new(),
        });
    }

    // Requested features land on the crates named in --add
    if let Some(features) = &args.features {
        let wanted: Vec<String> = features
            .split(',')
            .map(|f| f.trim().to_string())
            .filter(|f| !f.is_empty())
            .collect();
        for add in &args.add {
            let name = add.split('@').next().unwrap_or(add);
            if let Some(d) = decls.iter_mut().find(|d| d.crate_name == name) {
                for f in &wanted {
                    if !d.features.contains(f) {
                        d.features.push(f.clone());
                    }
                }
                d.features.sort();
                d.root = true;
                eprintln!("lock: {} features = {}", name, d.features.join(","));
            }
        }
    }

    finish_lock(args, decls).map(|m| m)
}

/// Feed dependencies that feature unification turned on back into the solver
/// until the declaration set is closed.
fn heal_missing(
    args: &LockCmdArgs,
    index: &Index,
    mut missing: Vec<crate::resolve::MissingDep>,
) -> Result<()> {
    // Resolution runs over the declared set with real feature unification,
    // which can activate optional dependencies the version solve never saw
    // (enabling serde's `derive` needs serde_derive declared). Feed anything
    // it could not find back in and solve again until the graph is closed.
    for round in 0..8 {
        if missing.is_empty() {
            break;
        }
        let mut adds: Vec<String> = Vec::new();
        for m in &missing {
            let req = m.req.clone().unwrap_or_else(|| "*".to_string());
            eprintln!(
                "lock: {} needs {} ({}), which a feature activated; adding it",
                m.requirer, m.package, req
            );
            adds.push(format!("{}@{}", m.package, req));
        }
        adds.sort();
        adds.dedup();
        let healed = LockCmdArgs {
            add: adds,
            features: None,
            ..clone_lock_args(args)
        };
        let build_text = fs::read_to_string(&args.build_file)?;
        let lines: Vec<String> = build_text.lines().map(|s| s.to_string()).collect();
        let decls = parse_build(&lines)?;
        missing = lock_round(&healed, index, decls, &build_text)?;
        if round == 7 && !missing.is_empty() {
            eprintln!(
                "lock: still missing {} dependencies after healing; declare them manually",
                missing.len()
            );
        }
    }
    Ok(())
}

fn clone_lock_args(args: &LockCmdArgs) -> LockCmdArgs {
    LockCmdArgs {
        build_file: args.build_file.clone(),
        third_party_folder: args.third_party_folder.clone(),
        add: args.add.clone(),
        index_url: args.index_url.clone(),
        cache_dir: args.cache_dir.clone(),
        offline: args.offline,
        target: args.target.clone(),
        targets: args.targets.clone(),
        curl: args.curl.clone(),
        plz: args.plz.clone(),
        greedy: args.greedy,
        ignore_msrv: args.ignore_msrv,
        features: args.features.clone(),
    }
}

fn default_activated_deps(iv: &IndexVersion) -> BTreeSet<String> {
    let features = iv.all_features();
    let mut activated = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let mut stack = vec!["default".to_string()];
    while let Some(f) = stack.pop() {
        if !seen.insert(f.clone()) {
            continue;
        }
        if let Some(items) = features.get(&f) {
            for item in items {
                if let Some(dep) = item.strip_prefix("dep:") {
                    activated.insert(dep.to_string());
                } else if let Some((dep, _)) = item.split_once("?/") {
                    let _ = dep; // weak: does not activate
                } else if let Some((dep, _)) = item.split_once('/') {
                    activated.insert(dep.to_string());
                } else {
                    stack.push(item.clone());
                }
            }
        } else {
            // Implicit optional-dep feature
            activated.insert(f);
        }
    }
    activated
}

pub fn target_applies(target_cfg: &str, triple: &str) -> bool {
    if target_cfg.starts_with("cfg(") {
        if let Some(info) = cfg_expr::targets::get_builtin_target_by_triple(triple) {
            if let Ok(expr) = cfg_expr::Expression::parse(target_cfg) {
                return expr.eval(|pred| match pred {
                    cfg_expr::expr::Predicate::Target(tp) => tp.matches(info),
                    _ => false,
                });
            }
        }
        false
    } else {
        target_cfg == triple
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Vec<Decl> {
        let lines: Vec<String> = text.lines().map(|s| s.to_string()).collect();
        parse_build(&lines).unwrap()
    }

    const BUILD: &str = r#"subinclude("//build_defs:rust")

rust_toolchain(
    name = "toolchain",
    version = "1.97.1",
)

# A load-bearing comment (with parens) about serde
rust_repo(
    name = "serde",
    crate = "serde",
    version = "1.0.228",
    features = ["derive", "default"],
    hashes = ["abc123"],
    visibility = ["PUBLIC"],
)

rust_repo(
    name = "itoa",
    crate = "itoa",
    version = "1.0.11",
    indirect = True,
)

rust_repo(
    name = "quirky",
    crate = "quirky-crate",
    version = "0.1.0",
    default_features = False,
    git_repo = "someone/quirky",
    git_revision = "abcdef",
)
"#;

    #[test]
    fn parses_declarations() {
        let decls = parse(BUILD);
        assert_eq!(decls.len(), 3);

        let serde = &decls[0];
        assert_eq!(serde.name.as_deref(), Some("serde"));
        assert_eq!(serde.crate_name, "serde");
        assert_eq!(serde.version, "1.0.228");
        assert_eq!(serde.features, vec!["derive", "default"]);
        assert_eq!(serde.hashes, vec!["abc123"]);
        assert!(serde.root);
        assert!(serde.default_features);
        assert_eq!(serde.passthrough, vec!["    visibility = [\"PUBLIC\"]"]);
        assert_eq!(serde.leading_comments.len(), 1);

        let itoa = &decls[1];
        assert!(!itoa.root);

        let quirky = &decls[2];
        assert!(!quirky.default_features);
        assert_eq!(quirky.git_repo, "someone/quirky");
        assert_eq!(quirky.git_revision, "abcdef");
    }

    #[test]
    fn emit_parse_round_trip() {
        let decls = parse(BUILD);
        let emitted: String = decls.iter().map(emit_decl).collect::<Vec<_>>().join("\n");
        let reparsed = parse(&emitted);
        assert_eq!(decls.len(), reparsed.len());
        for (a, b) in decls.iter().zip(&reparsed) {
            assert_eq!(a.subrepo(), b.subrepo());
            assert_eq!(a.crate_name, b.crate_name);
            assert_eq!(a.version, b.version);
            assert_eq!(a.root, b.root);
            assert_eq!(a.default_features, b.default_features);
            assert_eq!(a.hashes, b.hashes);
            assert_eq!(a.git_repo, b.git_repo);
            // Indirect entries never emit features (derived data)
            if a.root {
                assert_eq!(a.features, b.features);
            }
        }
    }

    #[test]
    fn indirect_entries_emit_no_features() {
        let mut decls = parse(BUILD);
        decls[1].features = vec!["stale".to_string()];
        let text = emit_decl(&decls[1]);
        assert!(!text.contains("stale"));
        assert!(text.contains("indirect = True"));
    }

    #[test]
    fn rewrite_replaces_in_place_and_deletes() {
        let lines: Vec<String> = BUILD.lines().map(|s| s.to_string()).collect();
        let mut decls = parse_build(&lines).unwrap();

        // Drop itoa, keeping its span for deletion
        let deleted = vec![decls[1].span.unwrap()];
        decls.remove(1);
        // Add a new entry (no span -> appended)
        decls.push(Decl {
            name: Some("newbie".to_string()),
            crate_name: "newbie".to_string(),
            version: "0.1.0".to_string(),
            features: vec![],
            hashes: vec![],
            passthrough: vec![],
            leading_comments: vec![],
            span: None,
            imported: false,
            root: true,
            default_features: true,
            git_repo: String::new(),
            git_revision: String::new(),
        });

        let out = rewrite_build(&lines, &decls, &deleted);
        assert!(!out.contains("itoa"));
        assert!(out.contains("newbie"));
        assert!(out.contains("rust_toolchain")); // untouched content survives
        assert!(out.contains("load-bearing comment"));

        // Idempotency: rewriting the rewrite changes nothing
        let lines2: Vec<String> = out.lines().map(|s| s.to_string()).collect();
        let decls2 = parse_build(&lines2).unwrap();
        let out2 = rewrite_build(&lines2, &decls2, &[]);
        assert_eq!(out, out2);
    }

    #[test]
    fn resolve_block_is_idempotent() {
        let decls = parse(BUILD);
        let one = write_resolve_block(BUILD, &decls, "x86_64-unknown-linux-gnu");
        let two = write_resolve_block(&one, &decls, "x86_64-unknown-linux-gnu");
        assert_eq!(one, two);
        assert_eq!(one.matches("rust_resolve(").count(), 1);
        assert!(one.contains("serde|serde|1.0.228|derive,default|true|true"));
    }

    #[test]
    fn normalize_names_versions() {
        let mut decls = parse(BUILD);
        decls.push(Decl {
            name: Some("serde_old".to_string()),
            crate_name: "serde".to_string(),
            version: "1.0.100".to_string(),
            features: vec![],
            hashes: vec![],
            passthrough: vec![],
            leading_comments: vec![],
            span: None,
            imported: false,
            root: false,
            default_features: true,
            git_repo: String::new(),
            git_revision: String::new(),
        });
        let renames = normalize_names(&mut decls).unwrap();
        assert_eq!(renames.get("serde_old").unwrap(), "serde-1.0.100");
        // The newest keeps the plain name
        assert!(decls.iter().any(|d| d.version == "1.0.228" && d.subrepo() == "serde"));
    }

    #[test]
    fn import_lockfile_sources() {
        let dir = std::env::temp_dir().join(format!("please_rust_sync_test_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("Cargo.lock"), r#"
[[package]]
name = "serde"
version = "1.0.228"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "feedface"

[[package]]
name = "fresh"
version = "2.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "cafebabe"

[[package]]
name = "forked"
version = "0.5.0"
source = "git+https://github.com/owner/forked?rev=abc#abcdef123456"

[[package]]
name = "elsewhere"
version = "0.1.0"
source = "git+https://gitlab.example.com/x/y#deadbeef"

[[package]]
name = "local_thing"
version = "0.0.1"
"#).unwrap();
        fs::write(dir.join("Cargo.toml"), r#"
[package]
name = "ws"
version = "0.0.0"

[dependencies]
fresh = { version = "2", features = ["extra"] }
"#).unwrap();

        let mut decls = parse(BUILD);
        // serde already declared but without this hash
        decls[0].hashes.clear();
        import_cargo_lock(&dir.join("Cargo.lock"), &mut decls).unwrap();

        // Existing entry got the hash attached
        assert_eq!(decls.iter().find(|d| d.crate_name == "serde").unwrap().hashes, vec!["feedface"]);
        // New registry crate imported with hash, marked root via workspace manifest
        let fresh = decls.iter().find(|d| d.crate_name == "fresh").unwrap();
        assert_eq!(fresh.hashes, vec!["cafebabe"]);
        assert!(fresh.root);
        assert!(fresh.features.contains(&"extra".to_string()));
        assert!(fresh.features.contains(&"default".to_string()));
        // Github git source imported with repo/revision
        let forked = decls.iter().find(|d| d.crate_name == "forked").unwrap();
        assert_eq!(forked.git_repo, "owner/forked");
        assert_eq!(forked.git_revision, "abcdef123456");
        // Non-github git and local path crates skipped
        assert!(!decls.iter().any(|d| d.crate_name == "elsewhere"));
        assert!(!decls.iter().any(|d| d.crate_name == "local_thing"));
    }

    #[test]
    fn repo_root_walks_to_plzconfig() {
        let dir = std::env::temp_dir().join(format!("please_rust_root_test_{}", std::process::id()));
        let nested = dir.join("third_party/rust");
        fs::create_dir_all(&nested).unwrap();
        fs::write(dir.join(".plzconfig"), "").unwrap();
        fs::write(nested.join("BUILD"), "").unwrap();
        assert_eq!(repo_root(&nested.join("BUILD")), dir.canonicalize().unwrap());
    }
}

#[cfg(test)]
mod run_tests {
    use super::*;

    /// A full sync round-trip against a scratch repo: fixtures on disk,
    /// resolution, prune, rewrite, and idempotency on the second pass.
    #[test]
    fn full_sync_round_trip() {
        let dir = std::env::temp_dir().join(format!("please_rust_sync_run_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let store = dir.join("store");
        fs::create_dir_all(&store).unwrap();
        fs::write(dir.join(".plzconfig"), "").unwrap();

        for (name, version, manifest) in [
            ("app", "1.0.0", "[package]\nname = \"app\"\nversion = \"1.0.0\"\n\n[dependencies]\nutil = \"1\"\n"),
            ("util", "1.5.0", "[package]\nname = \"util\"\nversion = \"1.5.0\"\n"),
            ("unused", "0.1.0", "[package]\nname = \"unused\"\nversion = \"0.1.0\"\n"),
        ] {
            let cdir = store.join(format!("{}-{}", name, version));
            fs::create_dir_all(&cdir).unwrap();
            fs::write(cdir.join("Cargo.toml"), manifest).unwrap();
        }

        let build_file = dir.join("BUILD");
        fs::write(&build_file, r#"rust_repo(
    name = "app",
    crate = "app",
    version = "1.0.0",
)

rust_repo(
    name = "util_old_name",
    crate = "util",
    version = "1.5.0",
    indirect = True,
)

rust_repo(
    name = "unused",
    crate = "unused",
    version = "0.1.0",
    indirect = True,
)
"#).unwrap();

        let args = || SyncArgs {
            build_file: build_file.clone(),
            third_party_folder: "third_party/rust".to_string(),
            crate_store: Some(store.clone()),
            import: None,
            import_workspace: None,
            target: "x86_64-unknown-linux-gnu".to_string(),
            targets: "x86_64-unknown-linux-gnu".to_string(),
            lock_output: None,
            plz: "".to_string(),
            no_rename: false,
            prune: true,
            dry_run: false,
        };
        run(args()).unwrap();

        let out = fs::read_to_string(&build_file).unwrap();
        // Naming normalized, inactive indirect pruned, resolve block written
        assert!(out.contains("name = \"util\""));
        assert!(!out.contains("util_old_name"));
        assert!(!out.contains("\"unused\""));
        assert!(out.contains("rust_resolve("));
        assert!(out.contains("app|app|1.0.0||true|true"));

        // Second pass is a fixed point
        run(args()).unwrap();
        assert_eq!(fs::read_to_string(&build_file).unwrap(), out);

        // Dry run changes nothing
        let mut dry = args();
        dry.dry_run = true;
        run(dry).unwrap();
        assert_eq!(fs::read_to_string(&build_file).unwrap(), out);
    }

    #[test]
    fn missing_manifest_without_plz_errors() {
        let dir = std::env::temp_dir().join(format!("please_rust_sync_missing_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let store = dir.join("store");
        fs::create_dir_all(&store).unwrap();
        let build_file = dir.join("BUILD");
        fs::write(&build_file, "rust_repo(\n    name = \"ghost\",\n    crate = \"ghost\",\n    version = \"1.0.0\",\n)\n").unwrap();
        let err = run(SyncArgs {
            build_file,
            third_party_folder: "third_party/rust".to_string(),
            crate_store: Some(store),
            import: None,
            import_workspace: None,
            target: "x86_64-unknown-linux-gnu".to_string(),
            targets: "x86_64-unknown-linux-gnu".to_string(),
            lock_output: None,
            plz: "".to_string(),
            no_rename: true,
            prune: false,
            dry_run: false,
        })
        .unwrap_err();
        assert!(err.to_string().contains("not downloaded"));
    }

    #[test]
    fn index_path_layout() {
        assert_eq!(Index::path_for("a"), "1/a");
        assert_eq!(Index::path_for("ab"), "2/ab");
        assert_eq!(Index::path_for("abc"), "3/a/abc");
        assert_eq!(Index::path_for("Serde"), "se/rd/serde");
    }

    #[test]
    fn default_feature_dep_activation() {
        let iv = IndexVersion {
            vers: "1.0.0".to_string(),
            deps: vec![],
            cksum: "x".to_string(),
            features: [
                ("default".to_string(), vec!["std".to_string(), "dep:mandatory_opt".to_string()]),
                ("std".to_string(), vec!["helper/fast".to_string()]),
            ]
            .into_iter()
            .collect(),
            features2: Some([("weakling".to_string(), vec!["other?/x".to_string()])].into_iter().collect()),
            yanked: false,
            rust_version: None,
        };
        let activated = default_activated_deps(&iv);
        assert!(activated.contains("mandatory_opt"));
        assert!(activated.contains("helper"));
        assert!(!activated.contains("other")); // weak, and feature not defaulted
    }

    #[test]
    fn target_cfg_matching() {
        assert!(target_applies("cfg(unix)", "x86_64-unknown-linux-gnu"));
        assert!(!target_applies("cfg(windows)", "x86_64-unknown-linux-gnu"));
        assert!(target_applies("x86_64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"));
        assert!(!target_applies("cfg(broken", "x86_64-unknown-linux-gnu"));
    }
}

#[cfg(test)]
mod lock_cmd_tests {
    use super::*;

    /// The lock command end to end, fully offline: index responses come from
    /// a pre-populated cache directory, downloads from a fake crate store.
    #[test]
    fn lock_add_resolves_from_cached_index() {
        let dir = std::env::temp_dir().join(format!("please_rust_lock_e2e_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(".plzconfig"), "").unwrap();

        // Sparse-index cache: hexlib 0.4.3 (yanked 0.4.4 must be skipped),
        // with a mandatory dep on tinydep ^1
        let cache = dir.join("index-cache");
        fs::create_dir_all(cache.join("he")).unwrap();
        fs::create_dir_all(cache.join("he/xl")).unwrap();
        fs::write(cache.join("he/xl/hexlib"), concat!(
            r#"{"name":"hexlib","vers":"0.4.3","deps":[{"name":"tinydep","req":"^1","features":[],"optional":false,"default_features":true,"target":null,"kind":"normal"}],"cksum":"aaa111","features":{}}"#, "\n",
            r#"{"name":"hexlib","vers":"0.4.4","deps":[],"cksum":"bbb222","features":{},"yanked":true}"#, "\n",
        )).unwrap();
        fs::create_dir_all(cache.join("ti/ny")).unwrap();
        fs::write(cache.join("ti/ny/tinydep"),
            concat!(r#"{"name":"tinydep","vers":"1.2.0","deps":[],"cksum":"ccc333","features":{}}"#, "\n")).unwrap();

        // Crate store so the post-lock sync can resolve manifests
        let store = dir.join("plz-out/gen/third_party/rust");
        for (name, ver) in [("hexlib", "0.4.3"), ("tinydep", "1.2.0")] {
            let cdir = store.join(format!("{}-{}", name, ver));
            fs::create_dir_all(&cdir).unwrap();
            let manifest = if name == "hexlib" {
                format!("[package]\nname = \"{}\"\nversion = \"{}\"\n\n[dependencies]\ntinydep = \"1\"\n", name, ver)
            } else {
                format!("[package]\nname = \"{}\"\nversion = \"{}\"\n", name, ver)
            };
            fs::write(cdir.join("Cargo.toml"), manifest).unwrap();
        }

        let build_file = dir.join("BUILD");
        fs::write(&build_file, "").unwrap();

        lock(LockCmdArgs {
            build_file: build_file.clone(),
            third_party_folder: "third_party/rust".to_string(),
            add: vec!["hexlib@0.4".to_string()],
            index_url: "https://index.invalid".to_string(),
            cache_dir: Some(cache),
            offline: true,
            target: "x86_64-unknown-linux-gnu".to_string(),
            targets: "x86_64-unknown-linux-gnu".to_string(),
            curl: "false".to_string(),
            plz: "".to_string(),
            greedy: false,
            ignore_msrv: false,
            features: None,
        })
        .unwrap();

        let out = fs::read_to_string(&build_file).unwrap();
        // Yanked 0.4.4 skipped; 0.4.3 chosen as a root with its index hash
        assert!(out.contains("version = \"0.4.3\""));
        assert!(out.contains("hashes = [\"aaa111\"]"));
        // Transitive dep declared indirect with its hash
        assert!(out.contains("\"tinydep\""));
        assert!(out.contains("hashes = [\"ccc333\"]"));
        assert!(out.contains("indirect = True"));
        assert!(out.contains("rust_resolve("));
    }

    #[test]
    fn lock_offline_without_cache_errors() {
        let dir = std::env::temp_dir().join(format!("please_rust_lock_offline_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let build_file = dir.join("BUILD");
        fs::write(&build_file, "").unwrap();
        let err = lock(LockCmdArgs {
            build_file,
            third_party_folder: "third_party/rust".to_string(),
            add: vec!["ghost@1".to_string()],
            index_url: "https://index.invalid".to_string(),
            cache_dir: Some(dir.join("empty-cache")),
            offline: true,
            target: "x86_64-unknown-linux-gnu".to_string(),
            targets: "x86_64-unknown-linux-gnu".to_string(),
            curl: "false".to_string(),
            plz: "".to_string(),
            greedy: false,
            ignore_msrv: false,
            features: None,
        })
        .unwrap_err();
        assert!(err.to_string().contains("not in index cache"));
    }

    #[test]
    fn lock_nothing_to_do_when_satisfied() {
        let dir = std::env::temp_dir().join(format!("please_rust_lock_noop_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let build_file = dir.join("BUILD");
        fs::write(&build_file, "rust_repo(\n    name = \"present\",\n    crate = \"present\",\n    version = \"1.0.0\",\n)\n").unwrap();
        let before = fs::read_to_string(&build_file).unwrap();
        lock(LockCmdArgs {
            build_file: build_file.clone(),
            third_party_folder: "third_party/rust".to_string(),
            add: vec!["present@1".to_string()],
            index_url: "https://index.invalid".to_string(),
            cache_dir: Some(dir.join("cache")),
            offline: true,
            target: "x86_64-unknown-linux-gnu".to_string(),
            targets: "x86_64-unknown-linux-gnu".to_string(),
            curl: "false".to_string(),
            plz: "".to_string(),
            greedy: false,
            ignore_msrv: false,
            features: None,
        })
        .unwrap();
        assert_eq!(fs::read_to_string(&build_file).unwrap(), before);
    }
}
