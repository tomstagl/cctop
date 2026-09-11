# Packaging

## Release

Tag and push:

```
git tag v0.1.0 && git push origin v0.1.0
```

`.github/workflows/release.yml` then:

1. builds `cctop` for `aarch64-apple-darwin`, `x86_64-apple-darwin`,
   `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu` (`--release --locked`);
2. packages each as `cctop-<version>-<target>.tar.gz` (binary, README, LICENSE,
   the Claude Code plugin) with a `.sha256` next to it and a combined `SHA256SUMS`;
3. creates the GitHub release with generated notes and attaches everything;
4. renders `packaging/homebrew/cctop.rb` with the version and checksums and
   pushes it to the tap repository — only when the repository **variable**
   `HOMEBREW_TAP` (e.g. `tomstagl/homebrew-tap`) and the **secret**
   `HOMEBREW_TAP_TOKEN` (PAT with `contents: write` on the tap) are set;
5. runs `cargo publish --dry-run --locked` (the real publish is manual:
   `cargo publish` with a crates.io token).

## Homebrew tap

Create an empty repository named `homebrew-tap` under your account; the
workflow commits `Formula/cctop.rb` into it. Users then run:

```
brew install tomstagl/tap/cctop
```

The formula installs the binary and the plugin under `share/cctop/plugin`,
and prints the `claude plugin add …` line as a caveat.

## cargo

`Cargo.toml` carries the crates.io metadata (`description`, `license`,
`repository`, `keywords`, `categories`, `readme`). `cargo publish --dry-run`
passes locally and in CI. Package contents are trimmed by the `exclude` list
in `Cargo.toml` (fixtures, brand assets, planning files stay out of the crate).
