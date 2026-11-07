#!/usr/bin/env bash
set -euo pipefail

usage() { echo "Usage: bash tests/install.sh"; }
die() { echo "Error: $*" >&2; exit 1; }

function main() {
  [[ $# == 0 ]] || { usage; exit 1; }
  local installer scratch arch binary cleanup
  installer="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/install.sh"
  scratch="$(mktemp -d)"
  printf -v cleanup 'rm -rf -- %q' "$scratch"
  trap "$cleanup" EXIT
  mkdir -p "$scratch/tools" "$scratch/fixtures"
  cat > "$scratch/tools/uname" <<'SH'
#!/bin/sh
case "$1" in
  -s) echo "${TEST_OS:-Linux}" ;;
  -m) echo "$TEST_ARCH" ;;
esac
SH
  cat > "$scratch/tools/curl" <<'SH'
#!/bin/sh
set -eu
[ "$1" = -fsSL ]
[ "$3" = -o ]
case "$2" in
  https://github.com/hschne/bork/releases/latest/download/bork-linux-*) ;;
  *) exit 1 ;;
esac
[ "${TEST_DOWNLOAD_FAIL:-0}" = 0 ] || exit 22
cp "$TEST_FIXTURES/${2##*/}" "$4"
SH
  chmod +x "$scratch/tools/"*
  export PATH="$scratch/tools:$PATH" TEST_FIXTURES="$scratch/fixtures"
  export HOME="$scratch/home" TMPDIR="$scratch/tmp"
  mkdir -p "$TMPDIR"
  for binary in bork-linux-x86_64 bork-linux-aarch64; do
    printf 'test binary: %s\n' "$binary" > "$TEST_FIXTURES/$binary"
    (cd "$TEST_FIXTURES" && sha256sum "$binary" > "$binary.sha256")
  done

  for arch in x86_64 aarch64 arm64; do
    export TEST_ARCH="$arch"
    binary=bork-linux-aarch64
    [[ "$arch" != x86_64 ]] || binary=bork-linux-x86_64
    sh "$installer" > "$scratch/output"
    [[ -x "$HOME/.local/bin/bork" ]] || die "Binary is not executable"
    cmp "$HOME/.local/bin/bork" "$TEST_FIXTURES/$binary"
  done

  cp "$HOME/.local/bin/bork" "$scratch/previous"
  export TEST_ARCH=x86_64
  printf 'corrupted download\n' > "$TEST_FIXTURES/bork-linux-x86_64"
  if sh "$installer" > "$scratch/output" 2>&1; then die "Accepted checksum mismatch"; fi
  grep -q 'Checksum mismatch' "$scratch/output"
  cmp "$HOME/.local/bin/bork" "$scratch/previous"

  if TEST_DOWNLOAD_FAIL=1 sh "$installer" > "$scratch/output" 2>&1; then die "Accepted failed download"; fi
  cmp "$HOME/.local/bin/bork" "$scratch/previous"
  if TEST_ARCH=riscv64 sh "$installer" > "$scratch/output" 2>&1; then die "Accepted unsupported architecture"; fi
  if TEST_OS=Darwin sh "$installer" > "$scratch/output" 2>&1; then die "Accepted unsupported OS"; fi

  # A checksum for a different file must not authorize installing this binary.
  printf '0  unrelated\n' > "$TEST_FIXTURES/bork-linux-x86_64.sha256"
  if sh "$installer" > "$scratch/output" 2>&1; then die "Accepted unrelated checksum"; fi
  grep -q 'Unexpected checksum filename' "$scratch/output"
  cmp "$HOME/.local/bin/bork" "$scratch/previous"
  [[ -z "$(ls -A "$TMPDIR")" ]] || die "Temporary downloads were not cleaned up"
  [[ -z "$(find "$HOME/.local/bin" -name '.bork.*' -print)" ]] || die "Staging files were not cleaned up"
  echo "Installer architecture selection, installation, updates, checksum rejection and download failures passed."
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then main "$@"; fi
