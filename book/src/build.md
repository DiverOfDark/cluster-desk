# Build from source

Cluster Desk is a single Rust crate using [egui](https://github.com/emilk/egui).

```sh
git clone https://github.com/DiverOfDark/cluster-desk.git
cd cluster-desk
cargo build --release
./target/release/kubelens
```

On Linux, install `pkg-config` and the fontconfig development package first
(`libfontconfig1-dev` on Debian/Ubuntu, `fontconfig-devel` on Fedora).

Builds made with plain `cargo build` don't update themselves.

## Screenshots without a window

```sh
kubelens --screenshot out.png
```

Renders the built-in sample cluster off-screen and saves a picture. CI and tests use it.

## How releases are made

Every commit to `main` runs `.github/workflows/release.yml`:

1. Builds a Linux AppImage (on Ubuntu 22.04), a Windows zip and a universal macOS dmg.
2. Signs the update packages with the `MINISIGN_SECRET_KEY` repository secret.
3. Publishes a GitHub release `v0.1.<run number>` with the packages, signatures and `latest.json`.

Pull requests build the same packages without releasing.

> [!WARNING]
> Keep a copy of the minisign secret key. Installed copies refuse any release signed with
> a different key.

To test the updater against another feed, set `CLUSTER_DESK_UPDATE_URL` to the URL of a `latest.json`.

## These docs

An [mdBook](https://rust-lang.github.io/mdBook/) in `book/`, published to GitHub Pages by
`.github/workflows/pages.yml` on every push to `main` that touches it. Preview locally:

```sh
mdbook serve book --open
```

Screenshots come from the sample cluster.

## Credits

The look and behaviour follow [FreeLens](https://github.com/freelensapp/freelens)
(MIT, © Freelens Authors, © OpenLens Authors). `MECHANICS.md` in the repository lists which
behaviours, colours and values come from it. The pinned kubectl checksum table is taken from FreeLens too.
