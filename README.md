# Bork

A visual break reminder for **Hyprland on Linux**. Work for twenty minutes, then watch your desktop turn into a failing TV signal. After three minutes of interference, a steady **TAKE A BREAK** test card covers every monitor. Five continuous minutes without keyboard or mouse input clears the reminder.

Bork distorts the desktop using Hyprland’s screen shader, not screen capture. It runs locally as a single executable, with no plugin, account, or telemetry.

Preview demo: a clean desktop, escalating interference, then the break card, compressed into 15 seconds of playback. **Contains flashing and distortion.**

https://github.com/user-attachments/assets/7f2614c8-0375-4e5c-b95f-a8365dcadbab

## Install

Bork is still being finalized; no release binaries are published yet. Build a deployable executable from a checkout with Docker/BuildKit:

```sh
git clone https://github.com/hschne/bork.git
cd bork
docker build --target artifact --output type=local,dest=dist .
./dist/bork --version
```

Copy `dist/bork` to a directory on your `PATH`. The build uses two Cargo jobs by default; lower this with `--build-arg BUILD_JOBS=1` on a machine with fewer than four CPUs. See [development](docs/development.md) for native builds and tests.

### Requirements

- **Hyprland on Linux** for desktop effects. `bork doctor` checks the session without changing display settings.
- **glibc 2.35+**, the standard x86-64 ELF loader, and `libgcc_s` for the deployable build. The executable is dynamically linked, not static.
- Host Wayland and EGL/OpenGL libraries and graphics drivers. The preview needs OpenGL 3+/GLES 3+ and can also run on X11.
- On **NixOS**, an existing compatible ELF loader, such as a working nix-ld setup. Bork discovers graphics libraries but does not provision the loader.

See [host packages and runtime library configuration](docs/runtime.md) for Debian/Ubuntu packages, NixOS discovery, and environment overrides.

## Before you start

**The interference is intentionally disruptive.** The test card consumes ordinary keyboard and pointer input; it is **not a security lock screen**. Compositor shortcuts remain available.

Arrange a way to run `bork reset` before starting the timer, such as a compositor shortcut using the executable’s absolute path or another terminal/TTY. Do not use another shader manager to change the same settings while Bork’s effects are active.

Bork temporarily changes compositor rendering settings and restores them afterward. It does not install autostart entries, edit compositor configuration, or manage your lockscreen. See [safety and recovery](docs/recovery.md) for crash handling and restoration conflicts.

## Quick start

```sh
bork doctor     # Check session capabilities without activating effects
bork preview    # Preview and tune effects in a window
bork            # Start the timer, without a control window
```

The timer stays attached to its launching terminal. Use Ctrl-C or SIGTERM to shut it down; use your existing session launcher or service if you want it to start automatically.

### Preview and settings

`bork preview` shows the effect over a bundled sample desktop or a local screenshot. Play a 15-second demonstration, or scrub the timeline to inspect the interference and break card. This does not affect the desktop.

Set **Timing**, drag the intensity curve points, then choose **Save settings**. The running timer reloads changes without restarting. **Live Preview** is separate: after confirmation, it applies the displayed effect to the actual desktop for five seconds.

Defaults are **20 minutes work / 3 minutes interference / 5 minutes break**. Set grace to zero to skip interference and go straight to the card. The intensity curve controls interference, not the break deadline.

See [preview controls](docs/preview.md) and the [configuration reference](docs/configuration.md).

### Timer controls

| Command                      | Action                                                                     |
| ---------------------------- | -------------------------------------------------------------------------- |
| `bork status`                | Print a JSON timer snapshot.                                               |
| `bork status --format break` | Print a break countdown or state label.                                    |
| `bork pause`                 | Clear effects and freeze the timer, preserving elapsed work.               |
| `bork resume`                | Continue the paused timer.                                                 |
| `bork reset`                 | Clear effects and start a fresh work interval; also recover after a crash. |
| `bork config list`           | Show effective settings, including defaults.                               |

Pausing does not count toward a break. Reset keeps a healthy timer running; it does not shut it down. If the timer is absent or unresponsive, reset attempts independent display recovery without starting a new timer.

### What counts as a break?

A complete input-free break clears accumulated work at any point—not only when the card appears. Short idle periods do not erase elapsed work. Inactivity during interference carries over onto the card, and media playback does not prevent a break.

Suspend and input-free lock time count too. Locking hides Bork’s effects but does not reset the timer; typing or moving the mouse while locked interrupts an incomplete break. After a complete break, the next work interval starts when input resumes, or when you unlock if the session is locked. Bork never locks or unlocks the session itself.

## Documentation

- [Configuration](docs/configuration.md): timing, effect tuning, and CLI settings.
- [Preview](docs/preview.md): playback, custom images, and five-second desktop trials.
- [Bar and lockscreen integrations](docs/integrations.md): JSON status and a Hyprlock example.
- [Host requirements](docs/runtime.md): runtime libraries, NixOS, and overrides.
- [Safety and recovery](docs/recovery.md): emergency reset, recovery journals, and timing details.
- [Development](docs/development.md): builds, tests, and code structure.

## Contributing

Open an [issue](https://github.com/hschne/bork/issues) or pull request. For a bug, include your Bork and Hyprland versions, relevant error output, and steps to reproduce. Review logs and screenshots for private information before sharing them.

Run `mise run check` before submitting code changes. Keep display tests isolated from the active desktop; see [development](docs/development.md).

## License

[MIT](LICENSE) © 2026 Hans Schnedlitz. Dependencies and embedded fonts retain their [third-party licenses](THIRD_PARTY_LICENSES.txt).
