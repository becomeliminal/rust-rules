subinclude("//build_defs:rust")

# Where a crate's source comes from when it is not crates.io.
#
# Most forges serve /archive/<rev>.tar.gz: github, gitea and its forks such
# as codeberg, and sourcehut all do. GitLab does not. Its /archive/ path
# answers 403 and the tarball lives at /-/archive/<rev>/<project>-<rev>.tar.gz,
# so a gitlab repo declared as though it were github fails at download with
# nothing to say why.
#
# These run when the package is parsed, so a change to the URL scheme fails
# the build rather than a fetch.
_forge_cases = [
    # repo, forge, expected URL
    ["owner/repo", "", "https://github.com/owner/repo/archive/abc123.tar.gz"],
    [
        "https://github.com/owner/repo",
        "",
        "https://github.com/owner/repo/archive/abc123.tar.gz",
    ],
    # A trailing .git is how a Cargo.lock spells the same repository.
    [
        "https://codeberg.org/owner/repo.git",
        "",
        "https://codeberg.org/owner/repo/archive/abc123.tar.gz",
    ],
    # gitlab.com is recognised by its host, and the tarball is named for the
    # project rather than the whole path, so a repo inside a group nests.
    [
        "https://gitlab.com/group/sub/proj",
        "",
        "https://gitlab.com/group/sub/proj/-/archive/abc123/proj-abc123.tar.gz",
    ],
    # A forge whose name says nothing about what it runs has to be told.
    [
        "https://git.corp.example/team/proj",
        "gitlab",
        "https://git.corp.example/team/proj/-/archive/abc123/proj-abc123.tar.gz",
    ],
    [
        "https://git.corp.example/team/proj",
        "",
        "https://git.corp.example/team/proj/archive/abc123.tar.gz",
    ],
]

for _case in _forge_cases:
    _got = forge_archive_url(_case[0], "abc123", "github.com", _case[1])
    if _got != _case[2]:
        fail("forge_archive_url(%s, forge=%s) = %s, want %s" % (_case[0], _case[1], _got, _case[2]))
