subinclude("//build_defs:rust", "//build_defs:rust_repo")

# Regression test for --extern filtering in first-party rules. A dep names
# the declaration it wants and takes the crate name from it; deriving the
# crate from the label instead gets it wrong twice over - a label carries the
# package name, and a version is not one hyphenated part - and a dep that
# matches nothing silently loses its --extern.

[
    genrule(
        name = f"{_dir}_src",
        srcs = glob([f"{_dir}/**"]),
        # Under a directory of its own: rust_repo's subrepo output is
        # named after the rule, and one of these rules is named for a version.
        outs = [f"srcs/{_out}"],
        cmd = f"mkdir -p $(dirname $OUT) && mv $PKG_DIR/{_dir} $OUT",
    )
    for _dir, _out in [
        ("renamed", "renamed-pkg-0.1.0"),
        ("twin_old", "twin-0.1.0"),
        ("twin_new", "twin-0.2.0+spec-1.0"),
    ]
]

rust_repo(
    name = "renamed_pkg",
    crate = "renamed-pkg",
    download = ":renamed_src",
    lock = ":firstparty_lock",
    third_party_path = "test/firstparty",
    version = "0.1.0",
)

# Named with its version, the way a second declaration of one crate is -
# and a version with build metadata in it, because that is where guessing the
# crate by stripping the last hyphenated part of the name comes apart.
rust_repo(
    name = "twin-0.2.0+spec-1.0",
    crate = "twin",
    download = ":twin_new_src",
    lock = ":firstparty_lock",
    third_party_path = "test/firstparty",
    version = "0.2.0+spec-1.0",
)

rust_repo(
    name = "twin_old",
    crate = "twin",
    download = ":twin_old_src",
    indirect = True,
    lock = ":firstparty_lock",
    third_party_path = "test/firstparty",
    version = "0.1.0",
)

rust_resolve(
    name = "firstparty_lock",
    entries = [
        "renamed_pkg|renamed-pkg|0.1.0||true|true",
        "twin-0.2.0+spec-1.0|twin|0.2.0+spec-1.0||true|true",
        "twin_old|twin|0.1.0||false|true",
    ],
)

rust_test(
    name = "deps_test",
    root = "deps_test.rs",
    edition = "2021",
    deps = [
        ":renamed_pkg",
        ":twin-0.2.0+spec-1.0",
    ],
)
