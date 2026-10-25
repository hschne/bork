# Bork

A visual break reminder for **Hyprland on Linux**. Twenty minutes of work, three minutes of accelerating signal interference, then a steady **TAKE A BREAK** test card. Five continuous minutes without keyboard **or mouse** input restores the desktop.

The desktop effect uses Hyprland's screen shader—not screen capture. The preview renders the same effect shader over a crisp embedded sample desktop or your own screenshot, with a simple player and settings below it.

Bursts have irregular gaps and varying lengths, with independent timing for each effect and monitor—even identical monitors. Each run gets a fresh random seed, retained during tuning and shader recompilation. Glitchiness controls average frequency and severity, not a repeating loop.

## Run

Copy the executable anywhere on your PATH; no installer, Python, Lua interpreter, shader manager, plugin, or `hyprctl` is needed.

```sh
bork doctor       # Non-mutating compositor/protocol capability check
bork              # Background operation; no control window
bork preview      # Tune, save, preview test card, or explicitly try the desktop for 5 s
bork status       # One JSON timer snapshot, suitable for Waybar/lockscreen polling
bork config list  # Inspect effective settings, including defaults
bork pause        # Freeze work debt and clear effects
bork resume       # Continue with preserved debt
bork reset        # Fresh running interval; also the panic/recovery command
```

Normal operation stays attached to the launching terminal. Start it through your existing session launcher/service if desired. Bork does not install autostart entries or change compositor configuration.

Work, grace, and break durations are configurable in the preview’s **Timing** controls, displayed in minutes. Defaults are 1200 / 180 / 300 seconds (20 / 3 / 5 minutes). Work and break must be positive whole seconds; grace may be zero to go directly to the test card. The JSON `timing` object in the settings file uses:

```json
"timing": {"work_seconds": 1200, "grace_seconds": 180, "break_seconds": 300}
```

Omitted durations use their defaults. Values must fit unsigned 32-bit seconds. Saved duration changes reload without resetting work debt or observed inactivity; shortening a duration can therefore make a phase or break complete immediately.

The preview’s play/pause button and timeline compress a clean desktop, the complete grace curve, and the overtime test card into 15 seconds. Playback stops on the card; playing again restarts. Scrubbing pauses progression, not shader animation. Only the cycle clock is accelerated. A zero-length grace goes straight from the clean desktop to the card.

**Strength** controls overall interference without changing individual effect tuning. **Save settings** applies timing and strength to `$XDG_CONFIG_HOME/bork/settings.json` (normally `~/.config/bork/settings.json`). Background operation reloads saved changes within one second without resetting work debt. Effect toggles, entry levels, maxima, and the grace curve remain available through the configuration file and CLI rather than the preview UI.

Use the image menu at the bottom-left of the preview to choose a PNG, JPEG, or WebP screenshot. The path is remembered immediately without saving other unfinished edits; **Reset to sample** appears only for a custom image. Images stay local and never become desktop capture. Missing or invalid images fall back to the bundled sample with an error. The native file chooser uses the desktop’s XDG portal (or Zenity fallback). Decoding is limited to 64 MiB files, 8192 pixels per side and a 128 MiB decoder allocation budget; large images are reduced to at most 4096 pixels per side.

**Live Preview** pauses playback, asks for confirmation, applies the displayed interference or break card for five seconds, and restores the current background state. It also works while the background timer is running: the display guardian temporarily overrides the effect, while the controller continues counting normally. Closing the editor ends its trial; editor failure cannot extend it. Trial deadlines use boot time, so suspension does not extend a trial; locking cancels it.

## Configuration CLI

The editor and CLI share `$XDG_CONFIG_HOME/bork/settings.json` (normally `~/.config/bork/settings.json`). There are no local/global scopes. Reading configuration does not require a running timer, compositor, or graphics libraries, and does not create a settings file.

```sh
bork config list                         # Sorted dotted-key=value lines, including defaults
bork config list --json                  # One JSON object with the same dotted keys
bork config get timing.break_seconds     # Prints 300 by default
bork config set timing.break_seconds 240
bork config set timing.grace_seconds 0
bork config set tears.enabled false
bork config set colour.maximum 0.5
bork config set curve 2.5
bork config unset timing.break_seconds   # Restore only this key's default
bork config path                         # Print the settings file location
```

Keys include `strength` (0–1, default 1), `preview_image` (an image path string or `null` for the sample), `timing.work_seconds`, `timing.grace_seconds`, `timing.break_seconds`, `curve`, and `enabled`, `entry`, `maximum` under each of `tears`, `colour`, `bands`, `noise`, and `ghost`. Effect and timing values are JSON numbers or booleans, not strings. Curve accepts 1.01–5, effect entry 0–99, and maximum 0–1. Timing uses the whole-second ranges described above.

`get` prints one JSON scalar. `set` and `unset` are silent on success; they validate the complete settings before atomically saving, preserve other effective values, and leave the file untouched on failure. Unknown keys, object-valued groups, and invalid values are errors with a nonzero exit status. `unset` saves the default value rather than deleting the key. The running timer reloads changes within one second without resetting debt. These commands do not activate effects themselves; changing settings can change the running timer's phase or display intent through its normal reload.

## Timer control and Waybar

Poll `bork status` (for example, once per second) from a Waybar plugin. It prints one JSON object and exits:

```json
{
  "state": "running",
  "phase": "work",
  "work_seconds": 42,
  "remaining_seconds": 1338,
  "break_remaining_seconds": 300,
  "resting_seconds": 0,
  "glitchiness": 0.0
}
```

- `state`: `running`, `paused`, `resting` (break completed, waiting for input or unlock), or `stopped` (no background timer).
- `phase`: `work`, `grace`, or `overtime` (test card). It is `null` when stopped.
- `work_seconds`: accumulated work debt, including grace/overtime, excluding manual pause time.
- `remaining_seconds`: seconds until the test card during both work and grace (starts at work + grace), or until break completion in overtime. Frozen while paused.
- `break_remaining_seconds`: continuous inactivity still needed to complete a break.
- `resting_seconds`: elapsed time since break completion, excluding manual pause time; zero before a completed break or after returning. This is a count-up while resting.
- `glitchiness`: grace-curve intensity from 0–100; zero while paused. This is timer intent, not a query of lock or editor-trial display state.

All numeric countdown/debt fields are `null` when stopped. `pause`, `resume`, and `reset` also print the resulting status, after display changes have been applied. Errors go to stderr with a nonzero exit status; `status` with no timer is a successful `stopped` snapshot.

Pause clears effects and preserves work debt. Paused time does not advance work or complete a break; resume starts fresh idle observation. Repeating pause or resume does not change debt. Reset always starts a fresh running work interval, even from pause or overtime, and cancels desktop trials.

Reset replaces the former `stop` command. If the timer is absent or does not reply within two seconds, reset falls back to independent guardian shutdown and journal recovery. That fallback reports `stopped`, never launches a daemon, and still reports restoration conflicts as errors. Use Ctrl-C or SIGTERM to shut down a healthy background process completely.

## Lockscreen appearance

Bork does not draw over a security lockscreen. Your locker owns authentication and layout; Bork continues tracking input-free break time while locked. A voluntary lock can complete a break at any point in the cycle. Unlocking early preserves debt; typing or moving the mouse while locked interrupts an incomplete break.

The optional default background is [`assets/intermission.png`](assets/intermission.png), rendered from Bork's test-card artwork with an empty countdown area. Copy it from this checkout to a stable location:

```sh
install -Dm644 assets/intermission.png ~/.local/share/bork/intermission.png
```

### Hyprlock example

This is documentation, not an installed integration. Keep your existing authentication settings and password input field. In `~/.config/hypr/hyprlock.conf`, replace your screenshot background with the following and add the label. Install `jq` and make sure both `bork` and `jq` are on Hyprlock's PATH. Use an absolute image path, replacing `/home/YOU` below:

```ini
background {
    monitor =
    path = /home/YOU/.local/share/bork/intermission.png
    color = rgba(111718ff)
    blur_passes = 0
    noise = 0
    contrast = 1
    brightness = 1
    vibrancy = 0
}

label {
    monitor =
    text = cmd[update:1000] bork status 2>/dev/null | jq -ers 'def pad: tostring | if length < 2 then "0" + . else . end; .[0] | if .state == "resting" then "BREAK COMPLETE" elif .state == "paused" then "BORK PAUSED" elif .state == "stopped" then "BORK STOPPED" elif .state == "running" and (.break_remaining_seconds | type) == "number" then .break_remaining_seconds | "\(. / 60 | floor | pad):\(. % 60 | pad) LEFT" else error("Invalid status") end' 2>/dev/null || printf 'BORK UNAVAILABLE\n'
    color = rgba(d9e8b2ff)
    font_family = monospace
    font_size = 24
    position = 0, -10%
    halign = center
    valign = center
}
```

Keep the `text` command on a single line. It polls once per second and uses `break_remaining_seconds`, **not** `remaining_seconds` (which counts toward the test card during work/grace). It shows `BREAK COMPLETE` after sufficient inactivity, explicit paused/stopped states, and `BORK UNAVAILABLE` when status cannot be read. None of these messages disables authentication or prevents early unlocking.

Choose any PNG/JPEG/WebP by changing `background.path`. Countdown placement is entirely controlled by the label's `position`, `halign`, and `valign`; font and colour are configurable there too. Empty `monitor` applies to every output. Percentage offsets keep the label near the default image's empty strip on 16:9 displays; different aspect ratios may crop the background, so adjust positions per monitor as needed. Move your existing password field below the panel (for example, `position = 0, -29%` with centre alignment), and move or remove other labels that overlap it. Bork does not read or modify your lockscreen configuration.

## Host requirements

Desktop effects require Hyprland on Linux. Run `bork doctor` to check your session's capabilities. The editor runs independently on Wayland or X11 and requires OpenGL 3+/GLES 3+.

The deployable executable is **dynamically linked, not static**. It uses the standard `/lib64/ld-linux-x86-64.so.2` loader, glibc **2.35 or newer**, and `libgcc_s`. Graphics libraries/drivers are host dependencies loaded at runtime, including Wayland client/cursor, libxkbcommon, EGL/OpenGL; X11 preview additionally uses X11/Xcursor/Xi/Xrandr and **libxkbcommon-x11**. Their transitive dependencies depend on your driver.

Debian/Ubuntu graphics packages:

```sh
apt-get install libwayland-client0 libwayland-cursor0 libxkbcommon0 \
  libxkbcommon-x11-0 libegl1 libgl1 libgl1-mesa-dri \
  libx11-6 libxcursor1 libxi6 libxrandr2
```

These are example host packages, **not commands run by the application**. On NixOS, an existing compatible ELF loader (for example, an existing nix-ld setup) is still required. Bork automatically discovers installed graphics libraries from the system Hyprland package's ELF RUNPATHs, falling back to Hyprland on PATH, and includes `/run/opengl-driver/lib`. It restarts itself once, before threads or display changes, with those library paths. No Nix CLI, store-wide scan, development shell, or persistent environment change is needed at runtime.

### Runtime library configuration

| Environment variable   | Purpose                                                                                                                          |
| ---------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `BORK_LIBRARY_PATH`    | Colon-separated, existing absolute library directories. These precede inherited `LD_LIBRARY_PATH` and automatic discovery.       |
| `BORK_NIX_DISCOVERY`   | `auto` (default: enabled on NixOS), `0` to disable, or `1` to force discovery.                                                   |
| `BORK_HYPRLAND_BINARY` | Absolute compositor executable to inspect instead of the system/PATH package. Standard Nix wrappers are unwrapped automatically. |

For custom installations, or libraries absent from Hyprland's closure (including some X11-only dependencies):

```sh
BORK_LIBRARY_PATH=/absolute/graphics/lib:/absolute/other/lib bork preview
BORK_HYPRLAND_BINARY=/absolute/path/to/Hyprland bork doctor
```

`doctor` prints the effective library search paths. Timer commands bypass graphics discovery and validation so a broken graphics override cannot prevent emergency recovery. Standard display/session variables (`WAYLAND_DISPLAY`, `DISPLAY`, `XDG_RUNTIME_DIR`, `HYPRLAND_INSTANCE_SIGNATURE`) retain their normal meanings. No system configuration is installed or changed; Nix remains optional development/test tooling, not the deployment format.

## Safety and recovery

Interference is intentionally disruptive. The test card consumes ordinary input over every output; it is **not a security lock screen**. Compositor shortcuts remain available. Before ordinary use, arrange a way to run `bork reset`, such as a compositor shortcut or another terminal/TTY.

Configure a compositor shortcut to run `/absolute/path/to/bork reset` if desired. Bork does not add bindings itself. Do not use another shader manager to change the same settings during a trial or grace period.

Recovery safeguards:

- A separate guardian invocation of the **same executable** owns all display changes. Main-process crash, closed pipe, or missed two-second heartbeat restores state and releases overlays independently of normal exit handlers.
- Five-second trial expiry is enforced by the guardian, not GUI repainting.
- Exact previous shader path, damage tracking, and VFR values are journaled **before** mutation. Rendering overrides are enabled only during interference and removed for normal work/test cards. An existing user shader is restored, not replaced with a guessed default.
- The guardian independently removes its effects/overlays when the session locks. The controller keeps its break debt.
- Runtime files live in a private, instance-specific `$XDG_RUNTIME_DIR/bork-<hash>/` directory. Required shaders, sample artwork, and fonts are embedded; only the temporary shader must be extracted for Hyprland.
- If both the application and guardian are killed, run `bork reset` in the same Hyprland session to recover from the retained journal. If the compositor is disconnected, retry after it is reachable.
- A setting changed externally is left untouched. Recovery reports a conflict and keeps `journal.json` for inspection rather than overwriting the newer value. Resolve that conflict deliberately; do not delete the journal while Bork still owns an active shader.

Like any IPC-based tool, it cannot restore an unresponsive or dead compositor. If Hyprland itself is restarted, its new session/configuration owns the display; do not apply an old session's journal to a different compositor instance.

## Timing rules

- Short input-free pauses do not pause or erase work debt. Interference continues until a complete break or a security lock.
- Inactivity already accumulated during grace counts toward the same five minutes on the test card.
- A complete break clears debt at any point in the cycle, even during the work interval. After completing a break, staying idle cannot start another reminder. If unlocked, first resumed input starts the next work interval. If locked, waking the display or typing a password does not start work; successful unlock does.
- Input-idle is observed conservatively from application startup; standard Wayland does not expose pre-launch idle history. Observation has one-second granularity and never finishes a break early due to timestamp rounding. Input-seat topology changes conservatively restart observation rather than backdating inactivity.
- Suspend counts as input-free time. Short suspend does not erase debt. Suspend is distinguished from an application stall using boot-time versus monotonic clocks.
- Locking does not reset the cycle. Input-free lock time counts as a break; input while locked interrupts an incomplete break. Unlocking before a complete break retains debt. Effects never intentionally cover or distort the security lock screen. Bork never locks or unlocks the session; your existing locker and idle-lock policy remain responsible.

## Code structure

- `src/cycle.rs`: work debt, continuous breaks, and grace/deadline transitions; no I/O.
- `src/effect.rs`: validated glitchiness, grace curves, and effect tuning. `effect/shader.rs` adapts the shared GLSL.
- `src/control.rs`: JSON timer commands and status; its private server owns the background control socket.
- `src/background.rs`: clock/input observation and publication of display intent; no installation state.
- `src/recovery/`: the guardian owns display state, trial precedence, and lock suppression. Process transport, editor trial ownership, and write-ahead recovery are private modules.
- `src/wayland/`: input monitoring and software test cards have separate types and protocol state.
- `src/preview/`: controls, shader scene/GL resources, and screenshot verification are separate from the editor lifecycle.

Only the guardian mutates the desktop. Neither the break cycle nor the editor owns compositor installation or recovery policy.

## Build a deployable executable

With Docker/BuildKit:

```sh
docker build --target artifact --output type=local,dest=dist .
./dist/bork --version
```

`dist/bork` is the application; accompanying inspection files are optional.

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
