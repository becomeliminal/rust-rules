subinclude("//build_defs:rust")

rust_toolchain(
    name = "toolchain",
    # One per platform anyone builds this repo on: the download is for the
    # host, so a Mac fetches a different tarball than linux does and plz
    # accepts whichever of these matches.
    hashes = [
        "b4cdbc7cc6b0ee0a2666b1872769fdb2ad8393b28b63952f6493b4b400e4832b",  # x86_64-unknown-linux-gnu
        "cbd14c36f039f6f11f38148a6295d8234d18ddf20bea53031c86f119423a8b26",  # aarch64-apple-darwin
        "5f4c84d2bcce7983468642855a45fc4978fba1324cfdd1ea0b182face3ab4ffa",  # x86_64-apple-darwin
    ],
    version = "1.97.1",
    visibility = ["PUBLIC"],
)
