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

## Reporting something not listed here

The tool prints the exact rustc invocation before running it, so a failing
compile can be reproduced by hand from the build log. Include that, the
declaration for the crate, and the resolved entry
(`plz build //third_party/crates:rust_lock` then read the JSON) — those
three usually identify the cause immediately.
