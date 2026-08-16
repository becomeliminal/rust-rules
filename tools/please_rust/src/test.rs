//! Test wrapper: runs a test command (a libtest binary, or rustdoc --test),
//! parses the standard libtest output into per-test results, and writes
//! JUnit XML to $RESULTS_FILE so plz reports individual tests instead of a
//! blind pass/fail (the please_go test analog).

use anyhow::{Context, Result};
use clap::Args;
use std::io::Read;
use std::process::{Command, Stdio};

#[derive(Args)]
pub struct TestArgs {
    /// Test suite name for reporting
    #[arg(long, default_value = "test")]
    pub suite: String,

    /// Resolve *.externconfig files under the cwd and append --extern flags
    /// to the command (for rustdoc --test with dependencies)
    #[arg(long)]
    pub externs_from_cwd: bool,

    /// The command to run (test binary and its args)
    #[arg(trailing_var_arg = true, required = true)]
    pub command: Vec<String>,
}

struct Case {
    name: String,
    outcome: Outcome,
    details: String,
}

#[derive(PartialEq)]
enum Outcome {
    Pass,
    Fail,
    Skip,
}

pub fn run(args: TestArgs) -> Result<()> {
    let mut cmd = Command::new(&args.command[0]);
    cmd.args(&args.command[1..]);
    if args.externs_from_cwd {
        for (name, path) in collect_externs(".") {
            cmd.arg("--extern").arg(format!("{}={}", name, path));
            if let Some(dir) = std::path::Path::new(&path).parent() {
                cmd.arg("-L").arg(dir);
            }
        }
    }
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::inherit());

    let mut child = cmd
        .spawn()
        .with_context(|| format!("Failed to run {}", args.command[0]))?;
    let mut output = String::new();
    child
        .stdout
        .take()
        .context("no stdout")?
        .read_to_string(&mut output)?;
    let status = child.wait()?;

    // Echo through so failures stay debuggable in the log
    print!("{}", output);

    let cases = parse_libtest(&output);
    if let Ok(results_file) = std::env::var("RESULTS_FILE") {
        std::fs::write(&results_file, junit_xml(&args.suite, &cases))
            .with_context(|| format!("Failed to write {}", results_file))?;
    }

    if !status.success() {
        anyhow::bail!("tests failed");
    }
    Ok(())
}

/// Parses `test <name> ... ok|FAILED|ignored` lines plus the
/// `---- <name> stdout ----` failure detail sections.
fn parse_libtest(output: &str) -> Vec<Case> {
    let mut cases: Vec<Case> = Vec::new();
    let mut details_for: Option<usize> = None;
    let mut in_failures = false;

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed == "failures:" {
            in_failures = true;
            continue;
        }
        if in_failures {
            if let Some(rest) = trimmed.strip_prefix("---- ") {
                if let Some(name) = rest.strip_suffix(" stdout ----") {
                    details_for = cases.iter().position(|c| c.name == name);
                    continue;
                }
            }
            if trimmed.starts_with("test result:") {
                in_failures = false;
                details_for = None;
                continue;
            }
            if let Some(i) = details_for {
                cases[i].details.push_str(line);
                cases[i].details.push('\n');
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("test ") {
            if let Some((name, outcome)) = rest.rsplit_once(" ... ") {
                let outcome = match outcome.trim() {
                    "ok" => Outcome::Pass,
                    "ignored" | "skipped" => Outcome::Skip,
                    o if o.starts_with("ignored") => Outcome::Skip,
                    _ => Outcome::Fail,
                };
                cases.push(Case {
                    name: name.trim().to_string(),
                    outcome,
                    details: String::new(),
                });
            }
        }
    }
    cases
}

fn junit_xml(suite: &str, cases: &[Case]) -> String {
    let failures = cases.iter().filter(|c| c.outcome == Outcome::Fail).count();
    let skipped = cases.iter().filter(|c| c.outcome == Outcome::Skip).count();
    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str(&format!(
        "<testsuite name=\"{}\" tests=\"{}\" failures=\"{}\" skipped=\"{}\">\n",
        escape(suite),
        cases.len(),
        failures,
        skipped
    ));
    for c in cases {
        match c.outcome {
            Outcome::Pass => {
                xml.push_str(&format!("  <testcase name=\"{}\"/>\n", escape(&c.name)));
            }
            Outcome::Skip => {
                xml.push_str(&format!(
                    "  <testcase name=\"{}\"><skipped/></testcase>\n",
                    escape(&c.name)
                ));
            }
            Outcome::Fail => {
                xml.push_str(&format!(
                    "  <testcase name=\"{}\"><failure>{}</failure></testcase>\n",
                    escape(&c.name),
                    escape(&c.details)
                ));
            }
        }
    }
    xml.push_str("</testsuite>\n");
    xml
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Collect crate name -> library path pairs from externconfig files.
fn collect_externs(root: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut stack = vec![std::path::PathBuf::from(root)];
    let mut configs = Vec::new();
    while let Some(dir) = stack.pop() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().map(|e| e == "externconfig").unwrap_or(false) {
                    configs.push(p);
                }
            }
        }
    }
    for config in configs {
        if let Ok(content) = std::fs::read_to_string(&config) {
            for line in content.lines() {
                if let Some((name, filename)) = line.trim().split_once('=') {
                    // The library sits next to its externconfig
                    if let Some(dir) = config.parent() {
                        let lib = dir.join(filename.trim());
                        if lib.exists() {
                            out.push((name.trim().to_string(), lib.display().to_string()));
                        }
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUTPUT: &str = "\nrunning 4 tests\ntest add::works ... ok\ntest sub::works ... FAILED\ntest slow_one ... ignored\ntest math::doc (line 4) ... ok\n\nfailures:\n\n---- sub::works stdout ----\nassertion failed: 1 == 2\nnote: extra context\n\n\nfailures:\n    sub::works\n\ntest result: FAILED. 2 passed; 1 failed; 1 ignored\n";

    #[test]
    fn parses_all_outcomes() {
        let cases = parse_libtest(OUTPUT);
        assert_eq!(cases.len(), 4);
        assert!(cases[0].outcome == Outcome::Pass && cases[0].name == "add::works");
        assert!(cases[1].outcome == Outcome::Fail);
        assert!(cases[2].outcome == Outcome::Skip);
        assert_eq!(cases[3].name, "math::doc (line 4)");
        assert!(cases[1].details.contains("assertion failed: 1 == 2"));
        assert!(cases[1].details.contains("extra context"));
    }

    #[test]
    fn junit_reflects_results() {
        let cases = parse_libtest(OUTPUT);
        let xml = junit_xml("suite<1>", &cases);
        assert!(xml.contains("tests=\"4\" failures=\"1\" skipped=\"1\""));
        assert!(xml.contains("name=\"suite&lt;1&gt;\"")); // escaped
        assert!(xml.contains("<testcase name=\"add::works\"/>"));
        assert!(xml.contains("<failure>"));
        assert!(xml.contains("<skipped/>"));
    }

    #[test]
    fn escape_covers_specials() {
        assert_eq!(escape("a<b>&\"c\""), "a&lt;b&gt;&amp;&quot;c&quot;");
    }

    #[test]
    fn collect_externs_finds_libs() {
        let dir = std::env::temp_dir().join(format!("please_rust_externs_test_{}", std::process::id()));
        let sub = dir.join("nested");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("libx-1_0_0.rlib"), "").unwrap();
        std::fs::write(sub.join("x.externconfig"), "x=libx-1_0_0.rlib\n").unwrap();
        std::fs::write(sub.join("missing.externconfig"), "y=libgone.rlib\n").unwrap();
        let externs = collect_externs(dir.to_str().unwrap());
        assert_eq!(externs.len(), 1);
        assert_eq!(externs[0].0, "x");
        assert!(externs[0].1.ends_with("libx-1_0_0.rlib"));
    }
}

#[cfg(test)]
mod run_wrapper_tests {
    use super::*;

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn wrapper_runs_command_and_writes_results() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("please_rust_wrap_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let results = dir.join("test.results");
        std::env::set_var("RESULTS_FILE", &results);
        run(TestArgs {
            suite: "wrapped".to_string(),
            externs_from_cwd: false,
            command: vec![
                "sh".to_string(),
                "-c".to_string(),
                "printf 'running 1 test\\ntest it_works ... ok\\n\\ntest result: ok. 1 passed\\n'".to_string(),
            ],
        })
        .unwrap();
        std::env::remove_var("RESULTS_FILE");
        let xml = std::fs::read_to_string(&results).unwrap();
        assert!(xml.contains("tests=\"1\" failures=\"0\""));
        assert!(xml.contains("it_works"));
    }

    #[test]
    fn wrapper_propagates_failure() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("RESULTS_FILE");
        let err = run(TestArgs {
            suite: "failing".to_string(),
            externs_from_cwd: false,
            command: vec!["sh".to_string(), "-c".to_string(), "exit 3".to_string()],
        })
        .unwrap_err();
        assert!(err.to_string().contains("tests failed"));
    }
}
