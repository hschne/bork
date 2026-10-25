# Development

[Back to README](../README.md)

Run commands below from the repository root. The toolchain is pinned in `mise.toml`; install it with `mise install` if you use mise. Keep builds within half of available CPU capacity (`CARGO_BUILD_JOBS` for native builds; `--build-arg BUILD_JOBS=N` for Docker builds).

## Build a deployable executable

With Docker/BuildKit:

```sh
docker build --target artifact --output type=local,dest=dist .
./dist/bork --version
```

`dist/bork` is the application. The output also includes the README, reference docs, lockscreen artwork, MIT license, and third-party license notices. ELF inspection files are optional diagnostics.

For native development:

```sh
cargo build --locked
```

`mise run release` provides the container release build. Native `target/release` artifacts inherit their build host's loader/library paths; do not mistake a Nix-linked native artifact for the portable Debian-built one.

Optional NixOS development graphics environment:

```sh
nix develop path:./nix
```

Use `path:./nix`, not `path:.`, to avoid copying `target/` into the Nix store. This is an ephemeral shell with a project-local flake, not a system rebuild.

## Testing

`mise run check` runs formatting, Clippy, and ordinary tests without activating desktop effects.

Optional full display checks:

```sh
# Installed Hyprland inside headless Sway:
nix develop path:./nix --command bash tests/nested-session.sh
# Same display scenario from Debian userspace:
nix develop path:./nix --command bash tests/debian-session.sh
```

Display checks run isolated from the active desktop. The harness owns their setup and compatibility workarounds.

## Release candidates

CI runs the container build, formatting, Clippy, ordinary tests, and an isolated software-OpenGL preview check. A successful run uploads `bork-x86_64-unknown-linux-gnu`: a versioned `.tar.gz` and its `.sha256` checksum. It does not create a tag, GitHub release, or registry publication. The workflow can also be run manually from GitHub Actions.

Verify the checksum from the download directory with `sha256sum -c bork-0.1.0-x86_64-unknown-linux-gnu.tar.gz.sha256`, then extract the archive. The package includes documentation, licenses, and optional lockscreen artwork alongside `bork`. Only Linux x86-64 is packaged by this workflow.

Before tagging, review the candidate against the documented host requirements, confirm that the preview demo matches the current UI, and scan the final working tree and Git history for secrets. A green build is not a test of every compositor or graphics driver.

### Dependency notices and audit

`THIRD_PARTY_LICENSES.txt` includes the normal/build Linux dependency tree and embedded-font notices. Refresh it whenever dependencies change. Generate license data with `cargo bundle-licenses --format json --output .ci-evidence/third-party.json` (cargo-bundle-licenses 4.2.0), and restrict the inventory to `cargo tree --locked --target x86_64-unknown-linux-gnu --edges normal,build`. Preserve the full license texts and copyright notices, not just SPDX identifiers.

Some crates omit license files from their published archives. Resolve any `NOT FOUND` entries from the exact upstream commit in the crate’s `.cargo_vcs_info.json`; the current notice file records these sources. Include the Hack, Noto Emoji, Ubuntu, and emoji-icon-font notices used by egui’s embedded fonts.

Run `cargo audit` against the lockfile before a release. RustSec marks the transitive `ttf-parser 0.25.1` dependency as unmaintained (`RUSTSEC-2026-0192`). `clipboard-win 5.4.1` is yanked and belongs to the Windows-only dependency tree, not the Linux package. Review these warnings when updating egui and its dependencies; do not suppress them globally.

## Code structure

- `src/cycle.rs`: work debt, continuous breaks, and grace/deadline transitions; no I/O.
- `src/effect.rs`: validated glitchiness, grace curves, and effect tuning. `effect/shader.rs` adapts the shared GLSL.
- `src/control.rs`: JSON timer commands and status; its private server owns the background control socket.
- `src/background.rs`: clock/input observation and publication of display intent; no installation state.
- `src/recovery/`: the guardian owns display state, trial precedence, and lock suppression. Process transport, editor trial ownership, and write-ahead recovery are private modules.
- `src/wayland/`: input monitoring and software test cards have separate types and protocol state.
- `src/preview/`: controls, shader scene/GL resources, and screenshot verification are separate from the editor lifecycle.

Only the guardian mutates the desktop. Neither the break cycle nor the editor owns compositor installation or recovery policy.
