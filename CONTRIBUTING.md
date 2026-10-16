# Contributing to MangaPoint

Thank you for helping. Bug reports, reader specs, ideas and pull requests are all welcome.

## Reporting a problem

Open an issue and fill in the template: what happened, the steps, your system and
MangaPoint's version (**Settings → About**). The newest file in the `logs` folder of
the [data folder](README.md#your-data) is the most useful thing you can attach;
remove anything private first.

Adding or correcting a **reader** (screen size in inches, pixels per inch, the file
types it opens, whether CrossPoint runs on it) is one of the easiest and most
useful contributions. The catalog is in `crates/device-profiles/src/lib.rs`.

## Setting up

You need stable [Rust](https://rustup.rs). On Linux also install the packages in the
[README](README.md#linux-packages). Then `cargo run -p mangapoint`.

Before opening a pull request, run what CI runs:

```sh
cargo clippy --workspace --all-targets --features catalog/fake,converter/decode -- -D warnings
cargo test   --workspace --features catalog/fake,converter/decode
python -m unittest tools/test_release.py
```

If you changed a screen, run the app and click through what you changed. The
`tools/ui-smoke.ps1` script (Windows) does a basic version of that with real mouse
events.

## Making a change

1. Fork the repository and make a branch from `main`.
2. Keep one pull request to one idea.
3. Write the pull request **title** as a Conventional Commit (below). The
   repository squash-merges, so the title becomes the commit on `main`, and a check
   on the pull request tells you if the title does not fit.
4. Describe what changed, why, and how you tried it.

The app's look is deliberately plain and consistent: a black theme, one button size,
little explanatory text on screen (put it behind a "?" instead), and pixel-art
animation that moves in whole pixels. Please match the screens around your change.

## Releases and versions

The version is written in one place, `[workspace.package] version` in `Cargo.toml`,
and follows [Semantic Versioning](https://semver.org).

**What decides it is the title of each pull request merged since the last release:**

| Title starts with | Meaning | Raises |
|---|---|---|
| `feat:` | a new feature | minor (1.0.0 → 1.1.0) |
| `fix:`, `perf:` | a bug fix, or faster | patch (1.0.0 → 1.0.1) |
| `feat!:` or `BREAKING CHANGE` in the description | something existing stops working the old way | major (1.0.0 → 2.0.0) |
| `docs:`, `refactor:`, `test:`, `build:`, `ci:`, `chore:`, `style:` | no change for users | nothing by themselves |

The largest one wins, so a `fix:` and a `feat:` together make a minor release.
Examples: `feat: export CBZ files`, `fix(export): the cover was stretched`,
`feat!: move the library to a new folder`.

**A release is made by merging a version change.** The **Release** workflow runs on
every push to `main` and looks at the version in `Cargo.toml`. If that version
already has a release, it stops after a couple of seconds and builds nothing. If it
has no release yet, it builds Windows, Linux and macOS, tags the commit and
publishes. So a pull request that raises the version (and adds its section to
`CHANGELOG.md`) publishes its own release when it is merged, and any pull request
that leaves the version alone does not. Raise the version in your own pull request
with `python tools/release.py apply X.Y.Z`, or let a workflow do it:

**How a release is made** (maintainers):

1. Open **Actions → Prepare a release → Run workflow**. Leave it on `auto` to take
   the next version from the titles, or choose `patch`, `minor` or `major`. It opens
   a pull request named `chore(release): vX.Y.Z` that raises the version and adds the
   section to `CHANGELOG.md`.
2. Read the changelog in that pull request and edit it if you like.
3. Merge it. The **Release** workflow sees a version with no release yet, builds
   Windows, Linux and macOS, tags the commit `vX.Y.Z` and publishes the GitHub
   release with the files, their checksums and the changelog section as the notes.

Nobody tags or uploads by hand. Pull requests opened by the workflow do not start
the usual checks (a GitHub rule for automatic pushes); close and reopen the pull
request to run them.

## Conduct

Be kind and assume good faith. Harassment of any kind is not welcome.
