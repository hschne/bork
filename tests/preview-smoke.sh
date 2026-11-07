#!/usr/bin/env bash
set -euo pipefail

usage() { echo "Usage: bash tests/preview-smoke.sh EXECUTABLE OUTPUT_DIRECTORY"; }
die() { echo "Error: $*" >&2; exit 1; }

wait_for() {
  local pattern="$1" file="$2"
  for _ in {1..200}; do
    if [[ -f "$file" ]] && grep -q "$pattern" "$file"; then return; fi
    sleep .05
  done
  die "Timed out waiting for $pattern in $file"
}

click_target() {
  local ready="$1" window="$2" name="$3" label x y focused
  read -r label x y focused < <(grep "^$name " "$ready") || die "Missing target $name in $ready"
  xdotool mousemove --window "$window" "$x" "$y" mousedown 1
  sleep .1
  xdotool mouseup 1
}

function main() {
  [[ $# == 2 ]] || { usage; exit 1; }
  local app output scratch xvfb app_pid window mode cleanup capture ready step target
  app="$(realpath "$1")"
  output="$(realpath -m "$2")"
  [[ -x "$app" ]] || die "Missing executable: $app"
  for tool in Xvfb xdotool timeout; do command -v "$tool" >/dev/null || die "$tool is required"; done
  mkdir -p "$output"
  scratch="$(mktemp -d)"
  # Exercise a standalone install with no adjacent assets or source-tree cwd.
  cp "$app" "$scratch/bork"
  app="$scratch/bork"
  cd "$scratch"
  Xvfb -displayfd 3 -screen 0 1280x1000x24 -nolisten tcp 3>"$scratch/display" >"$output/xvfb.log" 2>&1 &
  xvfb=$!
  printf -v cleanup 'kill %q 2>/dev/null || true; rm -rf -- %q' "$xvfb" "$scratch"
  # shellcheck disable=SC2064
  trap "$cleanup" EXIT
  for _ in {1..50}; do [[ -s "$scratch/display" ]] && break; sleep .05; done
  [[ -s "$scratch/display" ]] || die "Xvfb failed to start"
  mkdir -p "$scratch/home" "$scratch/runtime"
  chmod 700 "$scratch/runtime"
  DISPLAY=":$(cat "$scratch/display")"
  export DISPLAY HOME="$scratch/home" XDG_CONFIG_HOME="$scratch/config" XDG_CACHE_HOME="$scratch/cache" XDG_DATA_HOME="$scratch/data" XDG_RUNTIME_DIR="$scratch/runtime" LIBGL_ALWAYS_SOFTWARE=1
  unset WAYLAND_DISPLAY HYPRLAND_INSTANCE_SIGNATURE
  for mode in default tuned signal-loss tear test-card reopened confirmation; do
    capture="$output/preview-$mode.png"
    ready="${capture%.png}.ready"
    rm -f -- "$ready" "${capture%.png}.capture"
    BORK_TEST_CAPTURE="$capture" BORK_TEST_CAPTURE_MANUAL=1 timeout 30 "$app" preview >"$output/preview-$mode.log" 2>&1 &
    app_pid=$!
    printf -v cleanup 'kill %q %q 2>/dev/null || true; rm -rf -- %q' "$app_pid" "$xvfb" "$scratch"
    # shellcheck disable=SC2064
    trap "$cleanup" EXIT
    window="$(timeout 10 xdotool search --sync --onlyvisible --name 'Bork')" \
      || die "Preview $mode did not open; see $output/preview-$mode.log"
    xdotool windowfocus --sync "$window"
    wait_for '^curve-start ' "$ready"
    if [[ "$mode" == tuned ]]; then
      click_target "$ready" "$window" curve-start
      wait_for '^curve-start .* focused$' "$ready"
      xdotool key shift+Up
      wait_for '^intensity 40$' "$ready"
      click_target "$ready" "$window" save
      wait_for '"intensity": 40.0' "$scratch/config/bork/settings.json"
    elif [[ "$mode" == signal-loss || "$mode" == tear ]]; then
      click_target "$ready" "$window" timeline
      wait_for '^timeline .* focused$' "$ready"
      target=0
      [[ "$mode" != tear ]] || target=0.78
      for step in {1..500}; do
        if awk -v target="$target" \
          '$1 == "signal-loss" && $2 != "none" && $2 >= target { found=1 } END { exit !found }' "$ready"; then break; fi
        xdotool key Right
        sleep .02
      done
      grep -q '^signal-loss [0-9]' "$ready" || die "Timeline did not reach signal loss"
    elif [[ "$mode" == test-card ]]; then
      click_target "$ready" "$window" timeline
      wait_for '^timeline .* focused$' "$ready"
      # egui's slider handles arrows, not End; each press moves one UI point.
      xdotool key --repeat 1280 --delay 1 Right
      wait_for '^test-card true$' "$ready"
    elif [[ "$mode" == confirmation ]]; then
      click_target "$ready" "$window" live-preview
    fi
    touch "${capture%.png}.capture"
    wait "$app_pid" || die "Preview $mode failed; see $output/preview-$mode.log"
    [[ -s "$capture" ]] || die "Screenshot missing for $mode"
    [[ ! -s "$output/preview-$mode.log" ]] || die "Preview emitted diagnostics; inspect $output/preview-$mode.log"
    if [[ "$mode" == tuned ]]; then grep -q '"intensity": 40.0' "$output/preview-$mode.json" || die "Curve point was not changed"; fi
    if [[ "$mode" == signal-loss || "$mode" == tear ]]; then grep -q '"signal_loss": [0-9]' "$output/preview-$mode.json" || die "Signal loss was not captured"; fi
    if [[ "$mode" == test-card ]]; then grep -q '"test_card": true' "$output/preview-$mode.json" || die "Timeline did not reach the break card"; fi
    if [[ "$mode" == reopened ]]; then grep -q '"intensity": 40.0' "$output/preview-$mode.json" || die "Saved curve was not reopened"; fi
  done
  cp "$scratch/config/bork/settings.json" "$output/preview-saved-settings.json"
  "$app" --version >"$output/version.txt"
  echo "Preview default, curve editing, signal loss, tear, test card, save/reopen and Live Preview confirmation passed in isolated Xvfb."
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then main "$@"; fi
