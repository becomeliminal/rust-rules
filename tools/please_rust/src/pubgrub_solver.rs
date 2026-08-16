//! PubGrub-backed version selection over the crates.io sparse index.
//!
//! This replaces the greedy max-satisfying walk at `lock`'s select() seam
//! with a real backtracking solver, so a requirement discovered late can
//! force an earlier choice to be revised instead of erroring out. Feature
//! unification is not PubGrub's job: it stays in `resolve`, which runs over
//! the declared set inside the build graph. What this decides is purely
//! *which version of each crate is declared*.
//!
//! Cargo semantics preserved here:
//!   - yanked releases are never candidates (unless already pinned)
//!   - dev-dependencies are ignored; build- and normal-dependencies flow
//!   - platform-gated deps are filtered by target cfg
//!   - optional deps only flow when a default feature activates them
//!   - `package = "..."` renames resolve against the real package name
//!   - multiple majors of the same crate coexist: the solver works per
//!     "major bucket", exactly how cargo allows serde 1.x and serde 0.9 to
//!     both exist in one graph
//!   - MSRV: when a toolchain version is given, releases requiring a newer
//!     rustc are filtered out (cargo >=1.84 behaviour), falling back to
//!     ignoring MSRV if that would leave a package unsolvable

use anyhow::{Context, Result};
use pubgrub::{
    Dependencies, DependencyConstraints, DependencyProvider, PackageResolutionStatistics, Ranges,
    Reporter,
};
use semver::{Version, VersionReq};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// A package as PubGrub sees it: a crate name plus the major-version bucket
/// it lives in. Cargo permits one version per bucket, so the bucket is part
/// of the package identity rather than a constraint on it.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Pkg {
    /// Virtual root holding the requested requirements
    Root,
    /// A concrete crate in one compatibility bucket; its versions are real
    /// releases and at most one is ever chosen, which is what makes two
    /// majors of the same crate able to coexist.
    Crate { name: String, bucket: Bucket },
    /// A requirement that spans more than one bucket (">=0.9, <2"). Its
    /// synthetic versions enumerate the candidate buckets, so *which bucket
    /// to use* becomes a decision PubGrub can backtrack over instead of
    /// something we collapse up front.
    Proxy { name: String, req: String },
}

/// Cargo's compatibility bucket: 1.x, 0.5.x, 0.0.3 are each their own.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Bucket {
    major: u64,
    minor: u64,
    patch: u64,
}

impl Bucket {
    pub fn of(v: &Version) -> Self {
        if v.major > 0 {
            Bucket { major: v.major, minor: 0, patch: 0 }
        } else if v.minor > 0 {
            Bucket { major: 0, minor: v.minor, patch: 0 }
        } else {
            Bucket { major: 0, minor: 0, patch: v.patch }
        }
    }

    fn contains(&self, v: &Version) -> bool {
        Bucket::of(v) == *self
    }
}

impl fmt::Display for Bucket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.major > 0 {
            write!(f, "{}", self.major)
        } else if self.minor > 0 {
            write!(f, "0.{}", self.minor)
        } else {
            write!(f, "0.0.{}", self.patch)
        }
    }
}

impl fmt::Display for Pkg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Pkg::Root => write!(f, "the requested dependencies"),
            Pkg::Crate { name, bucket } => write!(f, "{} {}.x", name, bucket),
            Pkg::Proxy { name, req } => write!(f, "{} matching {}", name, req),
        }
    }
}

/// One release as the solver needs it, independent of the index wire format.
#[derive(Clone, Debug)]
pub struct Release {
    pub version: Version,
    pub cksum: String,
    pub yanked: bool,
    /// Minimum rustc, from the index's rust_version field
    pub rust_version: Option<Version>,
    pub deps: Vec<ReleaseDep>,
}

#[derive(Clone, Debug)]
pub struct ReleaseDep {
    /// The real package name (after a `package = "..."` rename)
    pub package: String,
    pub req: String,
    pub optional: bool,
    /// True when a default feature activates this optional dependency
    pub default_activated: bool,
    /// "normal" | "build" | "dev"
    pub kind: String,
    /// cfg() or triple gate, if any
    pub target: Option<String>,
}

/// Everything the solver needs from the outside world, so it can be driven
/// from the real sparse index or from a fixture in tests.
pub trait ReleaseSource {
    fn releases(&self, name: &str) -> Result<Vec<Release>>;
    /// True when a platform-gated dependency applies to the target triple
    fn target_applies(&self, gate: &str) -> bool;
}

pub struct Solver<'a, S: ReleaseSource> {
    source: &'a S,
    /// Versions already declared, preferred when they satisfy a requirement
    pinned: BTreeMap<String, Vec<Version>>,
    /// Toolchain version for MSRV filtering; None disables the check
    msrv: Option<Version>,
    /// Root requirements: (package, req)
    roots: Vec<(String, VersionReq)>,
    cache: RefCell<BTreeMap<String, Vec<Release>>>,
    /// Packages whose MSRV filter had to be relaxed to stay solvable
    relaxed: RefCell<BTreeSet<String>>,
}

#[derive(Debug)]
pub struct SolverError(String);

impl fmt::Display for SolverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for SolverError {}

impl<'a, S: ReleaseSource> Solver<'a, S> {
    pub fn new(source: &'a S) -> Self {
        Solver {
            source,
            pinned: BTreeMap::new(),
            msrv: None,
            roots: Vec::new(),
            cache: RefCell::new(BTreeMap::new()),
            relaxed: RefCell::new(BTreeSet::new()),
        }
    }

    pub fn pin(&mut self, name: &str, version: Version) -> &mut Self {
        self.pinned.entry(name.to_string()).or_default().push(version);
        self
    }

    pub fn msrv(&mut self, toolchain: Option<Version>) -> &mut Self {
        self.msrv = toolchain;
        self
    }

    pub fn require(&mut self, name: &str, req: VersionReq) -> &mut Self {
        self.roots.push((name.to_string(), req));
        self
    }

    fn releases(&self, name: &str) -> Result<Vec<Release>> {
        if let Some(r) = self.cache.borrow().get(name) {
            return Ok(r.clone());
        }
        let r = self.source.releases(name)?;
        self.cache.borrow_mut().insert(name.to_string(), r.clone());
        Ok(r)
    }

    /// Candidate releases for a package, newest first, after the cargo rules:
    /// skip yanked (unless pinned), then apply MSRV, relaxing it only if that
    /// would leave nothing to choose.
    fn candidates(&self, name: &str, bucket: Bucket) -> Result<Vec<Release>> {
        let pinned = self.pinned.get(name);
        let mut in_bucket: Vec<Release> = self
            .releases(name)?
            .into_iter()
            .filter(|r| bucket.contains(&r.version))
            .filter(|r| {
                !r.yanked
                    || pinned
                        .map(|ps| ps.iter().any(|p| *p == r.version))
                        .unwrap_or(false)
            })
            .collect();
        in_bucket.sort_by(|a, b| b.version.cmp(&a.version));

        if let Some(toolchain) = &self.msrv {
            let ok: Vec<Release> = in_bucket
                .iter()
                .filter(|r| r.rust_version.as_ref().map(|rv| rv <= toolchain).unwrap_or(true))
                .cloned()
                .collect();
            if ok.is_empty() && !in_bucket.is_empty() {
                // Nothing in this bucket supports the toolchain. Cargo errors
                // here; we warn and continue, since refusing to resolve is
                // worse than building something that may need a newer rustc.
                if self.relaxed.borrow_mut().insert(name.to_string()) {
                    eprintln!(
                        "warning: no release of {} {}.x supports rustc {}; ignoring MSRV for it",
                        name, bucket, toolchain
                    );
                }
                return Ok(in_bucket);
            }
            return Ok(ok);
        }
        Ok(in_bucket)
    }

    /// Buckets a requirement can be satisfied by, newest bucket first.
    fn buckets_for(&self, name: &str, req: &VersionReq) -> Result<Vec<Bucket>> {
        let mut buckets: Vec<Bucket> = self
            .releases(name)?
            .iter()
            .filter(|r| !r.yanked && req.matches(&r.version))
            .map(|r| Bucket::of(&r.version))
            .collect();
        buckets.sort();
        buckets.dedup();
        buckets.reverse();
        Ok(buckets)
    }

    /// Translate one dependency edge into PubGrub constraints. A requirement
    /// spanning multiple majors (">=0.9, <2") becomes a choice; PubGrub picks
    /// per bucket, so we constrain the newest satisfiable bucket and let
    /// backtracking revisit if it fails.
    fn edge(&self, package: &str, req_str: &str) -> Result<Option<(Pkg, Ranges<Version>)>> {
        let req = match VersionReq::parse(req_str) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("warning: bad requirement {} on {}: {}", req_str, package, e);
                return Ok(None);
            }
        };
        let buckets = self.buckets_for(package, &req)?;
        if buckets.is_empty() {
            return Ok(None);
        }
        if buckets.len() == 1 {
            let range = self.bucket_range(package, buckets[0], &req)?;
            if range.is_empty() {
                return Ok(None);
            }
            return Ok(Some((
                Pkg::Crate { name: package.to_string(), bucket: buckets[0] },
                range,
            )));
        }
        // Several buckets satisfy the requirement: hand PubGrub the choice.
        let n = buckets.len() as u64;
        Ok(Some((
            Pkg::Proxy { name: package.to_string(), req: req.to_string() },
            Ranges::between(Version::new(0, 0, 0), Version::new(n, 0, 0)),
        )))
    }

    /// The set of concrete versions of `package` inside `bucket` satisfying
    /// `req`, after candidate filtering (yanked, MSRV).
    fn bucket_range(
        &self,
        package: &str,
        bucket: Bucket,
        req: &VersionReq,
    ) -> Result<Ranges<Version>> {
        let mut range = Ranges::empty();
        for r in self.candidates(package, bucket)? {
            if req.matches(&r.version) {
                range = range.union(&Ranges::singleton(r.version));
            }
        }
        Ok(range)
    }

    /// Buckets a proxy enumerates, oldest first, so that a larger synthetic
    /// version means a newer bucket and PubGrub's preference for larger
    /// versions tries the newest major first.
    fn proxy_buckets(&self, name: &str, req_str: &str) -> Result<Vec<Bucket>> {
        let req = VersionReq::parse(req_str)
            .with_context(|| format!("bad requirement {} on {}", req_str, name))?;
        let mut buckets = self.buckets_for(name, &req)?;
        buckets.reverse(); // buckets_for returns newest first
        Ok(buckets)
    }

    /// Run the solve. Returns the chosen version and checksum per crate.
    pub fn solve(&self) -> Result<BTreeMap<String, (Version, String)>> {
        let solution = pubgrub::resolve(self, Pkg::Root, Version::new(0, 0, 0)).map_err(|e| {
            match e {
                pubgrub::PubGrubError::NoSolution(mut derivation) => {
                    derivation.collapse_no_versions();
                    let report =
                        <pubgrub::DefaultStringReporter as Reporter<Pkg, Ranges<Version>, String>>::report(
                            &derivation,
                        )
                        // The virtual root has a synthetic version; saying so
                        // in a user-facing error is just noise.
                        .replace("the requested dependencies at version 0.0.0", "the request")
                        .replace("the requested dependencies 0.0.0", "the request");
                    anyhow::anyhow!("version conflict:\n{}", report)
                }
                pubgrub::PubGrubError::ErrorRetrievingDependencies {
                    package,
                    version,
                    source,
                } => anyhow::anyhow!(
                    "could not resolve dependencies of {}@{}: {}",
                    package,
                    version,
                    source
                ),
                other => anyhow::anyhow!("resolution failed: {}", other),
            }
        })?;

        let mut out: BTreeMap<String, (Version, String)> = BTreeMap::new();
        for (pkg, version) in solution {
            if let Pkg::Crate { name, .. } = pkg {
                let cksum = self
                    .releases(&name)?
                    .into_iter()
                    .find(|r| r.version == version)
                    .map(|r| r.cksum)
                    .unwrap_or_default();
                // One entry per (crate, bucket): keep them all, keyed by the
                // version so callers can name duplicates crate-x.y.z.
                out.insert(format!("{}@{}", name, version), (version.clone(), cksum));
            }
        }
        Ok(out)
    }
}

impl<'a, S: ReleaseSource> DependencyProvider for Solver<'a, S> {
    type P = Pkg;
    type V = Version;
    type VS = Ranges<Version>;
    type M = String;
    type Priority = (u8, std::cmp::Reverse<usize>);
    type Err = SolverError;

    fn prioritize(
        &self,
        package: &Pkg,
        range: &Ranges<Version>,
        _stats: &PackageResolutionStatistics,
    ) -> Self::Priority {
        // Root first, then packages with the fewest candidates: deciding the
        // most constrained package first is what keeps backtracking cheap.
        match package {
            Pkg::Root => (2, std::cmp::Reverse(0)),
            // Proxies are cheap bucket choices; settle them before the
            // concrete versions that hang off them.
            Pkg::Proxy { .. } => (1, std::cmp::Reverse(0)),
            Pkg::Crate { name, bucket } => {
                let n = self
                    .candidates(name, *bucket)
                    .map(|c| c.iter().filter(|r| range.contains(&r.version)).count())
                    .unwrap_or(usize::MAX);
                (0, std::cmp::Reverse(n))
            }
        }
    }

    fn choose_version(
        &self,
        package: &Pkg,
        range: &Ranges<Version>,
    ) -> Result<Option<Version>, Self::Err> {
        match package {
            Pkg::Root => Ok(Some(Version::new(0, 0, 0))),
            Pkg::Proxy { name, req } => {
                let buckets = self
                    .proxy_buckets(name, req)
                    .map_err(|e| SolverError(e.to_string()))?;
                // A bucket that already holds a declared version wins, so
                // adding one crate does not shuffle unrelated majors.
                if let Some(pins) = self.pinned.get(name) {
                    if let Some(i) = buckets.iter().position(|b| {
                        pins.iter().any(|p| b.contains(p))
                    }) {
                        let v = Version::new(i as u64, 0, 0);
                        if range.contains(&v) {
                            return Ok(Some(v));
                        }
                    }
                }
                // Otherwise newest bucket first (highest synthetic version).
                Ok((0..buckets.len())
                    .rev()
                    .map(|i| Version::new(i as u64, 0, 0))
                    .find(|v| range.contains(v)))
            }
            Pkg::Crate { name, bucket } => {
                let candidates = self
                    .candidates(name, *bucket)
                    .map_err(|e| SolverError(e.to_string()))?;
                // An already-declared version wins when it is still in range,
                // so `lock --add` does not churn unrelated crates.
                if let Some(pins) = self.pinned.get(name) {
                    if let Some(p) = pins
                        .iter()
                        .filter(|p| bucket.contains(p) && range.contains(p))
                        .max()
                    {
                        return Ok(Some(p.clone()));
                    }
                }
                Ok(candidates
                    .into_iter()
                    .map(|r| r.version)
                    .find(|v| range.contains(v)))
            }
        }
    }

    fn get_dependencies(
        &self,
        package: &Pkg,
        version: &Version,
    ) -> Result<Dependencies<Pkg, Ranges<Version>, String>, Self::Err> {
        let mut out: BTreeMap<Pkg, Ranges<Version>> = BTreeMap::new();

        // A proxy resolves to exactly one concrete bucket of one crate.
        if let Pkg::Proxy { name, req } = package {
            let buckets = self
                .proxy_buckets(name, req)
                .map_err(|e| SolverError(e.to_string()))?;
            let idx = version.major as usize;
            let bucket = match buckets.get(idx) {
                Some(b) => *b,
                None => {
                    return Ok(Dependencies::Unavailable(format!(
                        "no bucket {} for {}",
                        idx, name
                    )))
                }
            };
            let parsed = match VersionReq::parse(req) {
                Ok(r) => r,
                Err(e) => return Err(SolverError(e.to_string())),
            };
            let range = self
                .bucket_range(name, bucket, &parsed)
                .map_err(|e| SolverError(e.to_string()))?;
            if range.is_empty() {
                return Ok(Dependencies::Unavailable(format!(
                    "no release of {} {}.x satisfies {}",
                    name, bucket, req
                )));
            }
            return Ok(Dependencies::Available(
                [(Pkg::Crate { name: name.clone(), bucket }, range)]
                    .into_iter()
                    .collect::<DependencyConstraints<_, _>>(),
            ));
        }

        let edges: Vec<(String, String)> = match package {
            Pkg::Root => self
                .roots
                .iter()
                .map(|(name, req)| (name.clone(), req.to_string()))
                .collect(),
            Pkg::Proxy { .. } => unreachable!("handled above"),
            Pkg::Crate { name, .. } => {
                let releases = self.releases(name).map_err(|e| SolverError(e.to_string()))?;
                let release = match releases.iter().find(|r| r.version == *version) {
                    Some(r) => r.clone(),
                    None => {
                        return Ok(Dependencies::Unavailable(format!(
                            "{}@{} is not in the index",
                            name, version
                        )))
                    }
                };
                release
                    .deps
                    .iter()
                    .filter(|d| d.kind != "dev")
                    .filter(|d| !d.optional || d.default_activated)
                    .filter(|d| {
                        d.target
                            .as_ref()
                            .map(|t| self.source.target_applies(t))
                            .unwrap_or(true)
                    })
                    .filter(|d| !d.package.starts_with("rustc-std-workspace"))
                    .map(|d| (d.package.clone(), d.req.clone()))
                    .collect()
            }
        };

        for (dep_name, req) in edges {
            match self.edge(&dep_name, &req) {
                Ok(Some((pkg, range))) => {
                    // Two edges onto the same bucket intersect, as cargo does
                    out.entry(pkg)
                        .and_modify(|r| *r = r.intersection(&range))
                        .or_insert(range);
                }
                Ok(None) => {
                    return Ok(Dependencies::Unavailable(format!(
                        "no release of {} satisfies {}",
                        dep_name, req
                    )))
                }
                Err(e) => return Err(SolverError(e.to_string())),
            }
        }
        Ok(Dependencies::Available(
            out.into_iter().collect::<DependencyConstraints<_, _>>(),
        ))
    }
}

/// Reads the toolchain version out of a rust_toolchain declaration, so MSRV
/// filtering matches whatever the repo actually builds with.
pub fn toolchain_version(build_text: &str) -> Option<Version> {
    let mut in_toolchain = false;
    for line in build_text.lines() {
        let t = line.trim();
        if t.starts_with("rust_toolchain(") {
            in_toolchain = true;
        } else if in_toolchain {
            if let Some(rest) = t.strip_prefix("version") {
                let v = rest
                    .trim_start_matches([' ', '=', '"'])
                    .trim_end_matches(['"', ',', ' ']);
                return Version::parse(v).ok();
            }
            if t == ")" {
                in_toolchain = false;
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hand-built index: crate -> releases.
    struct Fixture {
        crates: BTreeMap<String, Vec<Release>>,
        /// gates that count as applying to the target
        applies: BTreeSet<String>,
    }

    impl ReleaseSource for Fixture {
        fn releases(&self, name: &str) -> Result<Vec<Release>> {
            self.crates
                .get(name)
                .cloned()
                .with_context(|| format!("{} is not in the index", name))
        }
        fn target_applies(&self, gate: &str) -> bool {
            self.applies.contains(gate)
        }
    }

    struct Builder {
        crates: BTreeMap<String, Vec<Release>>,
        applies: BTreeSet<String>,
    }

    fn idx() -> Builder {
        Builder { crates: BTreeMap::new(), applies: BTreeSet::new() }
    }

    impl Builder {
        /// deps: (package, req)
        fn add(&mut self, name: &str, version: &str, deps: &[(&str, &str)]) -> &mut Self {
            self.push(name, version, deps, |r| r)
        }

        fn push(
            &mut self,
            name: &str,
            version: &str,
            deps: &[(&str, &str)],
            f: impl Fn(Release) -> Release,
        ) -> &mut Self {
            let release = Release {
                version: Version::parse(version).unwrap(),
                cksum: format!("cksum-{}-{}", name, version),
                yanked: false,
                rust_version: None,
                deps: deps
                    .iter()
                    .map(|(p, r)| ReleaseDep {
                        package: p.to_string(),
                        req: r.to_string(),
                        optional: false,
                        default_activated: false,
                        kind: "normal".to_string(),
                        target: None,
                    })
                    .collect(),
            };
            self.crates.entry(name.to_string()).or_default().push(f(release));
            self
        }

        fn yanked(&mut self, name: &str, version: &str) -> &mut Self {
            self.push(name, version, &[], |mut r| {
                r.yanked = true;
                r
            })
        }

        fn msrv_release(&mut self, name: &str, version: &str, rust_version: &str) -> &mut Self {
            self.push(name, version, &[], |mut r| {
                r.rust_version = Some(Version::parse(rust_version).unwrap());
                r
            })
        }

        fn dep_release(&mut self, name: &str, version: &str, dep: ReleaseDep) -> &mut Self {
            self.push(name, version, &[], |mut r| {
                r.deps = vec![dep.clone()];
                r
            })
        }

        fn applies(&mut self, gate: &str) -> &mut Self {
            self.applies.insert(gate.to_string());
            self
        }

        fn build(&self) -> Fixture {
            Fixture { crates: self.crates.clone(), applies: self.applies.clone() }
        }
    }

    fn dep(package: &str, req: &str) -> ReleaseDep {
        ReleaseDep {
            package: package.to_string(),
            req: req.to_string(),
            optional: false,
            default_activated: false,
            kind: "normal".to_string(),
            target: None,
        }
    }

    fn solve(fixture: &Fixture, roots: &[(&str, &str)]) -> Result<BTreeMap<String, Version>> {
        solve_with(fixture, roots, &[], None)
    }

    fn solve_with(
        fixture: &Fixture,
        roots: &[(&str, &str)],
        pins: &[(&str, &str)],
        msrv: Option<&str>,
    ) -> Result<BTreeMap<String, Version>> {
        let mut solver = Solver::new(fixture);
        for (name, req) in roots {
            solver.require(name, VersionReq::parse(req).unwrap());
        }
        for (name, v) in pins {
            solver.pin(name, Version::parse(v).unwrap());
        }
        solver.msrv(msrv.map(|m| Version::parse(m).unwrap()));
        let solution = solver.solve()?;
        Ok(solution
            .into_iter()
            .map(|(k, (v, _))| (k.split('@').next().unwrap().to_string(), v))
            .collect())
    }

    #[test]
    fn picks_newest_satisfying_version() {
        let f = idx().add("a", "1.0.0", &[]).add("a", "1.2.0", &[]).add("a", "2.0.0", &[]).build();
        let got = solve(&f, &[("a", "^1")]).unwrap();
        assert_eq!(got["a"], Version::parse("1.2.0").unwrap());
    }

    #[test]
    fn resolves_transitive_dependencies() {
        let f = idx()
            .add("app", "1.0.0", &[("lib", "^1")])
            .add("lib", "1.5.0", &[("core", "^0.2")])
            .add("core", "0.2.3", &[])
            .build();
        let got = solve(&f, &[("app", "^1")]).unwrap();
        assert_eq!(got["lib"], Version::parse("1.5.0").unwrap());
        assert_eq!(got["core"], Version::parse("0.2.3").unwrap());
    }

    #[test]
    fn backtracks_when_the_newest_choice_conflicts() {
        // Greedy selection takes a 1.1.0 first, then finds its requirement on
        // shared ^2 unsatisfiable. The solver must back up to a 1.0.0 rather
        // than giving up: both live in the same bucket, so no coexistence
        // escape hatch exists.
        let f = idx()
            .add("a", "1.0.0", &[("shared", "^1")])
            .add("a", "1.1.0", &[("shared", "^2")])
            .add("shared", "1.0.0", &[])
            .build();
        let got = solve(&f, &[("a", "^1")]).unwrap();
        assert_eq!(got["a"], Version::parse("1.0.0").unwrap());
        assert_eq!(got["shared"], Version::parse("1.0.0").unwrap());
    }

    #[test]
    fn backtracking_prefers_the_newest_workable_version() {
        // Only the newest release is broken; the solver should settle on the
        // next one down, not walk all the way back.
        let f = idx()
            .add("a", "1.0.0", &[("shared", "^1")])
            .add("a", "1.1.0", &[("shared", "^1")])
            .add("a", "1.2.0", &[("shared", "^9")])
            .add("shared", "1.0.0", &[])
            .build();
        let got = solve(&f, &[("a", "^1")]).unwrap();
        assert_eq!(got["a"], Version::parse("1.1.0").unwrap());
    }

    #[test]
    fn incompatible_majors_coexist_rather_than_conflicting() {
        // Cargo semantics: a root wanting >=1 and a dependency wanting ^1
        // yields *both* b 2.0.0 and b 1.0.0, because different majors are
        // different packages. This is not a conflict.
        let f = idx()
            .add("b", "1.0.0", &[])
            .add("b", "2.0.0", &[])
            .add("c", "1.0.0", &[("b", "^1")])
            .build();
        let mut solver = Solver::new(&f);
        solver.require("b", VersionReq::parse(">=1").unwrap());
        solver.require("c", VersionReq::parse("^1").unwrap());
        let solution = solver.solve().unwrap();
        let mut bs: Vec<String> = solution
            .keys()
            .filter(|k| k.starts_with("b@"))
            .cloned()
            .collect();
        bs.sort();
        assert_eq!(bs, vec!["b@1.0.0".to_string(), "b@2.0.0".to_string()]);
    }

    #[test]
    fn backtracks_through_a_transitive_chain() {
        // top 2.0.0 needs mid ^2, and mid 2.0.0 needs bottom ^2 which does
        // not exist; the solver must fall back to the 1.x line throughout.
        let f = idx()
            .add("top", "1.0.0", &[("mid", "^1")])
            .add("top", "2.0.0", &[("mid", "^2")])
            .add("mid", "1.0.0", &[("bottom", "^1")])
            .add("mid", "2.0.0", &[("bottom", "^2")])
            .add("bottom", "1.0.0", &[])
            .build();
        let got = solve(&f, &[("top", ">=1")]).unwrap();
        assert_eq!(got["top"], Version::parse("1.0.0").unwrap());
        assert_eq!(got["mid"], Version::parse("1.0.0").unwrap());
        assert_eq!(got["bottom"], Version::parse("1.0.0").unwrap());
    }

    #[test]
    fn two_majors_of_one_crate_coexist() {
        // cargo allows this: different major buckets are different packages
        let f = idx()
            .add("dup", "1.0.0", &[])
            .add("dup", "2.0.0", &[])
            .add("old", "1.0.0", &[("dup", "^1")])
            .add("new", "1.0.0", &[("dup", "^2")])
            .build();
        let solution = {
            let mut solver = Solver::new(&f);
            solver.require("old", VersionReq::parse("^1").unwrap());
            solver.require("new", VersionReq::parse("^1").unwrap());
            solver.solve().unwrap()
        };
        let dups: Vec<&String> = solution.keys().filter(|k| k.starts_with("dup@")).collect();
        assert_eq!(dups.len(), 2, "expected both majors, got {:?}", dups);
    }

    #[test]
    fn zero_versions_bucket_by_minor() {
        // 0.1 and 0.2 are incompatible under cargo's rules
        assert_ne!(
            Bucket::of(&Version::parse("0.1.0").unwrap()),
            Bucket::of(&Version::parse("0.2.0").unwrap())
        );
        assert_eq!(
            Bucket::of(&Version::parse("0.1.0").unwrap()),
            Bucket::of(&Version::parse("0.1.9").unwrap())
        );
        // 0.0.x are each their own bucket
        assert_ne!(
            Bucket::of(&Version::parse("0.0.1").unwrap()),
            Bucket::of(&Version::parse("0.0.2").unwrap())
        );
    }

    #[test]
    fn yanked_releases_are_skipped() {
        let mut b = idx();
        b.add("y", "1.0.0", &[]);
        b.yanked("y", "1.1.0");
        let f = b.build();
        let got = solve(&f, &[("y", "^1")]).unwrap();
        assert_eq!(got["y"], Version::parse("1.0.0").unwrap());
    }

    #[test]
    fn yanked_but_already_pinned_is_kept() {
        // Cargo keeps a yanked version that is already in your lockfile
        let mut b = idx();
        b.add("y", "1.0.0", &[]);
        b.yanked("y", "1.1.0");
        let f = b.build();
        let got = solve_with(&f, &[("y", "^1")], &[("y", "1.1.0")], None).unwrap();
        assert_eq!(got["y"], Version::parse("1.1.0").unwrap());
    }

    #[test]
    fn pinned_version_is_preferred_over_newer() {
        let f = idx().add("p", "1.0.0", &[]).add("p", "1.9.0", &[]).build();
        let got = solve_with(&f, &[("p", "^1")], &[("p", "1.0.0")], None).unwrap();
        assert_eq!(got["p"], Version::parse("1.0.0").unwrap());
    }

    #[test]
    fn pinned_version_is_abandoned_when_it_cannot_satisfy() {
        let f = idx().add("p", "1.0.0", &[]).add("p", "2.0.0", &[]).build();
        let got = solve_with(&f, &[("p", "^2")], &[("p", "1.0.0")], None).unwrap();
        assert_eq!(got["p"], Version::parse("2.0.0").unwrap());
    }

    #[test]
    fn msrv_filters_out_releases_needing_a_newer_rustc() {
        let mut b = idx();
        b.add("m", "1.0.0", &[]);
        b.msrv_release("m", "1.5.0", "1.80.0");
        b.msrv_release("m", "2.0.0", "1.99.0");
        let f = b.build();
        // Toolchain 1.85 can take 1.5.0 but not the 1.99-requiring release
        let got = solve_with(&f, &[("m", "^1")], &[], Some("1.85.0")).unwrap();
        assert_eq!(got["m"], Version::parse("1.5.0").unwrap());
    }

    #[test]
    fn msrv_relaxes_when_no_release_supports_the_toolchain() {
        // Refusing to resolve would be worse than warning and proceeding
        let mut b = idx();
        b.msrv_release("hard", "1.0.0", "1.99.0");
        let f = b.build();
        let got = solve_with(&f, &[("hard", "^1")], &[], Some("1.60.0")).unwrap();
        assert_eq!(got["hard"], Version::parse("1.0.0").unwrap());
    }

    #[test]
    fn msrv_off_by_default_takes_the_newest() {
        let mut b = idx();
        b.add("m", "1.0.0", &[]);
        b.msrv_release("m", "2.0.0", "1.99.0");
        let f = b.build();
        let got = solve(&f, &[("m", ">=1")]).unwrap();
        assert_eq!(got["m"], Version::parse("2.0.0").unwrap());
    }

    #[test]
    fn dev_dependencies_are_ignored() {
        let mut b = idx();
        b.dep_release("d", "1.0.0", ReleaseDep { kind: "dev".to_string(), ..dep("missing", "^1") });
        let f = b.build();
        // "missing" is not in the index at all; ignoring dev deps keeps this solvable
        let got = solve(&f, &[("d", "^1")]).unwrap();
        assert_eq!(got["d"], Version::parse("1.0.0").unwrap());
    }

    #[test]
    fn build_dependencies_are_resolved() {
        let mut b = idx();
        b.dep_release("bd", "1.0.0", ReleaseDep { kind: "build".to_string(), ..dep("helper", "^1") });
        b.add("helper", "1.2.0", &[]);
        let f = b.build();
        let got = solve(&f, &[("bd", "^1")]).unwrap();
        assert_eq!(got["helper"], Version::parse("1.2.0").unwrap());
    }

    #[test]
    fn inactive_optional_dependencies_are_skipped() {
        let mut b = idx();
        b.dep_release(
            "o",
            "1.0.0",
            ReleaseDep { optional: true, default_activated: false, ..dep("extra", "^1") },
        );
        let f = b.build();
        let got = solve(&f, &[("o", "^1")]).unwrap();
        assert!(!got.contains_key("extra"));
    }

    #[test]
    fn default_activated_optional_dependencies_flow() {
        let mut b = idx();
        b.dep_release(
            "o",
            "1.0.0",
            ReleaseDep { optional: true, default_activated: true, ..dep("extra", "^1") },
        );
        b.add("extra", "1.1.0", &[]);
        let f = b.build();
        let got = solve(&f, &[("o", "^1")]).unwrap();
        assert_eq!(got["extra"], Version::parse("1.1.0").unwrap());
    }

    #[test]
    fn platform_gated_dependencies_respect_the_target() {
        let mut b = idx();
        b.dep_release(
            "p",
            "1.0.0",
            ReleaseDep { target: Some("cfg(windows)".to_string()), ..dep("winapi", "^1") },
        );
        b.add("winapi", "1.0.0", &[]);
        let f = b.build(); // no gates apply
        let got = solve(&f, &[("p", "^1")]).unwrap();
        assert!(!got.contains_key("winapi"));

        let mut b2 = idx();
        b2.dep_release(
            "p",
            "1.0.0",
            ReleaseDep { target: Some("cfg(unix)".to_string()), ..dep("nix", "^1") },
        );
        b2.add("nix", "1.0.0", &[]);
        b2.applies("cfg(unix)");
        let f2 = b2.build();
        let got2 = solve(&f2, &[("p", "^1")]).unwrap();
        assert_eq!(got2["nix"], Version::parse("1.0.0").unwrap());
    }

    #[test]
    fn renamed_dependencies_resolve_against_the_real_package() {
        let mut b = idx();
        // declared as `errno` but the real package is `libc-errno`
        b.dep_release("r", "1.0.0", dep("libc-errno", "^1"));
        b.add("libc-errno", "1.3.0", &[]);
        let f = b.build();
        let got = solve(&f, &[("r", "^1")]).unwrap();
        assert_eq!(got["libc-errno"], Version::parse("1.3.0").unwrap());
    }

    #[test]
    fn diamond_dependencies_unify_on_one_version() {
        let f = idx()
            .add("left", "1.0.0", &[("shared", "^1.2")])
            .add("right", "1.0.0", &[("shared", ">=1.0, <1.9")])
            .add("shared", "1.1.0", &[])
            .add("shared", "1.5.0", &[])
            .add("shared", "1.9.0", &[])
            .build();
        let got = solve(&f, &[("left", "^1"), ("right", "^1")]).unwrap();
        assert_eq!(got["shared"], Version::parse("1.5.0").unwrap());
    }

    #[test]
    fn unsatisfiable_requirements_report_a_conflict() {
        let f = idx()
            .add("x", "1.0.0", &[("shared", "^1")])
            .add("y", "1.0.0", &[("shared", "^2")])
            .add("shared", "1.0.0", &[])
            .build();
        // y needs shared ^2, which does not exist
        let err = solve(&f, &[("x", "^1"), ("y", "^1")]).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("shared") || msg.contains("conflict"),
            "unhelpful error: {}",
            msg
        );
    }

    #[test]
    fn missing_crate_is_reported_by_name() {
        let f = idx().add("a", "1.0.0", &[("ghost", "^1")]).build();
        let err = solve(&f, &[("a", "^1")]).unwrap_err();
        assert!(format!("{:#}", err).contains("ghost"), "got: {:#}", err);
    }

    #[test]
    fn prerelease_versions_are_not_chosen_for_plain_requirements() {
        // semver's matches() already excludes prereleases from ^1
        let f = idx().add("pre", "1.0.0", &[]).add("pre", "2.0.0-alpha.1", &[]).build();
        let got = solve(&f, &[("pre", "^1")]).unwrap();
        assert_eq!(got["pre"], Version::parse("1.0.0").unwrap());
    }

    #[test]
    fn toolchain_version_is_read_from_the_build_file() {
        let build = "subinclude(\"//build_defs:rust\")\n\nrust_toolchain(\n    name = \"toolchain\",\n    hashes = [\"abc\"],\n    version = \"1.97.1\",\n    visibility = [\"PUBLIC\"],\n)\n";
        assert_eq!(toolchain_version(build), Some(Version::parse("1.97.1").unwrap()));
        assert_eq!(toolchain_version("nothing here"), None);
    }
}
