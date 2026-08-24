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
    #[arg(
        long,
        default_value = "x86_64-unknown-linux-gnu,aarch64-unknown-linux-gnu,aarch64-apple-darwin,x86_64-apple-darwin"
    )]
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
    /// Which operating systems resolution reaches this crate on, or None if
    /// every covered platform does. A property of the crate rather than of
    /// the machine, so it is the same in everyone's checkout.
    platforms: Option<BTreeSet<String>>,
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
    // A feature recorded only in the entries list is still a feature that was
    // asked for. Adopted here so a rewrite reconciles the two spellings rather
    // than dropping one of them.
    let entry_features = parse_resolve_features(&build_text);
    for d in decls.iter_mut() {
        if d.features.is_empty() {
            if let Some(f) = entry_features.get(&d.subrepo()) {
                d.features = f.clone();
            }
        }
    }

    // Import a cargo lockfile: add anything not yet declared, and attach
    // hashes to existing entries that lack them.
    if let Some(import) = &args.import {
        import_cargo_lock(import, &mut decls)?;
    }

    // Naming normalization: newest declared version of a crate gets the plain
    // normalized name; older duplicates get `crate_norm-x.y.z`.
    if !args.no_rename {
        for (old, new) in &normalize_names(&mut decls)? {
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

    // Which operating systems each declaration is reachable on. This is a
    // property of the crate and the graph, not of the machine running sync,
    // so it is the same wherever it is computed - which it has to be, since
    // it is written into a file everyone shares.
    let mut oses: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut covered_oses: BTreeSet<String> = BTreeSet::new();
    let note = |triple: &str,
                lock: &crate::resolve::LockFile,
                oses: &mut BTreeMap<String, BTreeSet<String>>,
                covered: &mut BTreeSet<String>| {
        let os = target_os_of(triple);
        covered.insert(os.clone());
        for name in lock.crates.keys().chain(lock.host_crates.keys()) {
            oses.entry(name.clone()).or_default().insert(os.clone());
        }
    };
    note(&args.target, &lock, &mut oses, &mut covered_oses);

    for triple in target_list(&args.targets, &args.target) {
        if triple == args.target {
            continue;
        }
        match resolve_entries(&entries, &triple) {
            Ok(other) => {
                note(&triple, &other, &mut oses, &mut covered_oses);
                active_anywhere.extend(other.crates.keys().cloned());
                active_anywhere.extend(other.host_crates.keys().cloned());
                // A dependency only another platform reaches is missing there
                // and nowhere here, so resolving one triple cannot see it.
                // rustix needs errno on macOS and linux-raw-sys on linux; a
                // linux-only view of the graph declares one and not the other.
                for m in other.missing {
                    if !missing_deps
                        .iter()
                        .any(|d| d.package == m.package && d.requirer == m.requirer)
                    {
                        eprintln!(
                            "sync: {} needs {} on {}, which is not declared",
                            m.requirer, m.package, triple
                        );
                        missing_deps.push(m);
                    }
                }
            }
            Err(e) => eprintln!("sync: could not resolve for {}: {:#}", triple, e),
        }
    }

    // A crate every covered platform reaches needs no attribute at all: the
    // absent case means "anywhere", which is what almost every crate is.
    //
    // Only from a run that covered at least the platforms the file already
    // talks about. `platforms` is a statement about all of them, and a run
    // resolving one triple has nothing to say about the others - recomputing
    // from it turns `platforms = ["macos"]` into nothing at all, which is how
    // rust-corpus lost every one of them to an unrelated `lock --add`.
    let file_oses: BTreeSet<String> = decls
        .iter()
        .filter_map(|d| d.platforms.clone())
        .flatten()
        .collect();
    if may_refresh_platforms(&file_oses, &covered_oses) {
        for d in decls.iter_mut() {
            let on = oses.get(&d.subrepo()).cloned().unwrap_or_default();
            d.platforms = if on == covered_oses { None } else { Some(on) };
        }
    } else {
        let unseen: Vec<&str> = file_oses
            .difference(&covered_oses)
            .map(|s| s.as_str())
            .collect();
        eprintln!(
            "sync: keeping the platforms already recorded - this run did not resolve for {}. \
             Pass --targets to refresh them.",
            unseen.join(", ")
        );
    }

    let before = decls.len();
    let mut deleted_spans: Vec<(usize, usize)> = Vec::new();
    decls.retain(|d| {
        let active = active_anywhere.contains(&d.subrepo());
        let keep = if d.imported || (args.prune && !d.root) {
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
    let new_build = write_resolve_block(
        &rewrite_build(&lines, &decls, &deleted_spans),
        &decls,
        &args.target,
    );

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
    let old_lock = args
        .build_file
        .parent()
        .unwrap_or(Path::new("."))
        .join("rust.lock");
    if old_lock.exists() {
        let _ = fs::remove_file(&old_lock);
        eprintln!(
            "sync: removed stale {} (resolution now happens in the build graph)",
            old_lock.display()
        );
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

/// Whether this run is entitled to rewrite the recorded platforms.
///
/// `platforms` says which of the covered platforms reach a crate, so it can
/// only be restated by a run that resolved for all of the platforms already
/// named. A run covering fewer has nothing to say about the rest, and
/// recomputing from it silently deletes them.
fn may_refresh_platforms(file_oses: &BTreeSet<String>, covered: &BTreeSet<String>) -> bool {
    file_oses.is_subset(covered)
}

/// The features each entry in an existing rust_resolve block asks for.
///
/// Features live in the rust_repo block as far as sync is concerned, but they
/// are visible - and editable - in the entries list too, and an entry edited
/// there is the natural thing to do. Read back so that a rewrite reconciles
/// the two rather than silently discarding one: rust-corpus lost `bundled`
/// from libsqlite3-sys this way, which is the entire reason that crate is in
/// the corpus.
fn parse_resolve_features(build: &str) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    let mut inside = false;
    for line in build.lines() {
        let t = line.trim();
        if t.starts_with("entries = [") {
            inside = true;
            continue;
        }
        if inside && t.starts_with(']') {
            break;
        }
        if !inside {
            continue;
        }
        let Some(entry) = t.trim_end_matches(',').strip_prefix('"') else {
            continue;
        };
        let entry = entry.trim_end_matches('"');
        let fields: Vec<&str> = entry.split('|').collect();
        if fields.len() < 4 || fields[3].is_empty() {
            continue;
        }
        out.insert(
            fields[0].to_string(),
            fields[3].split(',').map(|f| f.to_string()).collect(),
        );
    }
    out
}

/// Rewrite (or append) the rust_resolve block encoding the declared graph.
fn write_resolve_block(build: &str, decls: &[Decl], target: &str) -> String {
    let mut block = String::new();
    block.push_str(
        "# Machine-maintained by please_rust sync; resolution runs in the build graph.\n",
    );
    block.push_str("rust_resolve(\n    name = \"rust_lock\",\n");
    // No target: the rule derives the host's, so the same declarations
    // resolve correctly for linux and mac developers alike. sync --target
    // still resolves for whatever was asked, it just is not written here.
    let _ = target;
    block.push_str("    entries = [\n");
    for d in decls {
        let features = if d.root {
            d.features.join(",")
        } else {
            String::new()
        };
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
        if !replaced
            && (t.starts_with("rust_resolve(")
                || (t.starts_with('#') && t.contains("Machine-maintained by please_rust sync")))
        {
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

// Attributes sync writes itself. Anything else in a declaration is a user
// edit and is preserved verbatim; these would otherwise be written twice.
const MANAGED_KEYS: &[&str] = &[
    "name",
    "crate",
    "version",
    "features",
    "hashes",
    "dep_overrides",
    "indirect",
    "default_features",
    "git_repo",
    "git_revision",
    "platforms",
];

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
                if key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
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
            // Recomputed on every sync, but parsed so a hand-written value
            // survives a run that does not reach the crate at all.
            platforms: if body.contains("platforms = [") {
                Some(getlist("platforms").into_iter().collect())
            } else {
                None
            },
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
    let content =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
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
                // The shorthand, because that is what the rule has always
                // recorded for github and what its declarations look like.
                git_repo = path.trim_end_matches(".git").to_string();
            } else {
                // Any other forge is recorded as the URL it was cloned from.
                // The rule derives the archive scheme from the host, which is
                // right except for gitlab hosted somewhere its name does not
                // say; those need git_forge = "gitlab" adding by hand.
                git_repo = url.trim_end_matches(".git").to_string();
            }
            git_revision = frag.to_string();
            if git_revision.is_empty() {
                eprintln!(
                    "warning: {} git source has no pinned revision, skipping",
                    name
                );
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
            platforms: None,
            root: false,
            default_features: true,
            git_repo: git_repo.clone(),
            git_revision: git_revision.clone(),
        });
        added += 1;
    }
    eprintln!(
        "sync: imported {} new crates from {}",
        added,
        path.display()
    );

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

/// The `target_os` of a triple, which is the vocabulary the attribute uses
/// because it is the vocabulary the gating is written in.
fn target_os_of(triple: &str) -> String {
    if triple.contains("apple") {
        "macos".to_string()
    } else if triple.contains("windows") {
        "windows".to_string()
    } else if triple.contains("linux") {
        "linux".to_string()
    } else {
        triple.rsplit('-').next().unwrap_or("unknown").to_string()
    }
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
    if let Some(on) = &d.platforms {
        // Absent means anywhere. Present means only these, and an empty list
        // means none of the platforms this repo covers - objc2 is reachable
        // only on macOS, and says so itself in a compile_error!.
        let names: Vec<String> = on.iter().map(|o| format!("\"{}\"", o)).collect();
        s.push_str(&format!("    platforms = [{}],\n", names.join(", ")));
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

    /// Move crates to the newest version their requirements allow, which is
    /// what `cargo update` does. Bare `--upgrade` moves every declared crate;
    /// `--upgrade serde --upgrade hex` moves only those.
    ///
    /// Without it a declared version is preferred wherever it still fits, so
    /// adding one crate never churns the rest. That preference is exactly
    /// what this drops.
    #[arg(long = "upgrade", num_args = 0.., value_name = "CRATE")]
    pub upgrade: Option<Vec<String>>,

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
    #[arg(
        long,
        default_value = "x86_64-unknown-linux-gnu,aarch64-unknown-linux-gnu,aarch64-apple-darwin,x86_64-apple-darwin"
    )]
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
    /// default, using the version the repo's rust_toolchain declares)
    #[arg(long)]
    pub ignore_msrv: bool,

    /// The toolchain version to filter against, e.g. 1.97.1. Found from the
    /// repo's `rust_toolchain` declaration when not given, which is what it
    /// is for.
    #[arg(long)]
    pub toolchain_version: Option<String>,

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
    optional: bool,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    package: Option<String>,
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
        self.cache
            .borrow_mut()
            .insert(name.to_string(), versions.clone());
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
                    rust_version: iv.rust_version.as_deref().and_then(parse_rust_version),
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

/// Say what moved.
///
/// One line per crate is right for `lock --add`, where a handful move and
/// each one is news. `--upgrade` across a whole repo moves hundreds, and a
/// list that long is not read. What matters there is which crossed a major
/// boundary, because those are the ones that can stop compiling.
fn report_upgrades(upgraded: &[(String, String, Version)], upgrading: bool) {
    use crate::pubgrub_solver::Bucket;
    const LIST_LIMIT: usize = 25;

    // A version can move backwards during an upgrade, and it is not a bug.
    // Taking the newest of one crate can mean stepping another back, because
    // the newer release of that other crate forbade it: crypto-common 0.1.7
    // requires generic-array =0.14.7, so reaching generic-array 0.14.9 costs
    // crypto-common 0.1.6. Saying `^` about that is a lie, and a version
    // going down is exactly the thing a reader needs to see.
    let down = |from: &str, to: &Version| Version::parse(from).map(|f| f > *to).unwrap_or(false);
    let mark = |from: &str, to: &Version| if down(from, to) { "v" } else { "^" };

    if !upgrading || upgraded.len() <= LIST_LIMIT {
        for (name, from, to) in upgraded {
            eprintln!("lock: {} {} {} -> {}", mark(from, to), name, from, to);
        }
        return;
    }

    // Past the limit, the list is not read. What is worth saying is how many
    // moved, and then every crate that either crossed a major version or went
    // backwards, since those are the two ways this stops compiling.
    let notable: Vec<&(String, String, Version)> = upgraded
        .iter()
        .filter(|(_, from, to)| {
            down(from, to)
                || Version::parse(from)
                    .map(|f| Bucket::of(&f) != Bucket::of(to))
                    .unwrap_or(false)
        })
        .collect();
    let backwards = upgraded.iter().filter(|(_, f, t)| down(f, t)).count();
    eprintln!(
        "lock: moved {} crates, {} across a major version, {} backwards",
        upgraded.len(),
        notable.len() - backwards,
        backwards
    );
    for (name, from, to) in &notable {
        eprintln!("lock: {} {} {} -> {}", mark(from, to), name, from, to);
    }
}

/// The toolchain version MSRV filtering compares against.
///
/// It used to be scraped out of whichever file the declarations came from,
/// which is only right when the toolchain happens to be declared beside them.
/// This repo keeps them apart, so filtering was off here for every run: a
/// crate needing a newer rustc was declared without complaint, and the
/// warning written for exactly that case cannot fire while the filter is off.
/// Since #20 a repo can have several declaration files, so where the
/// toolchain sits is no longer a safe guess at all.
///
/// A rust_toolchain declares its version, so find it: the flag if given, the
/// declarations file if it is there, then the repo.
fn toolchain_for_msrv(args: &LockCmdArgs, build_text: &str) -> Option<Version> {
    if let Some(v) = &args.toolchain_version {
        match Version::parse(v) {
            Ok(v) => return Some(v),
            Err(e) => {
                eprintln!("lock: --toolchain-version {} is not a version ({})", v, e);
                return None;
            }
        }
    }
    if let Some(v) = crate::pubgrub_solver::toolchain_version(build_text) {
        return Some(v);
    }
    let root = repo_root(&args.build_file);
    let mut found: Vec<(PathBuf, Version)> = Vec::new();
    for entry in walkdir::WalkDir::new(&root)
        .max_depth(6)
        .into_iter()
        .filter_entry(|e| {
            let n = e.file_name().to_string_lossy();
            // plz-out holds generated BUILD files for every crate, and none
            // of them declares a toolchain.
            !(e.depth() > 0 && e.file_type().is_dir() && (n == "plz-out" || n.starts_with('.')))
        })
        .filter_map(Result::ok)
    {
        let name = entry.file_name().to_string_lossy().to_string();
        if name != "BUILD" && name != "BUILD.plz" {
            continue;
        }
        if let Ok(text) = fs::read_to_string(entry.path()) {
            if let Some(v) = crate::pubgrub_solver::toolchain_version(&text) {
                found.push((entry.path().to_path_buf(), v));
            }
        }
    }
    found.sort_by(|a, b| a.1.cmp(&b.1));
    match found.len() {
        0 => {
            eprintln!(
                "lock: no rust_toolchain found under {}, so MSRV filtering is off and a crate \
                 needing a newer rustc than yours can be declared. Pass --toolchain-version to \
                 say which rustc to resolve for.",
                root.display()
            );
            None
        }
        1 => Some(found.remove(0).1),
        _ => {
            // The oldest, because a declaration set is shared and has to build
            // for whoever has the oldest toolchain.
            let names: Vec<String> = found
                .iter()
                .map(|(p, v)| format!("{} ({})", v, p.display()))
                .collect();
            eprintln!(
                "lock: several rust_toolchain versions found, resolving for the oldest: {}",
                names.join(", ")
            );
            Some(found.remove(0).1)
        }
    }
}

/// Whether this crate's declared version should be dropped as a preference.
///
/// `None` is the flag absent, `Some([])` is a bare `--upgrade` meaning
/// everything, and named crates mean only those. Names are compared with `-`
/// and `_` folded together, because a crate is declared under whichever the
/// package uses and typed under whichever the user remembers.
fn upgrading(upgrade: &Option<Vec<String>>, crate_name: &str) -> bool {
    match upgrade {
        None => false,
        Some(names) if names.is_empty() => true,
        Some(names) => {
            let want = crate_name.replace('-', "_");
            names.iter().any(|n| n.replace('-', "_") == want)
        }
    }
}

pub fn lock(args: LockCmdArgs) -> Result<()> {
    let build_text = fs::read_to_string(&args.build_file)
        .with_context(|| format!("Failed to read {}", args.build_file.display()))?;
    let lines: Vec<String> = build_text.lines().map(|s| s.to_string()).collect();
    let mut decls = parse_build(&lines)?;

    // A feature recorded only in the entries list is still a feature that was
    // asked for. Adopted here so a rewrite reconciles the two spellings rather
    // than dropping one of them.
    let entry_features = parse_resolve_features(&build_text);
    for d in decls.iter_mut() {
        if d.features.is_empty() {
            if let Some(f) = entry_features.get(&d.subrepo()) {
                d.features = f.clone();
            }
        }
    }

    let cache_dir = args
        .cache_dir
        .clone()
        .unwrap_or_else(|| dirs_cache().join("please_rust").join("index"));
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
        let e = chosen
            .entry(d.crate_name.clone())
            .or_insert_with(|| v.clone());
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
        let req = semver::VersionReq::parse(&req_str).with_context(|| {
            format!(
                "Bad requirement {} on {} (from {})",
                req_str, package, requirer
            )
        })?;

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

        let is_new = !chosen.get(&package).map(|v| *v == version).unwrap_or(false)
            && !newly.contains_key(&package);
        if requirer == "--add" {
            added_roots.push((package.clone(), version.clone()));
        }
        if chosen
            .get(&package)
            .map(|v| req.matches(v))
            .unwrap_or(false)
        {
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
        eprintln!(
            "lock: + {}@{}{}",
            package,
            version,
            if root { " (root)" } else { "" }
        );
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
            platforms: None,
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
fn finish_lock(
    args: &LockCmdArgs,
    mut decls: Vec<Decl>,
) -> Result<Vec<crate::resolve::MissingDep>> {
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
/// Mark declarations named by `--add` as roots, returning what changed.
///
/// Asking for a crate that is already present as somebody else's dependency
/// still makes it a root. Otherwise it stays reachable only through whoever
/// pulled it in, and when that edge moves - a version bump elsewhere is
/// enough - the declaration is suddenly unreferenced and the crate this repo
/// asked for is being built by accident, or not at all.
fn promote_added_roots(decls: &mut [Decl], adds: &[String]) -> Vec<String> {
    let mut promoted: Vec<String> = Vec::new();
    for add in adds {
        let (name, req_str) = match add.split_once('@') {
            Some(p) => p,
            None => continue,
        };
        let req = semver::VersionReq::parse(req_str).ok();
        for d in decls.iter_mut() {
            if d.crate_name != name || d.root {
                continue;
            }
            let matches = match (&req, Version::parse(&d.version)) {
                (Some(r), Ok(v)) => r.matches(&v),
                _ => true,
            };
            if matches {
                d.root = true;
                promoted.push(format!("{}@{}", d.crate_name, d.version));
            }
        }
    }
    promoted.sort();
    promoted
}

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
    // Asking for a crate that is already present as somebody else's
    // dependency still makes it a root. Otherwise it stays reachable only
    // through whoever pulled it in, and when that edge moves - a version bump
    // elsewhere is enough - the declaration is suddenly unreferenced and the
    // crate this repo asked for is being built by accident, or not at all.
    let promoted = promote_added_roots(&mut decls, &args.add);
    for p in &promoted {
        eprintln!("lock: * {} is now a root", p);
    }

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
                    && Version::parse(&d.version)
                        .map(|v| req.matches(&v))
                        .unwrap_or(false)
            })
        });
    if all_satisfied && args.features.is_none() {
        if promoted.is_empty() {
            eprintln!("lock: nothing to do");
            return Ok(Vec::new());
        }
        // Nothing to solve, but the declarations changed, so they still have
        // to be written and re-reported.
        return finish_lock(args, decls).map(|_| Vec::new());
    }

    // Solve once per platform the declaration set covers and declare the
    // union: a crate gated behind cfg(target_os = "macos") is invisible to a
    // linux solve, and a checked-in declaration set missing it leaves mac
    // developers unable to build.
    let triples = target_list(&args.targets, &args.target);
    // A name nobody declares moves nothing, and silently doing nothing is how
    // a typo looks like a crate that is already current.
    if let Some(names) = &args.upgrade {
        for n in names {
            let want = n.replace('-', "_");
            if !decls.iter().any(|d| d.crate_name.replace('-', "_") == want) {
                eprintln!("lock: --upgrade {} is not declared here", n);
            }
        }
    }
    let mut solution: BTreeMap<String, (Version, String)> = BTreeMap::new();
    for triple in &triples {
        let source = IndexSource {
            index,
            target: triple,
        };
        let mut solver = crate::pubgrub_solver::Solver::new(&source);

        // Declared versions are preferences, not requirements: the solve is
        // driven by the additions, so `lock --add` never needs index entries for
        // unrelated crates (which would also break --offline).
        //
        // Dropping a crate's preference is the whole of --upgrade. Nothing
        // else changes: the same solver, the same MSRV filter, the same
        // requirements. The crate is simply no longer told where it already
        // is, so the newest version in range wins the way it does for a crate
        // being declared for the first time.
        for d in &decls {
            if upgrading(&args.upgrade, &d.crate_name) {
                continue;
            }
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
        // Dropping a preference moves nothing on its own: a solve is driven
        // by requirements, and an ordinary `lock` has none beyond its --add.
        // An upgraded crate becomes a root requiring its own compatibility
        // range, which is what its declaration has always meant and what
        // cargo update honours. Transitive crates are not rooted; they move
        // when the crate that needs them asks for something newer.
        for d in &decls {
            if !d.root || !upgrading(&args.upgrade, &d.crate_name) {
                continue;
            }
            let v = Version::parse(&d.version)
                .with_context(|| format!("Bad version {} for {}", d.version, d.crate_name))?;
            let req = semver::VersionReq::parse(&format!("^{}", v))
                .with_context(|| format!("Bad requirement for {}", d.crate_name))?;
            solver.require(&d.crate_name, req);
        }

        let toolchain = if args.ignore_msrv {
            None
        } else {
            toolchain_for_msrv(args, build_text)
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

    report_upgrades(&upgraded, args.upgrade.is_some());

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
        eprintln!(
            "lock: + {}@{}{}",
            name,
            version,
            if root { " (root)" } else { "" }
        );
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
            platforms: None,
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

    finish_lock(args, decls)
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
            // The upgrade already happened and is written down. Healing only
            // fills in what resolution found missing, and does that against
            // the versions now declared rather than moving them again.
            upgrade: None,
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
        upgrade: args.upgrade.clone(),
        toolchain_version: args.toolchain_version.clone(),
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
            platforms: None,
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
            platforms: None,
            root: false,
            default_features: true,
            git_repo: String::new(),
            git_revision: String::new(),
        });
        let renames = normalize_names(&mut decls).unwrap();
        assert_eq!(renames.get("serde_old").unwrap(), "serde-1.0.100");
        // The newest keeps the plain name
        assert!(decls
            .iter()
            .any(|d| d.version == "1.0.228" && d.subrepo() == "serde"));
    }

    #[test]
    fn import_lockfile_sources() {
        let dir =
            std::env::temp_dir().join(format!("please_rust_sync_test_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("Cargo.lock"),
            r#"
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
"#,
        )
        .unwrap();
        fs::write(
            dir.join("Cargo.toml"),
            r#"
[package]
name = "ws"
version = "0.0.0"

[dependencies]
fresh = { version = "2", features = ["extra"] }
"#,
        )
        .unwrap();

        let mut decls = parse(BUILD);
        // serde already declared but without this hash
        decls[0].hashes.clear();
        import_cargo_lock(&dir.join("Cargo.lock"), &mut decls).unwrap();

        // Existing entry got the hash attached
        assert_eq!(
            decls
                .iter()
                .find(|d| d.crate_name == "serde")
                .unwrap()
                .hashes,
            vec!["feedface"]
        );
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
        // A git source on any other forge is imported as the URL it was
        // cloned from, rather than skipped with a note to write the rule by
        // hand. The rule derives the archive scheme from the host.
        let elsewhere = decls.iter().find(|d| d.crate_name == "elsewhere").unwrap();
        assert_eq!(elsewhere.git_repo, "https://gitlab.example.com/x/y");
        assert_eq!(elsewhere.git_revision, "deadbeef");
        // A path dependency has no source to fetch, so it stays skipped.
        assert!(!decls.iter().any(|d| d.crate_name == "local_thing"));
    }

    #[test]
    fn repo_root_walks_to_plzconfig() {
        let dir =
            std::env::temp_dir().join(format!("please_rust_root_test_{}", std::process::id()));
        let nested = dir.join("third_party/rust");
        fs::create_dir_all(&nested).unwrap();
        fs::write(dir.join(".plzconfig"), "").unwrap();
        fs::write(nested.join("BUILD"), "").unwrap();
        assert_eq!(
            repo_root(&nested.join("BUILD")),
            dir.canonicalize().unwrap()
        );
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
            (
                "app",
                "1.0.0",
                "[package]\nname = \"app\"\nversion = \"1.0.0\"\n\n[dependencies]\nutil = \"1\"\n",
            ),
            (
                "util",
                "1.5.0",
                "[package]\nname = \"util\"\nversion = \"1.5.0\"\n",
            ),
            (
                "unused",
                "0.1.0",
                "[package]\nname = \"unused\"\nversion = \"0.1.0\"\n",
            ),
        ] {
            let cdir = store.join(format!("{}-{}", name, version));
            fs::create_dir_all(&cdir).unwrap();
            fs::write(cdir.join("Cargo.toml"), manifest).unwrap();
        }

        let build_file = dir.join("BUILD");
        fs::write(
            &build_file,
            r#"rust_repo(
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
"#,
        )
        .unwrap();

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
        let dir =
            std::env::temp_dir().join(format!("please_rust_sync_missing_{}", std::process::id()));
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
                (
                    "default".to_string(),
                    vec!["std".to_string(), "dep:mandatory_opt".to_string()],
                ),
                ("std".to_string(), vec!["helper/fast".to_string()]),
            ]
            .into_iter()
            .collect(),
            features2: Some(
                [("weakling".to_string(), vec!["other?/x".to_string()])]
                    .into_iter()
                    .collect(),
            ),
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
        assert!(target_applies(
            "x86_64-unknown-linux-gnu",
            "x86_64-unknown-linux-gnu"
        ));
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
        fs::write(
            cache.join("ti/ny/tinydep"),
            concat!(
                r#"{"name":"tinydep","vers":"1.2.0","deps":[],"cksum":"ccc333","features":{}}"#,
                "\n"
            ),
        )
        .unwrap();

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
            upgrade: None,
            toolchain_version: None,
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

    /// MSRV filtering compares against the toolchain the repo declares. It
    /// used to be scraped from whichever file the declarations came from, so
    /// a repo keeping the two apart resolved with no filter at all and said
    /// so in a line that read as informational.
    #[test]
    fn the_toolchain_is_found_even_when_it_is_declared_elsewhere() {
        let dir = std::env::temp_dir().join(format!("please_rust_msrv_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("third_party/rust")).unwrap();
        fs::create_dir_all(dir.join("third_party/crates")).unwrap();
        fs::write(dir.join(".plzconfig"), "").unwrap();
        fs::write(
            dir.join("third_party/rust/BUILD"),
            "rust_toolchain(\n    name = \"toolchain\",\n    version = \"1.74.0\",\n)\n",
        )
        .unwrap();
        let decls = dir.join("third_party/crates/BUILD");
        fs::write(&decls, "").unwrap();

        let args = |tv: Option<&str>| LockCmdArgs {
            upgrade: None,
            toolchain_version: tv.map(str::to_string),
            build_file: decls.clone(),
            third_party_folder: "third_party/crates".to_string(),
            add: vec![],
            index_url: "https://index.invalid".to_string(),
            cache_dir: None,
            offline: true,
            target: "x86_64-unknown-linux-gnu".to_string(),
            targets: "x86_64-unknown-linux-gnu".to_string(),
            curl: "false".to_string(),
            plz: "".to_string(),
            greedy: false,
            ignore_msrv: false,
            features: None,
        };

        // The declarations file has no toolchain, so this is the case that
        // used to silently disable filtering.
        assert_eq!(
            toolchain_for_msrv(&args(None), ""),
            Some(Version::parse("1.74.0").unwrap())
        );

        // An explicit version wins over anything found.
        assert_eq!(
            toolchain_for_msrv(&args(Some("1.60.0")), ""),
            Some(Version::parse("1.60.0").unwrap())
        );

        // A toolchain beside the declarations is still read without a search.
        assert_eq!(
            toolchain_for_msrv(
                &args(None),
                "rust_toolchain(\n    version = \"1.80.0\",\n)\n"
            ),
            Some(Version::parse("1.80.0").unwrap())
        );

        // Two disagreeing toolchains resolve for the oldest, because a
        // declaration set is shared and has to build for whoever has the
        // oldest rustc.
        fs::create_dir_all(dir.join("services/other")).unwrap();
        fs::write(
            dir.join("services/other/BUILD"),
            "rust_toolchain(\n    name = \"t2\",\n    version = \"1.70.0\",\n)\n",
        )
        .unwrap();
        assert_eq!(
            toolchain_for_msrv(&args(None), ""),
            Some(Version::parse("1.70.0").unwrap())
        );

        // No toolchain anywhere is not an error, but it is not silent either.
        let bare =
            std::env::temp_dir().join(format!("please_rust_msrv_none_{}", std::process::id()));
        let _ = fs::remove_dir_all(&bare);
        fs::create_dir_all(&bare).unwrap();
        fs::write(bare.join(".plzconfig"), "").unwrap();
        let bare_build = bare.join("BUILD");
        fs::write(&bare_build, "").unwrap();
        let mut a = args(None);
        a.build_file = bare_build;
        assert_eq!(toolchain_for_msrv(&a, ""), None);

        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&bare);
    }

    /// An upgrade can move a version backwards, and the report has to say so.
    /// Taking the newest generic-array costs crypto-common a version, because
    /// the newer crypto-common pins generic-array exactly. Found by running
    /// --upgrade over rust-corpus.
    #[test]
    fn a_version_that_moved_backwards_is_not_reported_as_an_upgrade() {
        let moved = vec![
            (
                "crypto-common".to_string(),
                "0.1.7".to_string(),
                Version::parse("0.1.6").unwrap(),
            ),
            (
                "generic-array".to_string(),
                "0.14.7".to_string(),
                Version::parse("0.14.9").unwrap(),
            ),
        ];
        // The marker is chosen per crate rather than for the run.
        let back = Version::parse(&moved[0].1).unwrap() > moved[0].2;
        let fwd = Version::parse(&moved[1].1).unwrap() < moved[1].2;
        assert!(back, "crypto-common went backwards");
        assert!(fwd, "generic-array went forwards");

        // Both forms print without panicking, over and under the list limit.
        report_upgrades(&moved, true);
        let many: Vec<(String, String, Version)> = (0..40)
            .map(|i| {
                (
                    format!("crate{}", i),
                    "1.0.0".to_string(),
                    Version::parse("1.1.0").unwrap(),
                )
            })
            .chain(moved.clone())
            .collect();
        report_upgrades(&many, true);
        report_upgrades(&many, false);
    }

    /// The acceptance test for --upgrade: deliberately old declarations move,
    /// the graph still resolves, and a second run changes nothing.
    #[test]
    fn upgrade_moves_old_declarations_and_is_then_a_no_op() {
        let dir = std::env::temp_dir().join(format!("please_rust_upgrade_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // A .plzconfig rather than a chdir: repo_root walks up for one and
        // falls back to the process cwd, which other tests are also using.
        fs::write(dir.join(".plzconfig"), "").unwrap();

        let cache = dir.join("index-cache");
        fs::create_dir_all(cache.join("he/xl")).unwrap();
        // Three releases in one compatibility bucket. 0.4.1 is what the repo
        // declares; 0.4.9 is what it should reach.
        fs::write(
            cache.join("he/xl/hexlib"),
            concat!(
                r#"{"name":"hexlib","vers":"0.4.1","deps":[{"name":"tinydep","req":"^1","features":[],"optional":false,"default_features":true,"target":null,"kind":"normal"}],"cksum":"old111","features":{}}"#,
                "\n",
                r#"{"name":"hexlib","vers":"0.4.5","deps":[{"name":"tinydep","req":"^1","features":[],"optional":false,"default_features":true,"target":null,"kind":"normal"}],"cksum":"mid555","features":{}}"#,
                "\n",
                r#"{"name":"hexlib","vers":"0.4.9","deps":[{"name":"tinydep","req":"^1","features":[],"optional":false,"default_features":true,"target":null,"kind":"normal"}],"cksum":"new999","features":{}}"#,
                "\n",
            ),
        )
        .unwrap();
        fs::create_dir_all(cache.join("ti/ny")).unwrap();
        fs::write(
            cache.join("ti/ny/tinydep"),
            concat!(
                r#"{"name":"tinydep","vers":"1.0.0","deps":[],"cksum":"tiny100","features":{}}"#,
                "\n",
                r#"{"name":"tinydep","vers":"1.4.0","deps":[],"cksum":"tiny140","features":{}}"#,
                "\n",
            ),
        )
        .unwrap();

        let store = dir.join("plz-out/gen/third_party/rust");
        for (name, ver) in [
            ("hexlib", "0.4.1"),
            ("hexlib", "0.4.9"),
            ("tinydep", "1.0.0"),
            ("tinydep", "1.4.0"),
        ] {
            let cdir = store.join(format!("{}-{}", name, ver));
            fs::create_dir_all(&cdir).unwrap();
            let manifest = if name == "hexlib" {
                format!(
                    "[package]\nname = \"{}\"\nversion = \"{}\"\n\n[dependencies]\ntinydep = \"1\"\n",
                    name, ver
                )
            } else {
                format!("[package]\nname = \"{}\"\nversion = \"{}\"\n", name, ver)
            };
            fs::write(cdir.join("Cargo.toml"), manifest).unwrap();
        }

        let build_file = dir.join("BUILD");
        let old = concat!(
            "rust_repo(\n    name = \"hexlib\",\n    crate = \"hexlib\",\n",
            "    version = \"0.4.1\",\n    hashes = [\"old111\"],\n)\n\n",
            "rust_repo(\n    name = \"tinydep\",\n    crate = \"tinydep\",\n",
            "    version = \"1.0.0\",\n    hashes = [\"tiny100\"],\n    indirect = True,\n)\n",
        );
        fs::write(&build_file, old).unwrap();

        let args = |upgrade| LockCmdArgs {
            upgrade,
            toolchain_version: None,
            build_file: build_file.clone(),
            third_party_folder: "third_party/rust".to_string(),
            add: vec![],
            index_url: "https://index.invalid".to_string(),
            cache_dir: Some(cache.clone()),
            offline: true,
            target: "x86_64-unknown-linux-gnu".to_string(),
            targets: "x86_64-unknown-linux-gnu".to_string(),
            curl: "false".to_string(),
            plz: "".to_string(),
            greedy: false,
            ignore_msrv: true,
            features: None,
        };

        // Without --upgrade the declared versions are preferences that still
        // fit, so nothing moves.
        lock(args(None)).unwrap();
        let kept = fs::read_to_string(&build_file).unwrap();
        assert!(kept.contains("0.4.1"), "{}", kept);
        assert!(kept.contains("1.0.0"), "{}", kept);

        // Naming one crate moves that one and leaves the other alone.
        lock(args(Some(vec!["hexlib".to_string()]))).unwrap();
        let one = fs::read_to_string(&build_file).unwrap();
        assert!(one.contains("0.4.9"), "hexlib should move: {}", one);
        assert!(one.contains("new999"), "hash should follow: {}", one);
        // tinydep is transitive: it follows hexlib's requirement rather than
        // being named, so what matters is that it still resolves.
        assert!(
            one.contains("tinydep"),
            "tinydep should still be there: {}",
            one
        );

        // Bare --upgrade moves the rest.
        lock(args(Some(Vec::new()))).unwrap();
        let all = fs::read_to_string(&build_file).unwrap();
        assert!(all.contains("1.4.0"), "tinydep should move: {}", all);
        assert!(all.contains("tiny140"), "hash should follow: {}", all);

        // And running it again changes nothing.
        lock(args(Some(Vec::new()))).unwrap();
        assert_eq!(fs::read_to_string(&build_file).unwrap(), all);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn lock_offline_without_cache_errors() {
        let dir =
            std::env::temp_dir().join(format!("please_rust_lock_offline_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let build_file = dir.join("BUILD");
        fs::write(&build_file, "").unwrap();
        let err = lock(LockCmdArgs {
            upgrade: None,
            toolchain_version: None,
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

    /// A crate every covered platform reaches carries no attribute: absent
    /// means anywhere, which is what almost every crate is. One that only
    /// macOS reaches says so, and rust_repo turns that into plz's `manual`
    /// label on machines where it cannot compile - objc2 refuses in a
    /// compile_error!, so without this `plz build //...` breaks on linux.
    ///
    /// What is written is a property of the crate, not of the machine that
    /// ran sync, which is what lets one checked-in file serve a Mac and a
    /// linux box at once.
    /// A feature can be written in the entries list as easily as in the
    /// rust_repo block - the entries list is where it is visible - and sync
    /// read only the block. rust-corpus lost `bundled` from libsqlite3-sys to
    /// an unrelated `lock --add`, which is the whole reason that crate is in
    /// the corpus: without the feature it vendors no C and tests nothing.
    #[test]
    fn entries_list_features_survive() {
        let build = r#"
rust_resolve(
    name = "rust_lock",
    entries = [
        "libsqlite3_sys|libsqlite3-sys|0.38.2|bundled|true|true",
        "plain|plain|1.0.0||false|true",
    ],
)
"#;
        let found = parse_resolve_features(build);
        assert_eq!(
            found.get("libsqlite3_sys"),
            Some(&vec!["bundled".to_string()])
        );
        // No features is no entry, rather than an empty one to adopt.
        assert!(!found.contains_key("plain"));
    }

    /// `platforms` is a statement about every platform resolution covers, so a
    /// run that resolved one triple cannot narrow it. Recomputing anyway turned
    /// every `platforms = ["macos"]` in rust-corpus into nothing, from a
    /// command that was adding an unrelated crate.
    #[test]
    fn a_single_platform_run_does_not_narrow_recorded_platforms() {
        let mut file_oses = BTreeSet::new();
        file_oses.insert("macos".to_string());
        let mut covered = BTreeSet::new();
        covered.insert("linux".to_string());
        assert!(
            !may_refresh_platforms(&file_oses, &covered),
            "a linux-only run must not restate what the file says about macos"
        );
        covered.insert("macos".to_string());
        assert!(
            may_refresh_platforms(&file_oses, &covered),
            "a run covering both may restate them"
        );
        // A file that names no platforms constrains nothing.
        assert!(may_refresh_platforms(&BTreeSet::new(), &BTreeSet::new()));
    }

    #[test]
    fn platforms_is_written_only_when_a_crate_is_gated() {
        let mut everywhere = decl("serde", "1.0.0", true);
        everywhere.platforms = None;
        assert!(
            !emit_decl(&everywhere).contains("platforms"),
            "{}",
            emit_decl(&everywhere)
        );

        let mut apple = decl("objc2", "0.6.4", false);
        apple.platforms = Some(["macos".to_string()].into_iter().collect());
        assert!(
            emit_decl(&apple).contains("platforms = [\"macos\"]"),
            "{}",
            emit_decl(&apple)
        );

        // Reachable on no platform this repo covers, which is different from
        // reachable on all of them.
        let mut nowhere = decl("windows-sys", "0.61.2", false);
        nowhere.platforms = Some(BTreeSet::new());
        assert!(
            emit_decl(&nowhere).contains("platforms = []"),
            "{}",
            emit_decl(&nowhere)
        );
    }

    fn decl(crate_name: &str, version: &str, root: bool) -> Decl {
        Decl {
            name: None,
            crate_name: crate_name.to_string(),
            version: version.to_string(),
            features: vec![],
            hashes: vec![],
            passthrough: vec![],
            leading_comments: vec![],
            span: None,
            imported: true,
            platforms: None,
            root,
            default_features: true,
            git_repo: String::new(),
            git_revision: String::new(),
        }
    }

    /// Asking for a crate that is already present as somebody else's
    /// dependency makes it a root. Otherwise it is reachable only through
    /// whoever pulled it in, and a version bump elsewhere can leave the crate
    /// this repo asked for unreferenced - which is how a corpus of 251 named
    /// crates ended up with 8 of them unreachable.
    #[test]
    fn add_promotes_an_existing_declaration_to_a_root() {
        let mut decls = vec![
            decl("itertools", "0.14.0", false),
            decl("itertools", "0.10.5", false),
            decl("serde", "1.0.0", false),
        ];
        let promoted = promote_added_roots(&mut decls, &["itertools@0.14".to_string()]);
        assert_eq!(promoted, vec!["itertools@0.14.0".to_string()]);
        assert!(decls[0].root, "the matching version is a root");
        assert!(!decls[1].root, "a version the requirement excludes is not");
        assert!(!decls[2].root, "an unrelated crate is untouched");
    }

    /// A wildcard names every version of the crate, which is what
    /// `lock --add x@*` means.
    #[test]
    fn add_with_a_wildcard_promotes_each_matching_declaration() {
        let mut decls = vec![decl("http", "1.3.1", false), decl("http", "0.2.12", false)];
        let promoted = promote_added_roots(&mut decls, &["http@*".to_string()]);
        assert_eq!(promoted.len(), 2, "{:?}", promoted);
    }

    #[test]
    fn lock_nothing_to_do_when_satisfied() {
        let dir =
            std::env::temp_dir().join(format!("please_rust_lock_noop_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let build_file = dir.join("BUILD");
        fs::write(&build_file, "rust_repo(\n    name = \"present\",\n    crate = \"present\",\n    version = \"1.0.0\",\n)\n").unwrap();
        let before = fs::read_to_string(&build_file).unwrap();
        lock(LockCmdArgs {
            upgrade: None,
            toolchain_version: None,
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
    /// `--add` keeps declared versions where they still fit, so adding one
    /// crate never churns the rest. `--upgrade` drops exactly that
    /// preference, for everything or for named crates.
    #[test]
    fn upgrade_drops_the_declared_version_as_a_preference() {
        // Flag absent: every declared version stays preferred.
        assert!(!upgrading(&None, "serde"));

        // Bare --upgrade: nothing is preferred, so everything moves.
        let all = Some(Vec::new());
        assert!(upgrading(&all, "serde"));
        assert!(upgrading(&all, "anything-at-all"));

        // Named: only those crates move.
        let some = Some(vec!["serde".to_string(), "hex".to_string()]);
        assert!(upgrading(&some, "serde"));
        assert!(upgrading(&some, "hex"));
        assert!(!upgrading(&some, "tokio"));

        // A crate is declared under whichever separator its package uses and
        // typed under whichever the user remembers.
        let dashed = Some(vec!["serde-json".to_string()]);
        assert!(upgrading(&dashed, "serde_json"));
        let under = Some(vec!["rustls_webpki".to_string()]);
        assert!(upgrading(&under, "rustls-webpki"));
    }
}
