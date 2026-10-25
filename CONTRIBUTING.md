# Contributing

Issues and pull requests are welcome at
https://github.com/griasdi/iced_impeccable. Contributions are dual licensed as
stated in the [License](README.md#license) section of the README.

## Setup

- Rust 1.91 or newer (`rust-version` in `Cargo.toml`).
- `git config core.hooksPath .githooks` enables the rustfmt pre-commit hook.

## Before a pull request

Run the gate:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
scripts/smoke.sh            # tiny-skia, no GPU needed; `scripts/smoke.sh wgpu` for the GPU path
cargo deny --log-level error check
```

Windows compile check: `cargo check --target x86_64-pc-windows-gnu --all-targets`.
CI runs these on Linux and Windows.

- A new or changed command or reply extends `scripts/smoke.sh`.
- Test UI headless (the app's `--headless --control` mode), never by driving
  the desktop.

## Commits

`type(scope): summary`, one line, max 60 chars, imperative. Types: feat, fix,
docs, chore, refactor, test, style, perf.

## Changelog

Every user-visible change gets an entry in `CHANGELOG.md` under
`## [Unreleased]`, in `### Added`, `### Changed` or `### Fixed`
([Keep a Changelog](https://keepachangelog.com/)). The CHANGELOG section
becomes the GitHub release notes.

## Releases

Pull requests do not bump versions. Maintainers release by
[`RELEASING.md`](RELEASING.md): a pushed `vX.Y.Z` tag publishes to crates.io.
