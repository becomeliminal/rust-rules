//! please_rust - Rust tooling for the Please build system
//!
//! This binary provides subcommands for:
//! - generate: Parse Cargo.toml and generate BUILD files for subrepos
//! - compile: Compile Rust crates (wraps rustc with externconfig support)
//! - build-script: Execute Cargo build scripts and parse output
//! - filter: Filter source files by cfg/features
//! - testmain: Generate test harness main.rs
//! - cover: Instrument for coverage
//! - crate_info: Emit crate metadata as JSON

use anyhow::Result;
use clap::{Parser, Subcommand};

mod build_script;
mod compile;
mod filter;
mod generate;
mod resolve;
mod starlark;
mod sync;

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

    /// Filter source files by cfg/features
    Filter(filter::FilterArgs),

    /// Resolve versions and features across the declared crate graph
    Resolve(resolve::ResolveArgs),

    /// Maintain the rust_repo declarations and regenerate the lock file
    Sync(sync::SyncArgs),
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Compile(args) => compile::run(args),
        Commands::BuildScript(args) => build_script::run(args),
        Commands::Generate(args) => generate::run(args),
        Commands::Filter(args) => filter::run(args),
        Commands::Resolve(args) => resolve::run(args),
        Commands::Sync(args) => sync::run(args),
    }
}
