#!/usr/bin/env python3
"""Generates the synthetic monorepo used by scripts/benchmark.sh.

One source tree that builds under both systems: a cargo workspace (root
Cargo.toml + per-crate Cargo.toml with path deps) and a plz repo (per-crate
BUILD with rust_library + rust_test). LAYERS x WIDTH crates; each crate
depends on two crates from the previous layer and carries a unit test.
"""

import os
import sys

LAYERS = 8
WIDTH = 5
FUNCS = 60  # per crate, to give rustc something measurable to chew on

def crate_name(layer, idx):
    return f"l{layer}c{idx}"

def gen_lib(layer, idx, deps):
    lines = []
    for dep in deps:
        lines.append(f"use {dep};")
    lines.append("")
    for f in range(FUNCS):
        if deps and f % 3 == 0:
            dep = deps[f % len(deps)]
            lines.append(f"pub fn work_{f}(x: u64) -> u64 {{")
            lines.append(f"    let y = {dep}::work_{f}(x.wrapping_add({f}));")
            lines.append("    let mut acc = y;")
            lines.append(f"    for i in 0..{(f % 7) + 3} {{")
            lines.append("        acc = acc.wrapping_mul(31).wrapping_add(i);")
            lines.append("    }")
            lines.append("    acc")
            lines.append("}")
        else:
            lines.append(f"pub fn work_{f}(x: u64) -> u64 {{")
            lines.append(f"    let mut acc = x.wrapping_add({f});")
            lines.append("    match acc % 5 {")
            lines.append("        0 => acc = acc.wrapping_mul(3),")
            lines.append("        1 => acc = acc.rotate_left(7),")
            lines.append("        2 => acc = acc.wrapping_sub(11),")
            lines.append("        3 => acc = acc ^ 0xdead_beef,")
            lines.append("        _ => acc = acc.wrapping_add(acc >> 3),")
            lines.append("    }")
            lines.append(f"    for i in 0..{(f % 9) + 2} {{")
            lines.append("        acc = acc.wrapping_mul(1_000_003).wrapping_add(i);")
            lines.append("    }")
            lines.append("    acc")
            lines.append("}")
        lines.append("")
    lines.append("#[cfg(test)]")
    lines.append("mod tests {")
    lines.append("    #[test]")
    lines.append("    fn work_is_deterministic() {")
    lines.append("        assert_eq!(super::work_0(1), super::work_0(1));")
    lines.append("        assert_ne!(super::work_1(1), super::work_1(2));")
    lines.append("    }")
    lines.append("}")
    return "\n".join(lines) + "\n"

def main(out_dir):
    crates_dir = os.path.join(out_dir, "crates")
    os.makedirs(crates_dir, exist_ok=True)
    members = []
    for layer in range(LAYERS):
        for idx in range(WIDTH):
            name = crate_name(layer, idx)
            members.append(f"crates/{name}")
            deps = []
            if layer > 0:
                deps = [crate_name(layer - 1, idx % WIDTH),
                        crate_name(layer - 1, (idx + 1) % WIDTH)]
            cdir = os.path.join(crates_dir, name, "src")
            os.makedirs(cdir, exist_ok=True)
            with open(os.path.join(cdir, "lib.rs"), "w") as f:
                f.write(gen_lib(layer, idx, deps))
            dep_lines = "\n".join(
                f'{d} = {{ path = "../{d}" }}' for d in deps)
            with open(os.path.join(crates_dir, name, "Cargo.toml"), "w") as f:
                f.write(f"""[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[dependencies]
{dep_lines}
""")
            dep_labels = ",\n        ".join(f'"//crates/{d}"' for d in deps)
            deps_attr = f"\n    deps = [\n        {dep_labels},\n    ]," if deps else ""
            with open(os.path.join(crates_dir, name, "BUILD"), "w") as f:
                f.write(f"""subinclude("///rust//build_defs:rust")

rust_library(
    name = "{name}",
    root = "src/lib.rs",
    edition = "2021",
    visibility = ["PUBLIC"],{deps_attr}
)

rust_test(
    name = "test",
    root = "src/lib.rs",
    edition = "2021",{deps_attr}
)
""")
    member_lines = ",\n    ".join(f'"{m}"' for m in members)
    with open(os.path.join(out_dir, "Cargo.toml"), "w") as f:
        f.write(f"""[workspace]
resolver = "2"
members = [
    {member_lines}
]
""")
    os.makedirs(os.path.join(out_dir, "plugins"), exist_ok=True)
    with open(os.path.join(out_dir, "plugins", "BUILD"), "w") as f:
        f.write("""plugin_repo(
    name = "rust",
    owner = "becomeliminal",
    plugin = "rust-rules",
    revision = "master",
)
""")
    with open(os.path.join(out_dir, "BUILD"), "w") as f:
        f.write("")
    with open(os.path.join(out_dir, ".plzconfig"), "w") as f:
        f.write("""[please]
version = 17.27.0

[Parse]
BlacklistDirs = target

[Plugin "rust"]
Target = //plugins:rust
""")
    n = LAYERS * WIDTH
    print(f"generated {n} crates ({LAYERS} layers x {WIDTH}) in {out_dir}")

if __name__ == "__main__":
    main(sys.argv[1])
