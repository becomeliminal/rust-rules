//! Starlark/BUILD file generation helpers
//!
//! This module provides type-safe structs for generating BUILD files,
//! similar to how go-rules uses bazelbuild/buildtools.

use std::collections::BTreeMap;
use std::fmt::{self, Display, Write};

/// A glob() expression in a BUILD file
#[derive(Debug, Clone)]
pub struct Glob {
    pub patterns: Vec<String>,
    pub exclude: Vec<String>,
    pub allow_empty: bool,
}

impl Glob {
    pub fn new(patterns: Vec<&str>) -> Self {
        Self {
            patterns: patterns.into_iter().map(|s| s.to_string()).collect(),
            exclude: vec![],
            allow_empty: true,
        }
    }

    pub fn exclude(mut self, patterns: Vec<&str>) -> Self {
        self.exclude = patterns.into_iter().map(|s| s.to_string()).collect();
        self
    }
}

impl Display for Glob {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "glob([")?;
        for (i, p) in self.patterns.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "\"{}\"", p)?;
        }
        write!(f, "]")?;

        if !self.exclude.is_empty() {
            write!(f, ", exclude=[")?;
            for (i, p) in self.exclude.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "\"{}\"", p)?;
            }
            write!(f, "]")?;
        }

        if self.allow_empty {
            write!(f, ", allow_empty=True")?;
        }

        write!(f, ")")
    }
}

/// A source entry - either a string or a glob
#[derive(Debug, Clone)]
pub enum SrcEntry {
    File(String),
    Glob(Glob),
    Target(String),
}

impl Display for SrcEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SrcEntry::File(s) => write!(f, "\"{}\"", s),
            SrcEntry::Glob(g) => write!(f, "{}", g),
            SrcEntry::Target(t) => write!(f, "\"{}\"", t),
        }
    }
}

/// A build_rule in a BUILD file
#[derive(Debug, Clone, Default)]
pub struct BuildRule {
    pub name: String,
    pub srcs: BTreeMap<String, Vec<SrcEntry>>,
    pub cmd: Option<BTreeMap<String, String>>,
    pub outs: BTreeMap<String, Vec<String>>,
    pub deps: Vec<String>,
    pub tools: BTreeMap<String, Vec<String>>,
    pub visibility: Vec<String>,
    pub needs_transitive_deps: bool,
    pub binary: bool,
}

impl BuildRule {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            ..Default::default()
        }
    }

    pub fn srcs(mut self, name: &str, entries: Vec<SrcEntry>) -> Self {
        self.srcs.insert(name.to_string(), entries);
        self
    }

    pub fn cmd_single(mut self, cmd: &str) -> Self {
        let mut map = BTreeMap::new();
        map.insert("".to_string(), cmd.to_string());
        self.cmd = Some(map);
        self
    }

    pub fn cmd_configs(mut self, dbg: &str, opt: &str) -> Self {
        let mut map = BTreeMap::new();
        map.insert("dbg".to_string(), dbg.to_string());
        map.insert("opt".to_string(), opt.to_string());
        self.cmd = Some(map);
        self
    }

    pub fn outs(mut self, name: &str, files: Vec<&str>) -> Self {
        self.outs.insert(
            name.to_string(),
            files.into_iter().map(|s| s.to_string()).collect(),
        );
        self
    }

    pub fn outs_single(mut self, file: &str) -> Self {
        self.outs.insert("".to_string(), vec![file.to_string()]);
        self
    }

    pub fn deps(mut self, deps: Vec<&str>) -> Self {
        self.deps = deps.into_iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn add_dep(mut self, dep: &str) -> Self {
        self.deps.push(dep.to_string());
        self
    }

    pub fn tools(mut self, name: &str, targets: Vec<&str>) -> Self {
        self.tools.insert(
            name.to_string(),
            targets.into_iter().map(|s| s.to_string()).collect(),
        );
        self
    }

    pub fn visibility_public(mut self) -> Self {
        self.visibility = vec!["PUBLIC".to_string()];
        self
    }

    pub fn needs_transitive_deps(mut self) -> Self {
        self.needs_transitive_deps = true;
        self
    }
}

impl Display for BuildRule {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "build_rule(")?;
        writeln!(f, "    name = \"{}\",", self.name)?;

        // srcs
        if !self.srcs.is_empty() {
            if self.srcs.len() == 1 && self.srcs.contains_key("") {
                // Single unnamed srcs list
                let entries = &self.srcs[""];
                write!(f, "    srcs = [")?;
                for (i, e) in entries.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", e)?;
                }
                writeln!(f, "],")?;
            } else {
                // Named srcs dict
                writeln!(f, "    srcs = {{")?;
                for (name, entries) in &self.srcs {
                    write!(f, "        \"{}\": [", name)?;
                    for (i, e) in entries.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{}", e)?;
                    }
                    writeln!(f, "],")?;
                }
                writeln!(f, "    }},")?;
            }
        }

        // cmd
        if let Some(cmd) = &self.cmd {
            if cmd.len() == 1 && cmd.contains_key("") {
                writeln!(f, "    cmd = \"{}\",", cmd[""])?;
            } else {
                writeln!(f, "    cmd = {{")?;
                for (config, c) in cmd {
                    writeln!(f, "        \"{}\": \"{}\",", config, c)?;
                }
                writeln!(f, "    }},")?;
            }
        }

        // outs
        if !self.outs.is_empty() {
            if self.outs.len() == 1 && self.outs.contains_key("") {
                let files = &self.outs[""];
                write!(f, "    outs = [")?;
                for (i, file) in files.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "\"{}\"", file)?;
                }
                writeln!(f, "],")?;
            } else {
                writeln!(f, "    outs = {{")?;
                for (name, files) in &self.outs {
                    write!(f, "        \"{}\": [", name)?;
                    for (i, file) in files.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "\"{}\"", file)?;
                    }
                    writeln!(f, "],")?;
                }
                writeln!(f, "    }},")?;
            }
        }

        // deps
        if !self.deps.is_empty() {
            writeln!(f, "    deps = [")?;
            for dep in &self.deps {
                writeln!(f, "        \"{}\",", dep)?;
            }
            writeln!(f, "    ],")?;
        }

        // tools
        if !self.tools.is_empty() {
            writeln!(f, "    tools = {{")?;
            for (name, targets) in &self.tools {
                write!(f, "        \"{}\": [", name)?;
                for (i, t) in targets.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "\"{}\"", t)?;
                }
                writeln!(f, "],")?;
            }
            writeln!(f, "    }},")?;
        }

        // needs_transitive_deps
        if self.needs_transitive_deps {
            writeln!(f, "    needs_transitive_deps = True,")?;
        }

        // visibility
        if !self.visibility.is_empty() {
            write!(f, "    visibility = [")?;
            for (i, v) in self.visibility.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "\"{}\"", v)?;
            }
            writeln!(f, "],")?;
        }

        writeln!(f, ")")?;
        Ok(())
    }
}

/// A BUILD file containing multiple rules
#[derive(Debug, Default)]
pub struct BuildFile {
    pub comments: Vec<String>,
    pub rules: Vec<BuildRule>,
}

impl BuildFile {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn comment(mut self, comment: &str) -> Self {
        self.comments.push(comment.to_string());
        self
    }

    pub fn rule(mut self, rule: BuildRule) -> Self {
        self.rules.push(rule);
        self
    }
}

impl Display for BuildFile {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for comment in &self.comments {
            writeln!(f, "# {}", comment)?;
        }
        if !self.comments.is_empty() && !self.rules.is_empty() {
            writeln!(f)?;
        }
        for (i, rule) in self.rules.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            write!(f, "{}", rule)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glob() {
        let g = Glob::new(vec!["src/**/*.rs"]).exclude(vec!["src/lib.rs"]);
        assert_eq!(
            g.to_string(),
            r#"glob(["src/**/*.rs"], exclude=["src/lib.rs"], allow_empty=True)"#
        );
    }

    #[test]
    fn test_simple_rule() {
        let rule = BuildRule::new("hello")
            .cmd_single("echo hello")
            .outs_single("hello.txt")
            .visibility_public();

        let expected = r#"build_rule(
    name = "hello",
    cmd = "echo hello",
    outs = ["hello.txt"],
    visibility = ["PUBLIC"],
)
"#;
        assert_eq!(rule.to_string(), expected);
    }

    #[test]
    fn test_build_file() {
        let rule = BuildRule::new("test")
            .cmd_single("true")
            .outs_single("test.txt");

        let bf = BuildFile::new()
            .comment("Generated by please_rust")
            .rule(rule);

        assert!(bf.to_string().starts_with("# Generated by please_rust\n"));
    }
}
