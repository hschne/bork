#!/usr/bin/env bash
set -euo pipefail

usage() { echo "Usage: bash tests/preview-smoke.sh EXECUTABLE EVIDENCE_DIRECTORY"; }
die() { echo "Error: $*" >&2; exit 1; }

function main() {
  [[ $# == 2 ]] || { usage; exit 1; }
  local app output scratch xvfb app_pid="" window mode cleanup
  app="$(realpath "$1")"
  output="$(realpath -m "$2")"
  [[ -x "$app" ]] || die "Missing executable: $app"
  for tool in Xvfb xdotool timeout; do command -v "$tool" >/dev/null || die "$tool is required"; done
  mkdir -p "$output"
  scratch="$(mktemp -d)"
  Xvfb -displayfd 3 -screen 0 1280x900x24 -nolisten tcp 3>"$scratch/display" >"$output/xvfb.log" 2>&1 &
  xvfb=$!
  # Expand/escape owned fixture paths before main's locals leave scope.
  printf -v cleanup 'kill %q 2>/dev/null || true; rm -rf -- %q' "$xvfb" "$scratch"
  # shellcheck disable=SC2064
  trap "$cleanup" EXIT
  for _ in {1..50}; do [[ -s "$scratch/display" ]] && break; sleep .05; done
  [[ -s "$scratch/display" ]] || die "Xvfb failed to start"
  mkdir -p "$scratch/home"
  DISPLAY=":$(cat "$scratch/display")"
  export DISPLAY HOME="$scratch/home" XDG_CONFIG_HOME="$scratch/config" XDG_CACHE_HOME="$scratch/cache" LIBGL_ALWAYS_SOFTWARE=1
  unset WAYLAND_DISPLAY
  for mode in default maximum test-card reopened; do
    SIGNAL_LOSS_TEST_CAPTURE="$output/preview-$mode.png" timeout 15 "$app" preview >"$output/preview-$mode.log" 2>&1 &
    app_pid=$!
    window="$(timeout 8 xdotool search --sync --onlyvisible --name 'Signal Loss')"
    xdotool windowfocus --sync "$window"
    if [[ "$mode" == maximum ]]; then
      xdotool mousemove --window "$window" 215 169 click --repeat 2 --delay 100 1
      xdotool key ctrl+a
      sleep .1
      xdotool type --clearmodifiers --delay 100 '100'
      sleep .1
      xdotool key Return
      xdotool mousemove --window "$window" 15 326 click 1
      xdotool mousemove --window "$window" 50 554 click 1
    elif [[ "$mode" == test-card ]]; then
      xdotool mousemove --window "$window" 15 226 click 1
    fi
    wait "$app_pid" || die "Preview $mode failed; see $output/preview-$mode.log"
    [[ -s "$output/preview-$mode.png" ]] || die "Screenshot missing for $mode"
    [[ ! -s "$output/preview-$mode.log" ]] || die "Preview emitted diagnostics; inspect $output/preview-$mode.log"
    if [[ "$mode" == maximum ]]; then grep -q '"level": 100.0' "$output/preview-$mode.json" || die "Numeric maximum was not entered"; fi
    if [[ "$mode" == test-card ]]; then grep -q '"test_card": true' "$output/preview-$mode.json" || die "Card toggle failed"; fi
    if [[ "$mode" == reopened ]]; then grep -q '"tears_enabled": false' "$output/preview-$mode.json" || die "Saved toggle was not reopened"; fi
  done
  [[ -f "$scratch/config/signal-loss/settings.json" ]] || die "Save did not create settings"
  grep -q '"enabled": false' "$scratch/config/signal-loss/settings.json" || die "Toggle was not saved"
  cp "$scratch/config/signal-loss/settings.json" "$output/preview-saved-settings.json"
  "$app" --version >"$output/version.txt"
  echo "Preview default, maximum, test card, save and reopen passed in isolated Xvfb."
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then main "$@"; fi
