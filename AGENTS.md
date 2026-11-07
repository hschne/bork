# Bork

## Documentation

- Never add documentation files. Put explanations in the conversation instead.

## Verification

- Use `mise.toml` for toolchain and tasks. Run `mise run check` for Rust changes and `mise run release` for distribution changes; native Nix builds do not verify the shipped executable.
- For graphics development on NixOS, use `nix develop path:./nix`; using the repository root copies build outputs into the Nix store.
- Keep regression tests runnable in CI; do not add ignored or manual-only tests. Run graphics tests in isolated sessions. Active-desktop effects and persistent system configuration changes require approval.
- Put temporary logs, captures, and test sessions in the system temporary directory; reserve `dist/` for deployable outputs.

## Invariants

- Only the guardian mutates the desktop; keep timing and effect calculation independent of compositor state.
- Journal previous compositor settings before mutation. Restore only values Bork still owns, preserving external changes.
- Keep status and emergency reset independent of graphics initialization, including when graphics configuration is broken.
