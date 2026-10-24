#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: nix develop path:./nix --command bash tests/nested-session.sh"
  echo "Starts isolated headless Sway + nested Hyprland with only a render node, never DRM outputs."
}

die() { echo "Error: $*" >&2; exit 1; }

inside() {
  local sway="$1" hyprland="$2" app="$3" proof="$4"
  local parent_pid child_pid
  WLR_BACKENDS=headless WLR_HEADLESS_OUTPUTS=2 WLR_RENDERER=gles2 \
    WLR_RENDER_DRM_DEVICE=/dev/dri/renderD128 "$sway" -c "$XDG_RUNTIME_DIR/sway.conf" >"$XDG_RUNTIME_DIR/sway.log" 2>&1 &
  parent_pid=$!
  # The PID namespace kills all children when this shell exits, even on failure.
  local socket=""
  for _ in {1..100}; do
    for socket in "$XDG_RUNTIME_DIR"/wayland-*; do
      [[ -S "$socket" ]] && break 2
    done
    sleep .1
  done
  [[ -S "$socket" ]] || die "Parent compositor did not start"
  export WAYLAND_DISPLAY="${socket##*/}"
  local config="$XDG_RUNTIME_DIR/hyprland.conf"
  [[ "${BORK_LUA_CONFIG:-}" != 1 ]] || config="$XDG_RUNTIME_DIR/hyprland.lua"
  "$hyprland" -c "$config" >"$XDG_RUNTIME_DIR/hyprland.log" 2>&1 &
  child_pid=$!
  local ipc=""
  for _ in {1..100}; do
    for ipc in "$XDG_RUNTIME_DIR"/hypr/*/.socket.sock; do
      [[ -S "$ipc" ]] && break 2
    done
    sleep .1
  done
  [[ -S "$ipc" ]] || die "Nested Hyprland did not start"
  HYPRLAND_INSTANCE_SIGNATURE="$(basename "$(dirname "$ipc")")"
  export HYPRLAND_INSTANCE_SIGNATURE
  for socket in "$XDG_RUNTIME_DIR"/wayland-*; do
    if [[ -S "$socket" && "${socket##*/}" != "$WAYLAND_DISPLAY" ]]; then
      export WAYLAND_DISPLAY="${socket##*/}"
      break
    fi
  done
  export BORK_COMPOSITOR_PID="$child_pid"
  "$app" doctor
  if [[ "${BORK_EXTERNAL_PROOF:-}" == 1 ]]; then
    printf '%s\n' "$WAYLAND_DISPLAY" >"$XDG_RUNTIME_DIR/test-display"
    printf '%s\n' "$HYPRLAND_INSTANCE_SIGNATURE" >"$XDG_RUNTIME_DIR/test-signature"
    touch "$XDG_RUNTIME_DIR/test-ready"
    for _ in {1..900}; do [[ -f "$XDG_RUNTIME_DIR/test-status" ]] && break; sleep .1; done
    [[ -f "$XDG_RUNTIME_DIR/test-status" ]] || die "External proof did not finish"
    [[ "$(cat "$XDG_RUNTIME_DIR/test-status")" == 0 ]] || die "External proof failed"
  else
    BORK_ISOLATED=1 timeout 45 "$proof" --ignored --nocapture --test-threads=1
  fi
  if [[ "${BORK_CLEAN_PREVIEW:-}" == 1 ]]; then
    # No development-shell graphics paths: exercise the executable's discovery
    # against a real Wayland display. /run/current-system is hidden here, so make
    # the installed compositor package available through PATH instead.
    env -u LD_LIBRARY_PATH -u _BORK_RUNTIME_READY \
      -u BORK_LIBRARY_PATH -u BORK_HYPRLAND_BINARY -u BORK_NIX_DISCOVERY \
      PATH="$(dirname "$hyprland"):$PATH" BORK_TEST_CAPTURE="$XDG_RUNTIME_DIR/standalone-preview.png" \
      timeout 20 "$app" preview >"$XDG_RUNTIME_DIR/standalone-preview.log" 2>&1
    [[ -s "$XDG_RUNTIME_DIR/standalone-preview.png" ]] || die "Standalone Wayland preview did not capture a frame"
  fi
  kill -KILL "$child_pid" "$parent_pid"
  wait "$child_pid" "$parent_pid" 2>/dev/null || true
}

function main() {
  if [[ "${1:-}" == --inside ]]; then shift; inside "$@"; return; fi
  [[ $# == 0 ]] || { usage; exit 1; }
  command -v bwrap >/dev/null || die "bwrap is required"
  [[ -c /dev/dri/renderD128 ]] || die "A render node is required"
  local root runtime sway hyprland shell coreutils sandbox aquamarine proof grim app output loader library_path entry
  local -a loader_paths
  root="$(pwd)"
  app="${BORK_TEST_BINARY:-$root/target/debug/bork}"
  app="$(realpath "$app")"
  [[ -x "$app" ]] || die "Run cargo build first, or set BORK_TEST_BINARY"
  output="${BORK_EVIDENCE_DIR:-$root/.agents/evidence}"
  proof="$(mise exec -- cargo test --test display --no-run --message-format=json | sed -n '/"name":"display"/s/.*"executable":"\([^"]*\)".*/\1/p')"
  [[ -x "$proof" ]] || die "Display test executable missing"
  grim="$(readlink -f "$(command -v grim)")"
  runtime="$(mktemp -d /tmp/sl.XXXX)"
  mkdir -p "$runtime/home/.cache"
  if [[ -n "${BORK_RUNTIME_FILE:-}" ]]; then printf '%s\n' "$runtime" >"$BORK_RUNTIME_FILE"; fi
  sway="$(nix build nixpkgs#sway-unwrapped --no-link --print-out-paths)/bin/sway"
  aquamarine="$(nix build path:./nix#nested-aquamarine --no-link --print-out-paths)"
  echo "Test-only Aquamarine version-negotiation fix: $aquamarine"
  hyprland="$(readlink -f "$(command -v Hyprland)")"
  # /run/wrappers is hidden; use the unprivileged compositor executable instead.
  [[ "$hyprland" != /run/wrappers/* ]] || hyprland="$(readlink -f /run/current-system/sw/bin/Hyprland)"
  shell="$(readlink -f /proc/$$/exe)"
  coreutils="$(dirname "$(readlink -f "$(command -v timeout)")")"
  sandbox="$(command -v bwrap)"
  cat >"$runtime/sway.conf" <<'EOF'
output * resolution 1280x720
seat seat0 fallback true
EOF
  cat >"$runtime/hyprland.conf" <<'EOF'
monitor = ,1280x720@30,0x0,1
misc:disable_hyprland_logo = true
misc:disable_splash_rendering = true
misc:enable_anr_dialog = false
animations:enabled = false
debug:disable_logs = false
debug:enable_stdout_logs = true
EOF
  cat >"$runtime/hyprland.lua" <<'EOF'
hl.config({misc={disable_hyprland_logo=true,disable_splash_rendering=true,enable_anr_dialog=false},animations={enabled=false},debug={disable_logs=false,enable_stdout_logs=true}})
hl.monitor({output="",mode="1280x720@30",position="0x0",scale="1"})
EOF
  # /run is hidden below. Preserve existing loader support by canonicalizing
  # only this sandbox's inherited paths, without touching host configuration.
  loader="${NIX_LD:-}"
  [[ -z "$loader" ]] || loader="$(readlink -f "$loader")"
  library_path=""
  IFS=: read -r -a loader_paths <<<"${NIX_LD_LIBRARY_PATH:-}"
  for entry in "${loader_paths[@]}"; do
    [[ -n "$entry" ]] || continue
    entry="$(realpath -m "$entry")"
    library_path="${library_path:+$library_path:}$entry"
  done
  local status=0
  env -u WAYLAND_DISPLAY -u DISPLAY -u DBUS_SESSION_BUS_ADDRESS -u HYPRLAND_INSTANCE_SIGNATURE \
    PATH="$coreutils" TMPDIR="$runtime" XDG_RUNTIME_DIR="$runtime" HOME="$runtime/home" XDG_CACHE_HOME="$runtime/home/.cache" \
    NIX_LD="$loader" NIX_LD_LIBRARY_PATH="$library_path" \
    HYPRLAND_NO_SD_NOTIFY=1 AQ_DRM_DEVICES='' LD_LIBRARY_PATH="$aquamarine/lib:${LD_LIBRARY_PATH:-}" BORK_GRIM="$grim" BORK_TEST_BINARY="$app" \
    "$sandbox" --ro-bind / / --tmpfs /run --ro-bind "$(readlink -f /run/opengl-driver)" /run/opengl-driver \
    --dev /dev --proc /proc --dev-bind /dev/dri/renderD128 /dev/dri/renderD128 \
    --bind "$runtime" "$runtime" --unshare-pid --as-pid-1 --die-with-parent --unshare-net -- \
    "$shell" "$root/tests/nested-session.sh" --inside "$sway" "$hyprland" "$app" "$proof" || status=$?
  mkdir -p "$output"
  cp "$runtime/"*.log "$output/"
  for proof in "$runtime/"*.png "$runtime/"*.json; do [[ ! -f "$proof" ]] || cp "$proof" "$output/"; done
  echo "Isolated session files: $runtime"
  return "$status"
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then main "$@"; fi
