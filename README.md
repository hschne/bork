# Signal Loss

A visual break reminder for **Hyprland on Linux**. Twenty minutes of work, three minutes of accelerating signal interference, then a steady **TAKE A BREAK** test card. Five continuous minutes without keyboard **or mouse** input restores the desktop.

The desktop effect uses Hyprland's screen shader—not screen capture. The editor renders the same effect shader over an embedded sample desktop, with clean controls beside it.

Bursts have irregular gaps and varying lengths, with independent timing for each effect and monitor—even identical monitors. Each run gets a fresh random seed, retained during tuning and shader recompilation. Glitchiness controls average frequency and severity, not a repeating loop.

## Run

Copy the executable anywhere on your PATH; no installer, Python, Lua interpreter, shader manager, plugin, or `hyprctl` is needed.

```sh
signal-loss doctor       # Non-mutating compositor/protocol capability check
signal-loss              # Background operation; no control window
signal-loss preview      # Tune, save, preview test card, or explicitly try the desktop for 5 s
signal-loss stop         # Emergency stop, normal stop, or retry crash recovery
```

Normal operation stays attached to the launching terminal. Start it through your existing session launcher/service if desired. Signal Loss does not install autostart entries or change compositor configuration.

The editor's slider accepts an exact numeric value from 0 to 100. Selecting 100 does **not** trigger the test card. Each effect has an independent toggle, entry level, and maximum strength. Settings are saved to `$XDG_CONFIG_HOME/signal-loss/settings.json` (normally `~/.config/signal-loss/settings.json`). Background operation reloads saved changes within one second without resetting work debt.

A desktop trial requires confirmation, lasts five seconds, and restores the current background state. It also works while the background timer is running: the display guardian temporarily overrides the effect, while the controller continues counting normally. Closing the editor ends its trial; editor failure cannot extend it. Trial deadlines use boot time, so suspension does not extend a trial; locking cancels it.

## Host requirements

Verified application architecture/artifact: **x86-64 Linux**. Verified compositor: **Hyprland 0.56.2**, both legacy and Lua configuration. Other architectures and compositor versions are not yet verified. GNOME, KDE, Windows, and macOS desktop effects are not supported. The editor does not require Hyprland; its X11 mode was tested in Debian and its Wayland mode in an isolated NixOS session.

Required compositor capabilities:

- Direct Hyprland IPC; `decoration:screen_shader`, `debug:damage_tracking`, `debug:vfr`, and lock-status query.
- Wayland `ext_idle_notifier_v1` **version 2** input-idle notifications, ignoring media idle inhibitors.
- Layer-shell, shared-memory buffers, outputs, and an input seat.
- GLSL ES 300 screen shaders; editor requires OpenGL 3+/GLES 3+ (a working Mesa software renderer is also usable).

The deployable executable is **dynamically linked, not static**. It uses the standard `/lib64/ld-linux-x86-64.so.2` loader, glibc **2.35 or newer**, and `libgcc_s`. Graphics libraries/drivers are host dependencies loaded at runtime, including Wayland client/cursor, libxkbcommon, EGL/OpenGL; X11 preview additionally uses X11/Xcursor/Xi/Xrandr and **libxkbcommon-x11**. Their transitive dependencies depend on your driver.

For example, the Debian verification environment uses:

```sh
apt-get install libwayland-client0 libwayland-cursor0 libxkbcommon0 \
  libxkbcommon-x11-0 libegl1 libgl1 libgl1-mesa-dri \
  libx11-6 libxcursor1 libxi6 libxrandr2
```

These are example host packages, **not commands run by the application**. On NixOS, an existing compatible ELF loader (for example, an existing nix-ld setup) is still required. Signal Loss automatically discovers installed graphics libraries from the system Hyprland package's ELF RUNPATHs, falling back to Hyprland on PATH, and includes `/run/opengl-driver/lib`. It restarts itself once, before threads or display changes, with those library paths. No Nix CLI, store-wide scan, development shell, or persistent environment change is needed at runtime.

### Runtime library configuration

| Environment variable          | Purpose                                                                                                                          |
| ----------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `SIGNAL_LOSS_LIBRARY_PATH`    | Colon-separated, existing absolute library directories. These precede inherited `LD_LIBRARY_PATH` and automatic discovery.       |
| `SIGNAL_LOSS_NIX_DISCOVERY`   | `auto` (default: enabled on NixOS), `0` to disable, or `1` to force discovery.                                                   |
| `SIGNAL_LOSS_HYPRLAND_BINARY` | Absolute compositor executable to inspect instead of the system/PATH package. Standard Nix wrappers are unwrapped automatically. |

For custom installations, or libraries absent from Hyprland's closure (including some X11-only dependencies):

```sh
SIGNAL_LOSS_LIBRARY_PATH=/absolute/graphics/lib:/absolute/other/lib signal-loss preview
SIGNAL_LOSS_HYPRLAND_BINARY=/absolute/path/to/Hyprland signal-loss doctor
```

`doctor` prints the effective library search paths. `stop` bypasses graphics discovery and validation so a broken graphics override cannot prevent emergency recovery. Standard display/session variables (`WAYLAND_DISPLAY`, `DISPLAY`, `XDG_RUNTIME_DIR`, `HYPRLAND_INSTANCE_SIGNATURE`) retain their normal meanings. No system configuration is installed or changed; Nix remains optional development/test tooling, not the deployment format.

## Safety and recovery

Interference is intentionally disruptive. The test card consumes ordinary input over every output; it is **not a security lock screen**. Compositor shortcuts remain available. Before ordinary use, arrange a way to run `signal-loss stop`, such as a compositor shortcut or another terminal/TTY.

For a legacy Hyprland configuration, an optional user-managed shortcut is:

```ini
bind = SUPER SHIFT, Escape, exec, /absolute/path/to/signal-loss stop
```

Signal Loss never adds that binding itself. Do not use another shader manager to change the same settings during a trial or grace period.

Recovery safeguards:

- A separate guardian invocation of the **same executable** owns all display changes. Main-process crash, closed pipe, or missed two-second heartbeat restores state and releases overlays independently of normal exit handlers.
- Five-second trial expiry is enforced by the guardian, not GUI repainting.
- Exact previous shader path, damage tracking, and VFR values are journaled **before** mutation. Rendering overrides are enabled only during interference and removed for normal work/test cards. An existing user shader is restored, not replaced with a guessed default.
- The guardian independently removes its effects/overlays when the session locks. The controller keeps its break debt.
- Runtime files live in a private, instance-specific `$XDG_RUNTIME_DIR/signal-loss-<hash>/` directory. Required shaders, sample artwork, and fonts are embedded; only the temporary shader must be extracted for Hyprland.
- If both the application and guardian are killed, run `signal-loss stop` in the same Hyprland session to recover from the retained journal. If the compositor is disconnected, retry after it is reachable.
- A setting changed externally is left untouched. Recovery reports a conflict and keeps `journal.json` for inspection rather than overwriting the newer value. Resolve that conflict deliberately; do not delete the journal while Signal Loss still owns an active shader.

Like any IPC-based tool, it cannot restore an unresponsive or dead compositor. If Hyprland itself is restarted, its new session/configuration owns the display; do not apply an old session's journal to a different compositor instance.

## Timing rules

- Short pauses do not pause or erase work debt. Interference continues until a complete break or a security lock.
- Inactivity already accumulated during grace counts toward the same five minutes on the test card.
- After completing a break, staying idle cannot start another reminder. First resumed input starts the next 20-minute interval.
- Input-idle is observed conservatively from application startup; standard Wayland does not expose pre-launch idle history. Observation has one-second granularity and never finishes a break early due to timestamp rounding. Input-seat topology changes conservatively restart observation rather than backdating inactivity.
- Suspend counts as input-free time. Short suspend does not erase debt. Suspend is distinguished from an application stall using boot-time versus monotonic clocks.
- Locking does not reset the cycle. Input-free lock time counts as a break; short lock periods retain debt. Effects never intentionally cover or distort the security lock screen.

## Build a deployable executable

With Docker/BuildKit:

```sh
docker build --target artifact --output type=local,dest=dist .
./dist/signal-loss --version
```

`dist/signal-loss` is the application. The other exported text files are dependency inspection evidence, not required runtime files. The build pins Rust 1.93.1's Debian image by digest and uses `Cargo.lock`. It does not publish an image or release, install global tools, or modify the host configuration.

For native development with Rust 1.93+ and a C linker:

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

`mise run check` runs formatting, Clippy, and ordinary tests using controlled time and fake compositor sockets; it never activates desktop effects. CI also exercises editor controls and save/reopen under Xvfb/software GL.

Optional full display checks:

```sh
# Installed Hyprland inside headless Sway:
nix develop path:./nix --command bash tests/nested-session.sh
# Same display scenario from Debian userspace:
nix develop path:./nix --command bash tests/debian-session.sh
```

The runners hide active display/session sockets and input/DRM display devices, exposing only a render node. A test-only Aquamarine protocol-negotiation fix stays inside the sandbox; it is not shipped or installed. `tests/preview-smoke.sh` provides the standalone Xvfb editor check used by CI.

Physical hotplug, hardware suspend/resume, other drivers, and battery-life attribution remain manual checks. Nested-session GPU/power samples are not application wattage or battery estimates.
