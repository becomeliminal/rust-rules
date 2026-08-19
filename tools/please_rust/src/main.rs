//! please_rust - Rust tooling for the Please build system
//!
//! This binary provides subcommands for:
//! - generate: Parse Cargo.toml and generate BUILD files for subrepos
//! - compile: Compile Rust crates (wraps rustc with externconfig support)
//! - build-script: Execute Cargo build scripts and parse output
//! - testmain: Generate test harness main.rs
//! - cover: Instrument for coverage
//!
//! Set PLEASE_RUST_TIMINGS=1 for per-phase timings, call counts and peak RSS
//! on stderr.

// Both of these are the same piece of work: generate_build_file threads
// seventeen positional arguments and the dep vectors it passes are nested
// tuples, because nothing ever hoisted per-crate state into a struct. That
// hoist is planned as part of `please_rust ide`, which needs exactly that
// struct to serialise. Allowed here rather than silenced at each site so it
// stays one visible debt rather than twenty invisible ones.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

use anyhow::Result;
use clap::{Parser, Subcommand};

mod build_script;
mod compile;
mod generate;
mod pubgrub_solver;
mod resolve;
mod sync;
mod test;
mod timing;
mod workspace;

#[derive(Parser)]
#[command(name = "please_rust")]
#[command(about = "Rust tooling for the Please build system")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Compile a Rust crate (wraps rustc with externconfig support)
    Compile(compile::CompileArgs),

    /// Execute a Cargo build script (build.rs) and output parsed directives
    BuildScript(build_script::BuildScriptArgs),

    /// Parse Cargo.toml and generate BUILD files for a subrepo
    Generate(generate::GenerateArgs),

    /// Resolve versions and features across the declared crate graph
    Resolve(resolve::ResolveArgs),

    /// Maintain the rust_repo declarations and regenerate the lock file
    Sync(sync::SyncArgs),

    /// Resolve new dependency versions from the crates.io sparse index
    Lock(sync::LockCmdArgs),

    /// Run a test command and report per-test results to plz
    Test(test::TestArgs),
}

fn main() -> Result<()> {
    dispatch(Cli::parse())
}

fn dispatch(cli: Cli) -> Result<()> {
    let name = match &cli.command {
        Commands::Compile(_) => "compile",
        Commands::BuildScript(_) => "build-script",
        Commands::Generate(_) => "generate",
        Commands::Resolve(_) => "resolve",
        Commands::Sync(_) => "sync",
        Commands::Lock(_) => "lock",
        Commands::Test(_) => "test",
    };
    // Reported even when the command fails: a run that died slowly is worth
    // as much to a profile as one that succeeded.
    let out = run_command(cli);
    timing::report(name);
    out
}

fn run_command(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Compile(args) => compile::run(args),
        Commands::BuildScript(args) => build_script::run(args),
        Commands::Generate(args) => generate::run(args),
        Commands::Resolve(args) => resolve::run(args),
        Commands::Sync(args) => sync::run(args),
        Commands::Lock(args) => sync::lock(args),
        Commands::Test(args) => test::run(args),
    }
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    #[test]
    fn parses_and_dispatches() {
        // Help output names every subcommand
        let help = match Cli::try_parse_from(["please_rust", "--help"]) {
            Err(e) => e.to_string(),
            Ok(_) => panic!("--help should not parse to a command"),
        };
        for cmd in [
            "compile",
            "build-script",
            "generate",
            "resolve",
            "sync",
            "lock",
            "test",
        ] {
            assert!(help.contains(cmd), "missing {}", cmd);
        }
        assert!(Cli::try_parse_from(["please_rust", "nonsense"]).is_err());

        // A real dispatch through the CLI surface: the test wrapper
        let cli = Cli::try_parse_from([
            "please_rust",
            "test",
            "--suite",
            "s",
            "--",
            "sh",
            "-c",
            "printf 'test a ... ok\n'",
        ])
        .unwrap_or_else(|e| panic!("{}", e));
        std::env::remove_var("RESULTS_FILE");
        dispatch(cli).unwrap();
    }
}
