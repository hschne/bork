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
  Xvfb -displayfd 3 -screen 0 1280x1000x24 -nolisten tcp 3>"$scratch/display" >"$output/xvfb.log" 2>&1 &
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
  unset WAYLAND_DISPLAY HYPRLAND_INSTANCE_SIGNATURE
  for mode in default tuned test-card reopened confirmation; do
    BORK_TEST_CAPTURE="$output/preview-$mode.png" timeout 15 "$app" preview >"$output/preview-$mode.log" 2>&1 &
    app_pid=$!
    window="$(timeout 8 xdotool search --sync --onlyvisible --name 'Bork')"
    xdotool windowfocus --sync "$window"
    # The window becomes visible before egui completes its first layout pass.
    sleep .3
    if [[ "$mode" == tuned ]]; then
      xdotool mousemove --window "$window" 599 716 mousedown 1
      sleep .1
      xdotool mouseup 1
      sleep .1
      xdotool key shift+Up
      sleep .1
      xdotool mousemove --window "$window" 1050 880 click 1
    elif [[ "$mode" == test-card ]]; then
      xdotool mousemove --window "$window" 880 488 mousedown 1
      sleep .1
      xdotool mouseup 1
    elif [[ "$mode" == confirmation ]]; then
      xdotool mousemove --window "$window" 1050 488 mousedown 1
      sleep .1
      xdotool mouseup 1
    fi
    wait "$app_pid" || die "Preview $mode failed; see $output/preview-$mode.log"
    [[ -s "$output/preview-$mode.png" ]] || die "Screenshot missing for $mode"
    [[ ! -s "$output/preview-$mode.log" ]] || die "Preview emitted diagnostics; inspect $output/preview-$mode.log"
    if [[ "$mode" == tuned ]]; then grep -q '"intensity": 40.0' "$output/preview-$mode.json" || die "Curve point was not changed"; fi
    if [[ "$mode" == test-card ]]; then grep -q '"test_card": true' "$output/preview-$mode.json" || die "Timeline did not reach the break card"; fi
    if [[ "$mode" == reopened ]]; then grep -q '"intensity": 40.0' "$output/preview-$mode.json" || die "Saved curve was not reopened"; fi
  done
  [[ -f "$scratch/config/bork/settings.json" ]] || die "Save did not create settings"
  grep -q '"intensity": 40.0' "$scratch/config/bork/settings.json" || die "Curve was not saved"
  cp "$scratch/config/bork/settings.json" "$output/preview-saved-settings.json"
  "$app" --version >"$output/version.txt"
  echo "Preview default, curve editing, timeline, save/reopen and Live Preview confirmation captured in isolated Xvfb."
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then main "$@"; fi
