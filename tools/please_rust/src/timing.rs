//! Timing, call counts and peak memory - off unless asked for.
//!
//! The benchmark harness times whole `plz` invocations and cannot see inside
//! a subcommand, so "where does the time go" has only ever been answered by
//! reading code. This answers it with numbers.
//!
//! Everything here short-circuits on one env var read, cached once, so an
//! uninstrumented run pays an atomic load per call site and nothing else.
//! Set `PLEASE_RUST_TIMINGS=1` to turn it on.
//!
//! Call counts matter as much as durations here. A function that takes 40µs
//! is uninteresting until you learn it runs once per dependency per compile,
//! and the shape of this tool - fixpoint loops, per-crate subprocesses, a
//! solver that is asked to prioritise the same package repeatedly - makes
//! "how many times" the more diagnostic question.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("PLEASE_RUST_TIMINGS").is_some())
}

fn totals() -> &'static Mutex<BTreeMap<&'static str, (u64, f64)>> {
    static T: OnceLock<Mutex<BTreeMap<&'static str, (u64, f64)>>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn started() -> Instant {
    static S: OnceLock<Instant> = OnceLock::new();
    *S.get_or_init(Instant::now)
}

/// A named span, accumulated by name and reported at exit.
///
/// Reports on drop rather than at an explicit end, so an early return or a
/// `?` still counts the work that happened before it.
pub struct Phase {
    name: &'static str,
    start: Instant,
}

/// Start a span. Bind it (`let _p = phase("x");`) to cover a scope.
pub fn phase(name: &'static str) -> Phase {
    started(); // anchor the process clock at the first span
    Phase { name, start: Instant::now() }
}

impl Drop for Phase {
    fn drop(&mut self) {
        if !enabled() {
            return;
        }
        let ms = self.start.elapsed().as_secs_f64() * 1000.0;
        if let Ok(mut t) = totals().lock() {
            let e = t.entry(self.name).or_insert((0, 0.0));
            e.0 += 1;
            e.1 += ms;
        }
    }
}

/// Count an event without timing it - for hot paths where a lock per call
/// would distort the thing being measured.
pub fn count(name: &'static str) {
    if !enabled() {
        return;
    }
    if let Ok(mut t) = totals().lock() {
        t.entry(name).or_insert((0, 0.0)).0 += 1;
    }
}

/// Peak resident set size in KB.
///
/// Linux only: VmHWM is the kernel's own high-water mark, which is the
/// number worth having - sampling RSS from outside would miss a peak between
/// samples. macOS would need getrusage through FFI and this tool carries no
/// libc dependency, so it reports nothing there rather than something wrong.
fn peak_rss_kb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmHWM:") {
            return rest.trim().split_whitespace().next()?.parse().ok();
        }
    }
    None
}

/// Print what was collected. Called once, at the end of a run.
pub fn report(command: &str) {
    if !enabled() {
        return;
    }
    let wall = started().elapsed().as_secs_f64() * 1000.0;
    eprintln!("please_rust timing: {} total {:.1}ms", command, wall);

    if let Ok(t) = totals().lock() {
        // Slowest first: the question is always which phase to look at next.
        let mut rows: Vec<_> = t.iter().collect();
        rows.sort_by(|a, b| b.1 .1.partial_cmp(&a.1 .1).unwrap_or(std::cmp::Ordering::Equal));
        for (name, (calls, ms)) in rows {
            if *ms > 0.0 {
                eprintln!(
                    "please_rust timing:   {:<28} {:>9.1}ms  {:>7} calls  {:>8.3}ms/call",
                    name,
                    ms,
                    calls,
                    ms / *calls as f64
                );
            } else {
                eprintln!("please_rust timing:   {:<28} {:>28} calls", name, calls);
            }
        }
    }

    match peak_rss_kb() {
        Some(kb) => eprintln!("please_rust timing: peak RSS {:.1}MB", kb as f64 / 1024.0),
        None => eprintln!("please_rust timing: peak RSS unavailable on this platform"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Disabled is the normal case and must record nothing at all - an
    /// instrumented build that quietly accumulated state would be a
    /// regression in the thing it exists to measure.
    #[test]
    fn disabled_collects_nothing() {
        std::env::remove_var("PLEASE_RUST_TIMINGS");
        assert!(!enabled());
        {
            let _p = phase("never-recorded");
        }
        count("never-counted");
        let t = totals().lock().unwrap();
        assert!(t.get("never-recorded").is_none());
        assert!(t.get("never-counted").is_none());
    }

    /// VmHWM is a kernel counter, so it is either present and sane or the
    /// platform does not have it. Zero would mean we parsed the wrong field.
    #[test]
    fn peak_rss_is_sane_or_absent() {
        if let Some(kb) = peak_rss_kb() {
            assert!(kb > 0, "VmHWM parsed as zero");
        }
    }
}
