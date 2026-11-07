#!/bin/sh
set -eu

main() {
  [ "$(uname -s)" = Linux ] || die "Bork requires Linux"
  case "$(uname -m)" in
    x86_64) arch=x86_64 ;;
    aarch64 | arm64) arch=aarch64 ;;
    *) die "Unsupported architecture: $(uname -m)" ;;
  esac
  for tool in curl sha256sum mktemp install; do
    command -v "$tool" >/dev/null 2>&1 || die "$tool is required"
  done

  destination="$HOME/.local/bin/bork"
  [ ! -d "$destination" ] || die "$destination is a directory"
  scratch=$(mktemp -d)
  staged=""
  trap 'rm -rf "$scratch"; [ -z "$staged" ] || rm -f "$staged"' EXIT
  trap 'exit 1' HUP INT TERM
  binary="bork-linux-$arch"
  base="https://github.com/hschne/bork/releases/latest/download"
  curl -fsSL "$base/$binary" -o "$scratch/$binary"
  curl -fsSL "$base/$binary.sha256" -o "$scratch/checksum"
  read -r expected filename < "$scratch/checksum"
  [ "$filename" = "$binary" ] || die "Unexpected checksum filename"
  actual=$(sha256sum "$scratch/$binary")
  [ "$expected" = "${actual%% *}" ] || die "Checksum mismatch; refusing to install"

  mkdir -p "$(dirname "$destination")"
  staged=$(mktemp "$HOME/.local/bin/.bork.XXXXXX")
  install -m755 "$scratch/$binary" "$staged"
  mv -f "$staged" "$destination"
  printf 'Installed Bork to %s\n' "$destination"
  case ":${PATH:-}:" in
    *":$HOME/.local/bin:"*) ;;
    *) printf 'Add %s/.local/bin to your PATH.\n' "$HOME" ;;
  esac
}

die() { printf 'Error: %s\n' "$*" >&2; exit 1; }

main "$@"
