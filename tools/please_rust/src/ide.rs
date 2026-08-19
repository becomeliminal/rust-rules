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

    /// Where to write the project file
    #[arg(long, default_value = "rust-project.json")]
    pub output: PathBuf,
}

#[derive(Serialize)]
struct Project {
    #[serde(skip_serializing_if = "Option::is_none")]
    sysroot: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sysroot_src: Option<String>,
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

#[derive(Serialize)]
struct DepEntry {
    /// Index into the crates array. rust-project.json addresses deps
    /// positionally, which is why the graph has to be walked twice - once to
    /// assign indices and once to resolve them.
    #[serde(rename = "crate")]
    krate: usize,
    name: String,
}

/// A crate's identity in the lock: its subrepo, and which unit of it. The
/// host unit of a dual crate is a different crate to rust-analyzer - it is
/// compiled for a different platform and may have different features.
type Key = (String, bool);

pub fn run(args: IdeArgs) -> Result<()> {
    let lock = crate::resolve::LockFile::load(&args.lock)
        .with_context(|| format!("reading {}", args.lock.display()))?;

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
        .map(|(i, (k, _))| (k.clone(), i))
        .collect();

    // Pass two: describe each one.
    let mut crates = Vec::new();
    let mut skipped = Vec::new();
    for ((subrepo, host), entry) in &order {
        match describe(&args.third_party_dir, subrepo, *host, entry, &lock, &index) {
            Ok(c) => crates.push(c),
            Err(e) => {
                skipped.push(format!("{}: {:#}", subrepo, e));
                // A crate that cannot be described must still occupy its
                // index, or every dep after it points at the wrong crate.
                crates.push(placeholder(entry));
            }
        }
    }

    let project = Project {
        sysroot: args.sysroot.as_ref().map(|p| abs(p)),
        sysroot_src: args.sysroot_src.as_ref().map(|p| abs(p)),
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
    host: bool,
    entry: &crate::resolve::LockEntry,
    lock: &crate::resolve::LockFile,
    index: &BTreeMap<Key, usize>,
) -> Result<CrateEntry> {
    let dir = third_party.join(subrepo);
    let manifest_path = dir.join("Cargo.toml");
    let bytes = std::fs::read(&manifest_path)
        .with_context(|| format!("reading {}", manifest_path.display()))?;
    let manifest = crate::resolve::parse_manifest(&bytes)
        .with_context(|| format!("parsing {}", manifest_path.display()))?;

    // A library root if there is one, and a binary's otherwise: a bin-only
    // crate like bindgen-cli has no lib.rs, and skipping it would leave a
    // crate rust-analyzer cannot follow a dep into.
    let lib_path = manifest
        .lib
        .as_ref()
        .and_then(|l| l.path.clone())
        .unwrap_or_else(|| "src/lib.rs".to_string());
    let mut root = dir.join(&lib_path);
    if !root.exists() {
        let bin = manifest
            .bin
            .iter()
            .find_map(|b| b.path.clone())
            .unwrap_or_else(|| "src/main.rs".to_string());
        root = dir.join(&bin);
    }
    anyhow::ensure!(root.exists(), "no root module at {}", root.display());

    let crate_type = crate::generate::determine_crate_type(&manifest, true);
    let is_proc_macro = crate_type == "proc-macro";

    // The crate name is what source imports, which is not the package name
    // whenever a manifest sets [lib] name.
    let lib_name = manifest
        .lib
        .as_ref()
        .and_then(|l| l.name.clone())
        .unwrap_or_else(|| entry.crate_name.clone());
    let ident = lib_name.replace('-', "_");

    let mut cfg: Vec<String> = entry
        .features
        .iter()
        .map(|f| format!("feature=\"{}\"", f))
        .collect();
    cfg.extend(buildscript_cfgs(&dir));

    let env: BTreeMap<String, String> = manifest
        .package
        .as_ref()
        .map(|p| crate::build_script::package_env(p).into_iter().collect())
        .unwrap_or_default();

    // A host unit's deps are the host units of what it depends on, which is
    // exactly what `target_name` records.
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
    let _ = (lock, host);

    let proc_macro_dylib_path = if is_proc_macro {
        let tag = entry.version.replace(['.', '+'], "_");
        let f = dir.join(format!(
            "lib{}-{}{}",
            ident,
            tag,
            std::env::consts::DLL_SUFFIX
        ));
        f.exists().then(|| abs(&f))
    } else {
        None
    };

    Ok(CrateEntry {
        display_name: ident,
        root_module: abs(&root),
        edition: edition_str(&manifest).to_string(),
        deps,
        cfg,
        env,
        is_proc_macro,
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

fn edition_str(manifest: &cargo_toml::Manifest) -> &'static str {
    match manifest.package.as_ref().map(|p| p.edition.get()) {
        Some(Ok(cargo_toml::Edition::E2015)) => "2015",
        Some(Ok(cargo_toml::Edition::E2018)) => "2018",
        Some(Ok(cargo_toml::Edition::E2024)) => "2024",
        _ => "2021",
    }
}

/// rust-analyzer reads the file from wherever it is launched, so every path
/// in it has to be absolute.
fn abs(p: &Path) -> String {
    std::fs::canonicalize(p)
        .unwrap_or_else(|_| p.to_path_buf())
        .display()
        .to_string()
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
