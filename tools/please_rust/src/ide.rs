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
    /// The resolved lock, as `rust_resolve` produces it
    #[arg(long)]
    pub lock: PathBuf,

    /// Directory holding the generated crate subrepos, one per declaration
    #[arg(long)]
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

    /// Where to write the project file
    #[arg(long, default_value = "rust-project.json")]
    pub output: PathBuf,
}

/// What a first-party rule knows about itself, which is everything
/// rust-project.json wants and none of which reaches this tool any other way.
#[derive(serde::Deserialize)]
struct FirstParty {
    display_name: String,
    /// Repo-relative; made absolute against `--repo-root`.
    root_module: String,
    edition: String,
    #[serde(default)]
    features: Vec<String>,
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

#[derive(Serialize)]
struct SysrootProject {
    crates: Vec<CrateEntry>,
}

#[derive(Serialize)]
struct CrateEntry {
    display_name: String,
    root_module: String,
    edition: String,
    deps: Vec<DepEntry>,
    cfg: Vec<String>,
    env: BTreeMap<String, String>,
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

/// A crate's identity in the lock: its subrepo, and which unit of it. The
/// host unit of a dual crate is a different crate to rust-analyzer - it is
/// compiled for a different platform and may have different features.
type Key = (String, bool);

pub fn run(args: IdeArgs) -> Result<()> {
    let lock = crate::resolve::LockFile::load(&args.lock)
        .with_context(|| format!("reading {}", args.lock.display()))?;

    // The sysroot is described as a nested project rather than as crates in
    // the main list, so rust-analyzer registers it as the sysroot and its
    // lang items attach. Indices inside it are its own.
    let mut sysroot_project = None;
    if let Some(src) = &args.sysroot_src {
        let mut sys: Vec<CrateEntry> = Vec::new();
        let mut at: BTreeMap<&str, usize> = BTreeMap::new();
        for (name, _) in SYSROOT_CRATES {
            at.insert(name, sys.len());
            sys.push(CrateEntry {
                display_name: name.to_string(),
                // Relative to sysroot_src, which is what rust-analyzer
                // absolutizes these against.
                root_module: format!("{}/src/lib.rs", name),
                edition: sysroot_edition(&src.join(name)),
                deps: Vec::new(),
                cfg: Vec::new(),
                env: BTreeMap::new(),
                is_proc_macro: false,
                proc_macro_dylib_path: None,
                is_workspace_member: false,
            });
        }
        for (name, deps) in SYSROOT_CRATES {
            let i = at[name];
            sys[i].deps = deps
                .iter()
                .filter_map(|d| {
                    at.get(d).map(|j| DepEntry {
                        krate: *j,
                        name: d.to_string(),
                    })
                })
                .collect();
        }
        sysroot_project = Some(SysrootProject { crates: sys });
    }
    let mut crates: Vec<CrateEntry> = Vec::new();
    let offset = 0usize;

    // Pass one: assign an index to every crate, so deps can name them.
    let mut order: Vec<(Key, &crate::resolve::LockEntry)> = Vec::new();
    for (subrepo, entry) in &lock.crates {
        order.push(((subrepo.clone(), false), entry));
    }
    for (subrepo, entry) in &lock.host_crates {
        order.push(((subrepo.clone(), true), entry));
    }
    let index: BTreeMap<Key, usize> = order
        .iter()
        .enumerate()
        .map(|(i, (k, _))| (k.clone(), i + offset))
        .collect();

    // Pass two: describe each one.
    let mut skipped = Vec::new();
    for ((subrepo, _host), entry) in &order {
        match describe(&args.third_party_dir, subrepo, entry, &index) {
            Ok(c) => crates.push(c),
            Err(e) => {
                skipped.push(format!("{}: {:#}", subrepo, e));
                // A crate that cannot be described must still occupy its
                // index, or every dep after it points at the wrong crate.
                crates.push(placeholder(entry));
            }
        }
    }

    // First-party crates go after the third-party ones so the indices
    // assigned above stay valid.
    let mut first: Vec<(String, FirstParty)> = Vec::new();
    for path in &args.first_party {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let fp: FirstParty =
            serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        first.push((path.display().to_string(), fp));
    }
    let mut by_label: BTreeMap<String, usize> = BTreeMap::new();
    for (i, (_, fp)) in first.iter().enumerate() {
        by_label.insert(fp.display_name.clone(), crates.len() + i);
    }
    for (_, fp) in &first {
        let mut deps = Vec::new();
        for d in &fp.deps {
            let name = d.name.replace('-', "_");
            // A first-party dep names another fragment; anything else is a
            // declaration in the lock.
            if let Some(i) = by_label.get(&name) {
                deps.push(DepEntry { krate: *i, name });
            } else if let Some(sub) = label_subrepo(&d.label) {
                if let Some(i) = index.get(&(sub, false)) {
                    // The crate's own name rather than the label's: a label
                    // carries the package, and rustls-webpki builds webpki.
                    let resolved = crates[*i].display_name.clone();
                    deps.push(DepEntry {
                        krate: *i,
                        name: resolved,
                    });
                }
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
                .collect(),
            env: BTreeMap::new(),
            is_proc_macro: fp.is_proc_macro,
            proc_macro_dylib_path: None,
            // This is the code being worked on, which is what the flag means:
            // rust-analyzer checks these on save and only indexes the rest.
            is_workspace_member: true,
        });
    }

    let project = Project {
        sysroot: args.sysroot.as_ref().map(|p| rel(p)),
        sysroot_src: args.sysroot_src.as_ref().map(|p| rel(p)),
        sysroot_project,
        crates,
    };
    let json = serde_json::to_string_pretty(&project)?;
    std::fs::write(&args.output, json + "\n")
        .with_context(|| format!("writing {}", args.output.display()))?;

    for s in &skipped {
        eprintln!("ide: could not describe {}", s);
    }
    eprintln!(
        "ide: wrote {} crates to {}",
        project.crates.len(),
        args.output.display()
    );
    Ok(())
}

/// An entry that keeps the indices aligned when a crate cannot be read. It is
/// deliberately minimal rather than absent: rust-analyzer tolerates a crate
/// it cannot open far better than it tolerates deps pointing one crate to the
/// left of where they should.
fn placeholder(entry: &crate::resolve::LockEntry) -> CrateEntry {
    CrateEntry {
        display_name: entry.crate_name.replace('-', "_"),
        root_module: String::new(),
        edition: "2021".to_string(),
        deps: Vec::new(),
        cfg: Vec::new(),
        env: BTreeMap::new(),
        is_proc_macro: false,
        proc_macro_dylib_path: None,
        is_workspace_member: false,
    }
}

fn describe(
    third_party: &Path,
    subrepo: &str,
    entry: &crate::resolve::LockEntry,
    index: &BTreeMap<Key, usize>,
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

    // The crate name is what source imports, which is not the package name
    // whenever a manifest sets [lib] name.
    let ident = entry.crate_name.replace('-', "_");

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
        if let Some(i) = index.get(&(d.subrepo.clone(), is_host)) {
            deps.push(DepEntry {
                krate: *i,
                name: d.name.replace('-', "_"),
            });
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
        is_proc_macro: entry.is_proc_macro,
        proc_macro_dylib_path,
        is_workspace_member: false,
    })
}

/// The cfgs a build script set, which are frequently the difference between
/// a crate that analyses and one that is half red. libc alone sets dozens.
/// Already parsed and written next to the crate by `build-script`, so this
/// reads rather than recomputes.
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
