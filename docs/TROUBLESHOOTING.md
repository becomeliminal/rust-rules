# Troubleshooting

Real failures, what causes them, and what to do. Each heading is the message
you will actually see.

## `Found multiple definitions for subrepo 'third_party/rust/<crate>'`

Please derives subrepo names from the declaring package path plus the name,
with nothing identifying which repo declared them, so a plugin's
`third_party/rust/serde` and yours are the same global name. Any command
that parses a package of the plugin — `plz cover`, or running a tool target
inside it — hits the clash.

Fixed in v0.3.2: this plugin keeps its own crates in `third_party/crates`.
If you see it against an older version, upgrade. If you see it between two
repos of your own, move one set of declarations to a package the other does
not use; `rust_repo` takes its paths from the package it is declared in, so
that is the whole change.

## `Subrepo <name> is not defined (referenced by <crate>)`

A crate you declared depends on one you did not. The generated build file for
the dependent names a subrepo nothing creates, so the whole graph stops
parsing — `plz build //...` fails before building anything.

It happens most often with a crate whose dependency is optional and enabled by
a default feature, and with platform-specific crates: resolution drops a crate
that no platform you cover reaches, but the declaration still creates a subrepo
whose build file is generated from the manifest.

```sh
please_rust lock --add <dep>@<version> \
    --build-file third_party/crates/BUILD --third-party-folder third_party/crates
```

Declaring it is enough even when the crate never builds here — the declaration
is what makes the graph parseable.

## `Found multiple definitions for subrepo '<plugin>'`

Plugin names are one global namespace. A subrepo that declares a plugin you
also declare collides with yours, and Please stops rather than choosing. It
surfaces when something parses that subrepo's own `plugins/` package, which a
normal build never does but a repo-wide query can.

Nothing needs changing in either repo: query the packages you mean rather than
the whole subrepo. `rust_project` does this for you — a sweep that fails
descends and skips only the package that will not parse, naming it.

## `error: the 'alloc' feature must currently be enabled` (or similar)

A crate is declared but no root reaches it, so there is nothing to unify its
features against and it is built on its own. Before v0.3.4 that meant no
features at all; now it means the crate's default features, which is what
cargo does for a crate built alone.

Either outcome is a hint that the declaration is stale. `please_rust sync`
names declarations resolution never reached; `sync --prune` drops them.

## `<crate> needs <dep> ^<version>, which a feature activated; adding it`

Not an error. Enabling a feature activated an optional dependency that was
not declared, and `lock` is adding it. It appears during `lock --add ...
--features ...` and resolves itself.

## `warning: <crate>: dependency <dep> is not declared, skipping`

Resolution needed something the declaration set does not contain. Run
`please_rust lock --add <dep>@<version>` to declare it. The version matters:
declaring a version the requirement excludes is not a fix, and since v0.3.2
it will not silently be accepted either.

## Coverage reports `No data` with every file at 0%

Please only aggregates coverage for file extensions it knows, and `.rs` is
not in its default list. Add to your `.plzconfig`:

```ini
[cover]
FileExtension = .rs
```

If files still read 0% after that, check the test actually ran: a cached
pass reports cached coverage, and `--rerun` forces it.

## `Target ///python//build_defs:python not found in build graph`

Nothing to do with Rust. The proto plugin's own config preloads the python
plugin, so any repo using proto needs `python` declared in `plugins/BUILD`
and configured. It surfaces from Rust only when a Rust proto rule pulls the
proto plugin in.

## `Bad output hash for rule //third_party/rust:please_rust_tool`

The `remote_file` URL and its `hashes` disagree, usually because one was
bumped without the other. Take the hash from the release, or recompute it:

```sh
curl -sL <url> | sha256sum
```

## The editor shows no completions, and nothing appears to happen

rust-analyzer has to be installed as an extension before any of this matters;
the `discoverConfig` setting is ignored without it, silently.

```sh
code --install-extension rust-lang.rust-analyzer     # or: cursor --install-...
```

Then check the command works on its own, which is the same command your editor
runs:

```sh
plz run //:rust-project        # writes rust-project.json and prints a summary
```

## `unrecognized subcommand 'ide'`

The `please_rust` in use is older than the rules asking it for a project file.
Pin a release that has it, or build the tool from the same source as the rules:

```ini
[Plugin "rust"]
PleaseRustTool = ///rust//tools/please_rust:bootstrap
```

Set it in `.plzconfig` rather than passing `plz -o`. **A nested `plz` does not
inherit command-line overrides**, and project discovery shells out to `plz`, so
`-o` reaches the outer invocation only — the lock gets rebuilt underneath by
whichever tool the config names. `PLZ_OVERRIDES` in the environment does carry
through.

## Third-party crates are listed in the editor but go nowhere

The lock was written by an older `please_rust` than the one generating the
project, so it does not carry the fields that need. The run says so, naming
how many crates it affects. Rebuild it — and check which tool the *lock rule*
uses, not the one you invoked, per the note above.

## `failed to get rustc cfgs ... has no bin/ directory`

rust-analyzer runs `<sysroot>/bin/rustc --print cfg` to learn the target's
cfgs, so `sysroot` has to be a rustup-shaped root — `bin/` beside `lib/` —
which is the rustc component and not what `Sysroot` names. Leave
`rust_project`'s `sysroot` unset and it derives both from the toolchain you
already configured.

## `Error preparing directories ...: unlinkat ...: directory not empty`

A previous build was killed — Ctrl-C, a timeout, an OOM — leaving a partly
written directory that Please then cannot clean. `plz-out/tmp` is scratch and
safe to remove:

```sh
rm -rf plz-out/tmp
```

Rust makes this more visible than most languages: a killed `rustc` leaves
codegen units behind, so the directory refills as Please is emptying it.

## Builds are slower than expected, or everything rebuilds after an upgrade

Every compile passes `--remap-path-prefix`, so artifacts do not embed the
build directory. Upgrading to a version that changed compile flags therefore
invalidates the cache once, and one full rebuild is expected. If chains of
dependent crates dominate the build, try `PipelinedCompilation = true`,
which lets each crate's dependents start against its metadata rather than
waiting for codegen.

## A crate needs a C library, or a `-sys` crate fails to link

C toolchains come from the host by convention, as in go-rules and the cc
plugin. Point `CCTool` at a build label to use something else. For crates
whose build scripts publish link metadata, `links` and
`DEP_<LINKS>_<KEY>` propagation is wired; see `test/links`.

## A cold checkout downloads a Rust toolchain during `plz query`

Parsing a package that references a crate subrepo has to build that subrepo,
and building it needs `please_rust`. With the default the tool is built from
source, which pulls a toolchain and runs the cargo bootstrap — expensive, and
it needs network, which remote execution setups often do not grant build
actions.

Pin the released binary instead (see PleaseRustTool in the README). It is
hash-verified, downloads in seconds, and removes cargo from the picture
entirely.

## `Invalid build label: //third_party/rust:toolchain|rustc`

Entry-point labels are for `tools`, not `deps`. Depend on the toolchain
target itself (`//third_party/rust:toolchain`) where you need it as a
dependency, and use the entry point where you need a particular binary.

If the label instead reaches a shell command, quote it: the `|` is a pipe.

## `can't find crate for 'std'` when building with `--arch`

The toolchain has no standard library for the platform being targeted.
`rust_toolchain` installs one for whatever `--arch` names, so this means the
build is using a toolchain target that was parsed for a different platform,
or `architectures` was used to cross-compile part of a repo by hand without
listing that platform:

```python
rust_toolchain(
    name = "toolchain",
    version = "1.97.1",
    architectures = ["darwin_arm64"],
)
```

## Cross-compiling links against the host's libraries, or fails in the linker

Compiling for another platform needs nothing but its `rust-std`; linking an
executable for one needs a linker that targets it. Rust libraries therefore
cross-compile out of the box and binaries do not. Point `CCTool` at a cross
linker (a build label works) as you would for any other C toolchain here.

Build scripts, proc macros and installed binaries are compiled for the host
whatever `--arch` says, since they run during the build. That is cargo's
split too, and it is why a repo can cross-compile even though its build
scripts execute.

## Upgrading toolchain config

The toolchain layout has moved twice. Before 0.5.0 it was eight sibling
targets; 0.5.0 collapsed them into one output with entry points; 0.6.3 split
out the pieces a build actually uses again, for reasons remote execution
made unavoidable. If you set these explicitly:

```ini
Rustc      = //third_party/rust:toolchain|rustc   ->  //third_party/rust:toolchain_rustc|rustc
Sysroot    = //third_party/rust:toolchain|sysroot ->  //third_party/rust:toolchain_sysroot
LlvmTools  = //third_party/rust:toolchain|llvm-tools -> //third_party/rust:toolchain_llvm_tools
CargoTool  = (derived from Rustc)                 ->  //third_party/rust:toolchain_cargo|cargo
```

`ClippyTool` and `RustfmtTool` stay entry points and move with `Rustc` onto
`toolchain_rustc`. `StdLib` is gone with the `rust_crate` rules it served;
`RustcLib` and `LlvmToolsLib` went in 0.5.0.

Nothing here needs setting in a normal repo - the defaults name all of it.

## Reporting something not listed here

The tool prints the exact rustc invocation before running it, so a failing
compile can be reproduced by hand from the build log. Include that, the
declaration for the crate, and the resolved entry
(`plz build //third_party/crates:rust_lock` then read the JSON) — those
three usually identify the cause immediately.
