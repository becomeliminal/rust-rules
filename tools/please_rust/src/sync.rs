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

    /// Target triple to resolve for
    #[arg(long, default_value = "x86_64-unknown-linux-gnu")]
    pub target: String,

    /// Where to write the resolved lock (defaults to rust.lock next to the BUILD file)
    #[arg(long)]
    pub lock_output: Option<PathBuf>,

    /// plz binary used to fetch missing crate downloads ("" disables)
    #[arg(long, default_value = "plz")]
    pub plz: String,

    /// Keep existing subrepo names instead of normalizing them
    #[arg(long)]
    pub no_rename: bool,

    /// Report what would change without writing anything
    #[arg(long)]
    pub dry_run: bool,
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
}

impl Decl {
    fn subrepo(&self) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| self.crate_name.replace('-', "_"))
    }
}

pub fn run(args: SyncArgs) -> Result<()> {
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
        })
        .collect();
    let mut lock = resolve_entries(&entries, &args.target)?;

    // Crates imported this run that did not activate for this target (e.g.
    // windows-only) are dropped again rather than declared dead weight.
    let before = decls.len();
    decls.retain(|d| !d.imported || lock.crates.contains_key(&d.subrepo()));
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
            })
            .collect();
        lock = resolve_entries(&entries, &args.target)?;
    }

    let lock_output = args.lock_output.clone().unwrap_or_else(|| {
        args.build_file
            .parent()
            .unwrap_or(Path::new("."))
            .join("rust.lock")
    });

    let new_build = rewrite_build(&lines, &decls);

    if args.dry_run {
        eprintln!(
            "sync (dry run): would write {} declarations, {} resolved crates",
            decls.len(),
            lock.crates.len()
        );
        return Ok(());
    }

    fs::write(&args.build_file, new_build)
        .with_context(|| format!("Failed to write {}", args.build_file.display()))?;
    fs::write(&lock_output, serde_json::to_string_pretty(&lock)? + "\n")
        .with_context(|| format!("Failed to write {}", lock_output.display()))?;
    eprintln!(
        "sync: wrote {} declarations to {} and {} resolved crates to {}",
        decls.len(),
        args.build_file.display(),
        lock.crates.len(),
        lock_output.display()
    );
    Ok(())
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

const MANAGED_KEYS: &[&str] = &["name", "crate", "version", "features", "hashes", "dep_overrides"];

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

        let name = get("name");
        decls.push(Decl {
            root: name.is_some(),
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
        // Only registry crates can be fetched from crates.io
        if name.is_empty() || !source.contains("registry") {
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
        if let Ok(manifest) = cargo_toml::Manifest::from_slice(&mcontent) {
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
    fs::write(&args.build_file, rewrite_build(&lines, decls))?;

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
/// imported entries at the end.
fn rewrite_build(lines: &[String], decls: &[Decl]) -> String {
    // Map from original start line -> canonical replacement text
    let mut replacements: BTreeMap<usize, (usize, String)> = BTreeMap::new();
    let mut appended = String::new();

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
    if !d.features.is_empty() {
        let feats: Vec<String> = d.features.iter().map(|f| format!("\"{}\"", f)).collect();
        s.push_str(&format!("    features = [{}],\n", feats.join(", ")));
    }
    if !d.hashes.is_empty() {
        let hs: Vec<String> = d.hashes.iter().map(|h| format!("\"{}\"", h)).collect();
        s.push_str(&format!("    hashes = [{}],\n", hs.join(", ")));
    }
    for p in &d.passthrough {
        s.push_str(&format!("    {},\n", p.trim()));
    }
    s.push_str(")\n");
    s
}
