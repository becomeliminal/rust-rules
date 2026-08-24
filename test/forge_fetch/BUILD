subinclude("//build_defs:rust", "//build_defs:rust_repo")

# A crate fetched from a forge that is not github.
#
# The archive URL is built per forge scheme, and the scheme was github's for
# every host until #21. Asserting the URL is one thing; whether the bytes
# arrive, extract to the layout rust_repo expects, and compile is another,
# and only a real fetch answers it. codeberg runs forgejo, a gitea fork,
# which serves github's /archive/<rev>.tar.gz.
rust_repo(
    name = "unit_prefix",
    crate = "unit-prefix",
    git_repo = "https://codeberg.org/commons-rs/unit-prefix",
    git_revision = "v0.5.2",
    lock = ":forge_lock",
    third_party_path = "test/forge_fetch",
    version = "0.5.2",
)

# gitlab serves a different scheme, /-/archive/<rev>/<name>-<rev>.tar.gz.
# Its /archive/ path answers 403, so a gitlab crate declared as though it
# were github fetches nothing.
rust_repo(
    name = "dunce",
    crate = "dunce",
    git_repo = "https://gitlab.com/kornelski/dunce",
    git_revision = "v1.0.5",
    lock = ":forge_lock",
    third_party_path = "test/forge_fetch",
    version = "1.0.5",
)

rust_resolve(
    name = "forge_lock",
    entries = [
        "unit_prefix|unit-prefix|0.5.2||true|true",
        "dunce|dunce|1.0.5||true|true",
    ],
)

# A forge whose host says nothing about what it runs, which is the case
# inference cannot get right: salsa.debian.org is gitlab. Only the download
# is exercised, because the crate behind it is a binary with a dependency
# graph this test has no use for.
rust_git_download(
    name = "salsa_download",
    crate = "debcargo",
    forge = "gitlab",
    repo = "https://salsa.debian.org/rust-team/debcargo",
    revision = "2.8.4",
    version = "2.8.4",
)

genrule(
    name = "salsa_fetch_test",
    srcs = [":salsa_download"],
    outs = ["salsa_fetch.txt"],
    cmd = "grep -q '^name = \"debcargo\"' $SRCS/Cargo.toml && echo ok > $OUT",
)

rust_test(
    name = "forge_fetch_test",
    root = "forge_fetch_test.rs",
    edition = "2021",
    deps = [":unit_prefix", ":dunce"],
)
