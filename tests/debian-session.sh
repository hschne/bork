#!/usr/bin/env bash
set -euo pipefail

usage() { echo "Usage: nix develop path:./nix --command bash tests/debian-session.sh"; }
die() { echo "Error: $*" >&2; exit 1; }

function main() {
  [[ $# == 0 ]] || { usage; exit 1; }
  local output scratch native runtime="" status cleanup
  output="$PWD/.agents/evidence/debian-session"
  mkdir -p "$output"
  if [[ "${SIGNAL_LOSS_VERIFY_IMAGE_READY:-}" != 1 ]]; then
    docker buildx build --load --build-arg "BUILD_JOBS=${CARGO_BUILD_JOBS:-2}" --target verify -t signal-loss-verify:local . >"$output/image-build.log" 2>&1
  fi
  scratch="$(mktemp -d)"
  SIGNAL_LOSS_RUNTIME_FILE="$scratch/runtime" SIGNAL_LOSS_EXTERNAL_PROOF=1 SIGNAL_LOSS_EVIDENCE_DIR="$output" \
    bash tests/nested-session.sh >"$output/session.log" 2>&1 &
  native=$!
  # Expand/escape owned fixture paths before main's locals leave scope.
  printf -v cleanup 'kill %q 2>/dev/null || true; rm -rf -- %q' "$native" "$scratch"
  # shellcheck disable=SC2064
  trap "$cleanup" EXIT
  for _ in {1..1200}; do
    [[ ! -f "$scratch/runtime" ]] || runtime="$(cat "$scratch/runtime")"
    [[ -z "$runtime" || ! -f "$runtime/test-ready" ]] || break
    kill -0 "$native" 2>/dev/null || die "Isolated session failed; see $output/session.log"
    sleep .1
  done
  [[ -n "$runtime" && -f "$runtime/test-ready" ]] || die "Isolated session never became ready"
  status=0
  docker run --rm --cpus "${SIGNAL_LOSS_CPU_LIMIT:-2}" --network none --read-only --cap-drop ALL --security-opt no-new-privileges \
    --user "$(id -u):$(id -g)" --tmpfs /tmp --tmpfs /run \
    -v "$runtime:$runtime" \
    -e "XDG_RUNTIME_DIR=$runtime" -e "TMPDIR=$runtime" -e "HOME=$runtime/home" -e "XDG_CACHE_HOME=$runtime/home/.cache" \
    -e "WAYLAND_DISPLAY=$(cat "$runtime/test-display")" -e "HYPRLAND_INSTANCE_SIGNATURE=$(cat "$runtime/test-signature")" \
    -e SIGNAL_LOSS_ISOLATED=1 -e SIGNAL_LOSS_GRIM=/usr/bin/grim -e SIGNAL_LOSS_TEST_BINARY=/artifact/signal-loss \
    signal-loss-verify:local timeout 60 /artifact/display-test --ignored --nocapture --test-threads=1 >"$output/nested-test.log" 2>&1 || status=$?
  printf '%s\n' "$status" >"$runtime/test-status"
  wait "$native" || die "Session/proof failed; see $output/nested-test.log and session.log"
  [[ "$status" == 0 ]] || die "Debian proof failed"
  trap - EXIT
  rm -rf -- "$scratch"
  echo "Debian release binary passed the real protocol/background/display proof against isolated Hyprland."
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then main "$@"; fi
