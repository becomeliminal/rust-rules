//! Imports a cargo workspace as first-party BUILD files.
//!
//! `sync --import-workspace path/to/workspace` walks the workspace members
//! and writes a BUILD file next to each: rust_library / rust_binary for the
//! crate's products, rust_test for unit and integration tests. Path
//! dependencies become member labels, registry dependencies become
//! //third_party/rust aliases (declared by the accompanying Cargo.lock
//! import). This is the puku analog for Rust: the single command that turns
//! a cargo repo into a plz one.

use anyhow::{Context, Result};
use cargo_toml::{Dependency, Manifest};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub struct WorkspaceImport {
    /// The workspace's Cargo.lock, if present (chained into --import)
    pub lockfile: Option<PathBuf>,
    pub members: usize,
    pub written: usize,
}

struct Member {
    dir: PathBuf,
    rel: PathBuf, // relative to cwd; label prefix
    manifest: Manifest,
    name: String,
}

pub fn import_workspace(ws: &Path, third_party_folder: &str) -> Result<WorkspaceImport> {
    let ws_root = if ws.ends_with("Cargo.toml") {
        ws.parent().unwrap_or(Path::new(".")).to_path_buf()
    } else {
        ws.to_path_buf()
    };
    let ws_manifest_path = ws_root.join("Cargo.toml");
    let bytes = fs::read(&ws_manifest_path)
        .with_context(|| format!("Failed to read {}", ws_manifest_path.display()))?;
    let ws_manifest =
        crate::resolve::parse_manifest(&bytes).context("Failed to parse workspace manifest")?;

    // Member set: expanded workspace.members globs, or the root itself for a
    // single-package repo.
    let mut member_dirs: Vec<PathBuf> = Vec::new();
    if let Some(workspace) = &ws_manifest.workspace {
        for pattern in &workspace.members {
            if pattern.contains('*') {
                let full = ws_root.join(pattern);
                let pattern_str = full.to_string_lossy().to_string();
                for entry in glob_dirs(&pattern_str)? {
                    if entry.join("Cargo.toml").exists() {
                        member_dirs.push(entry);
                    }
                }
            } else {
                let dir = ws_root.join(pattern);
                if dir.join("Cargo.toml").exists() {
                    member_dirs.push(dir);
                }
            }
        }
        // The root can be a member too (a package alongside [workspace])
        if ws_manifest.package.is_some() {
            member_dirs.push(ws_root.clone());
        }
    } else if ws_manifest.package.is_some() {
        member_dirs.push(ws_root.clone());
    } else {
        anyhow::bail!("{} has neither [workspace] nor [package]", ws_manifest_path.display());
    }
    member_dirs.sort();
    member_dirs.dedup();

    // Parse every member up front: path deps need the full name -> label map.
    let cwd = std::env::current_dir()?;
    let mut members: Vec<Member> = Vec::new();
    for dir in &member_dirs {
        let mpath = dir.join("Cargo.toml");
        let mbytes = fs::read(&mpath)
            .with_context(|| format!("Failed to read {}", mpath.display()))?;
        let manifest = crate::resolve::parse_manifest(&mbytes)
            .with_context(|| format!("Failed to parse {}", mpath.display()))?;
        let name = match &manifest.package {
            Some(p) => p.name.clone(),
            None => continue,
        };
        let abs = dir.canonicalize().unwrap_or_else(|_| dir.clone());
        let rel = abs
            .strip_prefix(&cwd)
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|_| dir.clone());
        members.push(Member { dir: dir.clone(), rel, manifest, name });
    }

    // canonical member dir -> label
    let mut labels: BTreeMap<PathBuf, String> = BTreeMap::new();
    for m in &members {
        let canon = m.dir.canonicalize().unwrap_or_else(|_| m.dir.clone());
        labels.insert(canon, format!("//{}:{}", m.rel.display(), m.name));
    }

    let mut written = 0;
    for m in &members {
        let build_path = m.dir.join("BUILD");
        if build_path.exists() {
            eprintln!(
                "import-workspace: {} already exists, skipping {}",
                build_path.display(),
                m.name
            );
            continue;
        }
        let content = emit_member_build(m, &ws_manifest, &labels, third_party_folder)?;
        fs::write(&build_path, content)
            .with_context(|| format!("Failed to write {}", build_path.display()))?;
        eprintln!("import-workspace: wrote {}", build_path.display());
        written += 1;
    }

    let lockfile = ws_root.join("Cargo.lock");
    Ok(WorkspaceImport {
        lockfile: lockfile.exists().then_some(lockfile),
        members: members.len(),
        written,
    })
}

/// Minimal glob: expands a single trailing `*` path segment to directories.
fn glob_dirs(pattern: &str) -> Result<Vec<PathBuf>> {
    let p = Path::new(pattern);
    let parent = p.parent().unwrap_or(Path::new("."));
    let leaf = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let mut out = Vec::new();
    if !leaf.contains('*') {
        out.push(p.to_path_buf());
        return Ok(out);
    }
    let (prefix, suffix) = leaf.split_once('*').unwrap();
    if let Ok(entries) = fs::read_dir(parent) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(prefix) && name.ends_with(suffix) {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Resolves a dependency to a label, looking through workspace inheritance.
fn dep_label(
    name: &str,
    dep: &Dependency,
    member_dir: &Path,
    ws_manifest: &Manifest,
    labels: &BTreeMap<PathBuf, String>,
    third_party_folder: &str,
) -> Option<String> {
    let dep = match dep {
        Dependency::Inherited(_) => {
            match ws_manifest
                .workspace
                .as_ref()
                .and_then(|w| w.dependencies.get(name))
            {
                Some(d) => d.clone(),
                None => {
                    eprintln!(
                        "import-workspace: warning: workspace dependency {} not found; using a third-party label",
                        name
                    );
                    return Some(third_party_label(name, third_party_folder));
                }
            }
        }
        other => other.clone(),
    };
    if let Some(detail) = dep.detail() {
        if let Some(path) = &detail.path {
            let target = member_dir.join(path);
            let canon = target.canonicalize().unwrap_or(target);
            return match labels.get(&canon) {
                Some(label) => Some(label.clone()),
                None => {
                    eprintln!(
                        "import-workspace: warning: path dependency {} ({}) is outside the workspace, skipping",
                        name,
                        canon.display()
                    );
                    None
                }
            };
        }
        if detail.git.is_some() {
            eprintln!(
                "import-workspace: warning: {} is a git dependency; declare it with rust_repo(git_repo = ...) in {}/BUILD",
                name, third_party_folder
            );
            return Some(third_party_label(name, third_party_folder));
        }
        if detail.optional {
            eprintln!(
                "import-workspace: warning: skipping optional dependency {} (enable it explicitly if a feature needs it)",
                name
            );
            return None;
        }
        // package = "real-name" renames: depend on the real crate
        if let Some(package) = &detail.package {
            eprintln!(
                "import-workspace: warning: {} renames {} (package = ...); first-party rules have no rename support, using the real name",
                name, package
            );
            return Some(third_party_label(package, third_party_folder));
        }
    }
    Some(third_party_label(name, third_party_folder))
}

fn third_party_label(name: &str, third_party_folder: &str) -> String {
    format!("//{}:{}", third_party_folder, name.replace('-', "_"))
}

/// Walks a member's src tree for module files (everything except the roots).
fn module_files(dir: &Path, exclude: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let src = dir.join("src");
    let mut stack = vec![src.clone()];
    while let Some(d) = stack.pop() {
        if let Ok(entries) = fs::read_dir(&d) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().map(|e| e == "rs").unwrap_or(false) {
                    if let Ok(rel) = p.strip_prefix(dir) {
                        let rel = rel.to_string_lossy().replace('\\', "/");
                        if !exclude.contains(&rel.as_str()) {
                            out.push(rel);
                        }
                    }
                }
            }
        }
    }
    out.sort();
    out
}

fn edition_str(m: &Member, ws_manifest: &Manifest) -> String {
    use cargo_toml::Edition;
    let edition = m
        .manifest
        .package
        .as_ref()
        .and_then(|p| p.edition.get().ok().copied())
        .or_else(|| {
            ws_manifest
                .workspace
                .as_ref()
                .and_then(|w| w.package.as_ref())
                .and_then(|p| p.edition)
        })
        .unwrap_or(Edition::E2021);
    match edition {
        Edition::E2015 => "2015",
        Edition::E2018 => "2018",
        Edition::E2021 => "2021",
        _ => "2024",
    }
    .to_string()
}

fn fmt_list(items: &[String], indent: &str) -> String {
    let mut s = String::from("[\n");
    for item in items {
        s.push_str(&format!("{}    \"{}\",\n", indent, item));
    }
    s.push_str(&format!("{}]", indent));
    s
}

fn emit_member_build(
    m: &Member,
    ws_manifest: &Manifest,
    labels: &BTreeMap<PathBuf, String>,
    third_party_folder: &str,
) -> Result<String> {
    let mut out = String::from("subinclude(\"///rust//build_defs:rust\")\n");
    let edition = edition_str(m, ws_manifest);
    let own_label = format!(":{}", m.name);

    if m.dir.join("build.rs").exists() {
        eprintln!(
            "import-workspace: warning: {} has a build script; first-party build scripts are unsupported and it was skipped",
            m.name
        );
    }

    let mut deps: Vec<String> = Vec::new();
    for (name, dep) in &m.manifest.dependencies {
        if let Some(label) = dep_label(name, dep, &m.dir, ws_manifest, labels, third_party_folder) {
            deps.push(label);
        }
    }
    deps.sort();
    let mut dev_deps: Vec<String> = Vec::new();
    for (name, dep) in &m.manifest.dev_dependencies {
        if let Some(label) = dep_label(name, dep, &m.dir, ws_manifest, labels, third_party_folder) {
            if !deps.contains(&label) {
                dev_deps.push(label);
            }
        }
    }
    dev_deps.sort();

    // Library product
    let lib_path = m
        .manifest
        .lib
        .as_ref()
        .and_then(|l| l.path.clone())
        .unwrap_or_else(|| "src/lib.rs".to_string());
    let has_lib = m.manifest.lib.is_some() || m.dir.join(&lib_path).exists();

    // Binaries: explicit [[bin]] plus the src/main.rs default
    let mut bins: Vec<(String, String)> = Vec::new();
    for b in &m.manifest.bin {
        let bname = b.name.clone().unwrap_or_else(|| m.name.clone());
        let bpath = b.path.clone().unwrap_or_else(|| "src/main.rs".to_string());
        if m.dir.join(&bpath).exists() {
            bins.push((bname, bpath));
        }
    }
    if bins.is_empty() && m.dir.join("src/main.rs").exists() {
        bins.push((m.name.clone(), "src/main.rs".to_string()));
    }

    let bin_roots: Vec<&str> = bins.iter().map(|(_, p)| p.as_str()).collect();
    let mut excludes: Vec<&str> = vec![lib_path.as_str(), "build.rs"];
    excludes.extend(&bin_roots);
    let modules = module_files(&m.dir, &excludes);

    let crate_type = m
        .manifest
        .lib
        .as_ref()
        .and_then(|l| l.crate_type.first().cloned())
        .map(|t| match t.as_str() {
            "lib" | "rlib" => "rlib".to_string(),
            other => other.to_string(),
        })
        .unwrap_or_else(|| "rlib".to_string());

    if has_lib {
        out.push_str(&format!("\nrust_library(\n    name = \"{}\",\n", m.name));
        out.push_str(&format!("    root = \"{}\",\n", lib_path));
        if crate_type != "rlib" {
            out.push_str(&format!("    crate_type = \"{}\",\n", crate_type));
        }
        if !modules.is_empty() {
            out.push_str(&format!("    modules = {},\n", fmt_list(&modules, "    ")));
        }
        out.push_str(&format!("    edition = \"{}\",\n", edition));
        if !deps.is_empty() {
            out.push_str(&format!("    deps = {},\n", fmt_list(&deps, "    ")));
        }
        out.push_str("    visibility = [\"PUBLIC\"],\n)\n");
    }

    for (bname, bpath) in &bins {
        let rule_name = if has_lib && *bname == m.name {
            format!("{}_bin", bname)
        } else {
            bname.clone()
        };
        let mut bin_deps = deps.clone();
        if has_lib {
            bin_deps.push(own_label.clone());
            bin_deps.sort();
        }
        out.push_str(&format!("\nrust_binary(\n    name = \"{}\",\n", rule_name));
        out.push_str(&format!("    main = \"{}\",\n", bpath));
        out.push_str(&format!("    edition = \"{}\",\n", edition));
        if !bin_deps.is_empty() {
            out.push_str(&format!("    deps = {},\n", fmt_list(&bin_deps, "    ")));
        }
        out.push_str("    visibility = [\"PUBLIC\"],\n)\n");
    }

    // Unit tests: only when the sources actually carry them
    let has_unit_tests = has_lib && {
        let mut found = std::iter::once(lib_path.clone())
            .chain(modules.iter().cloned())
            .any(|f| {
                fs::read_to_string(m.dir.join(&f))
                    .map(|s| s.contains("#[test]") || s.contains("#[cfg(test)]"))
                    .unwrap_or(false)
            });
        if !found && m.dir.join(&lib_path).exists() {
            found = false;
        }
        found
    };
    if has_unit_tests {
        let mut test_deps = deps.clone();
        for d in &dev_deps {
            test_deps.push(d.clone());
        }
        test_deps.sort();
        out.push_str("\nrust_test(\n    name = \"test\",\n");
        out.push_str(&format!("    root = \"{}\",\n", lib_path));
        if !modules.is_empty() {
            out.push_str(&format!("    modules = {},\n", fmt_list(&modules, "    ")));
        }
        out.push_str(&format!("    edition = \"{}\",\n", edition));
        if !test_deps.is_empty() {
            out.push_str(&format!("    deps = {},\n", fmt_list(&test_deps, "    ")));
        }
        out.push_str(")\n");
    }

    // Integration tests: each tests/*.rs is its own crate linking the lib
    let tests_dir = m.dir.join("tests");
    if tests_dir.is_dir() {
        let mut test_files: Vec<String> = fs::read_dir(&tests_dir)
            .map(|entries| {
                entries
                    .flatten()
                    .filter_map(|e| {
                        let p = e.path();
                        (p.extension().map(|x| x == "rs").unwrap_or(false) && p.is_file())
                            .then(|| p.file_name().unwrap().to_string_lossy().to_string())
                    })
                    .collect()
            })
            .unwrap_or_default();
        test_files.sort();
        for f in test_files {
            let stem = f.trim_end_matches(".rs").replace('-', "_");
            let rule_name = if stem == "test" { format!("{}_it", stem) } else { stem.clone() };
            let mut test_deps = deps.clone();
            if has_lib {
                test_deps.push(own_label.clone());
            }
            for d in &dev_deps {
                test_deps.push(d.clone());
            }
            test_deps.sort();
            test_deps.dedup();
            out.push_str(&format!("\nrust_test(\n    name = \"{}\",\n", rule_name));
            out.push_str(&format!("    root = \"tests/{}\",\n", f));
            out.push_str(&format!("    edition = \"{}\",\n", edition));
            if !test_deps.is_empty() {
                out.push_str(&format!("    deps = {},\n", fmt_list(&test_deps, "    ")));
            }
            out.push_str(")\n");
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("please_rust_ws_test_{}_{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(root: &Path, rel: &str, content: &str) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, content).unwrap();
    }

    #[test]
    fn imports_workspace_members() {
        let ws = scratch("members");
        write(
            &ws,
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/core\", \"crates/app\"]\nresolver = \"2\"\n",
        );
        write(
            &ws,
            "crates/core/Cargo.toml",
            "[package]\nname = \"core-lib\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nserde = \"1\"\n",
        );
        write(&ws, "crates/core/src/lib.rs", "pub mod util;\n#[cfg(test)]\nmod t { #[test] fn ok() {} }\n");
        write(&ws, "crates/core/src/util.rs", "pub fn f() {}\n");
        write(
            &ws,
            "crates/app/Cargo.toml",
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\ncore-lib = { path = \"../core\" }\n",
        );
        write(&ws, "crates/app/src/main.rs", "fn main() {}\n");
        write(&ws, "crates/app/tests/smoke.rs", "#[test]\nfn smoke() {}\n");

        let result = import_workspace(&ws, "third_party/rust").unwrap();
        assert_eq!(result.members, 2);
        assert_eq!(result.written, 2);

        let core = fs::read_to_string(ws.join("crates/core/BUILD")).unwrap();
        assert!(core.contains("rust_library("));
        assert!(core.contains("name = \"core-lib\""));
        assert!(core.contains("\"src/util.rs\""));
        assert!(core.contains("\"//third_party/rust:serde\""));
        assert!(core.contains("rust_test(") && core.contains("name = \"test\""));

        let app = fs::read_to_string(ws.join("crates/app/BUILD")).unwrap();
        assert!(app.contains("rust_binary("));
        // path dep resolves to the member's label
        assert!(app.contains(":core-lib\""));
        // integration test emitted, no unit-test rule (no lib)
        assert!(app.contains("name = \"smoke\""));
        assert!(!app.contains("rust_library("));
    }

    #[test]
    fn single_package_and_existing_build_skipped() {
        let ws = scratch("single");
        write(
            &ws,
            "Cargo.toml",
            "[package]\nname = \"solo\"\nversion = \"1.0.0\"\nedition = \"2018\"\n",
        );
        write(&ws, "src/lib.rs", "pub fn f() {}\n");
        write(&ws, "BUILD", "# hands off\n");
        let result = import_workspace(&ws, "third_party/rust").unwrap();
        assert_eq!(result.members, 1);
        assert_eq!(result.written, 0);
        assert_eq!(fs::read_to_string(ws.join("BUILD")).unwrap(), "# hands off\n");
    }

    #[test]
    fn workspace_inherited_deps_and_edition() {
        let ws = scratch("inherit");
        write(
            &ws,
            "Cargo.toml",
            "[workspace]\nmembers = [\"m\"]\n\n[workspace.package]\nedition = \"2021\"\n\n[workspace.dependencies]\nanyhow = \"1\"\n",
        );
        write(
            &ws,
            "m/Cargo.toml",
            "[package]\nname = \"m\"\nversion = \"0.1.0\"\nedition.workspace = true\n\n[dependencies]\nanyhow = { workspace = true }\n",
        );
        write(&ws, "m/src/lib.rs", "pub fn f() {}\n");
        import_workspace(&ws, "third_party/rust").unwrap();
        let build = fs::read_to_string(ws.join("m/BUILD")).unwrap();
        assert!(build.contains("edition = \"2021\""));
        assert!(build.contains("\"//third_party/rust:anyhow\""));
    }
}
