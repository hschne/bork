<div align="center">

# Bork

A visual break reminder for [Hyprland](https://hypr.land/).

[![CI](https://github.com/hschne/bork/actions/workflows/check.yml/badge.svg)](https://github.com/hschne/bork/actions/workflows/check.yml)
[![License](https://img.shields.io/github/license/hschne/bork)](LICENSE)

</div>

Work for 20 minutes. Your desktop starts losing its signal. Three minutes later, a **TAKE A BREAK** test card covers every monitor. Five minutes without keyboard or mouse input clears it.

**Demo contains flashing and distortion.** Timings are accelerated.

[Watch the demo (MP4, 25 seconds, 9.5 MB)](https://github.com/hschne/hschne/releases/download/bork-demo/bork-readme-720p.mp4)

## Install

Download a build from [GitHub Actions](https://github.com/hschne/bork/actions/workflows/check.yml), extract the archive, and put `bork` on your `PATH`, keeping `assets/` beside it.

Requires **Linux, Hyprland, glibc 2.35+, and working Wayland/OpenGL drivers**. On NixOS, the downloaded binary also needs a compatible loader, such as nix-ld. Run `bork doctor` to check your session.

## Use

```sh
bork preview    # Preview effects and configure timing
bork            # Start the reminder
bork pause      # Clear effects and pause the timer
bork resume     # Continue
bork reset      # Clear effects and start a fresh interval; also recover after a crash
```

Before starting, bind `bork reset` to a Hyprland shortcut you can reach while the break card is visible. The card consumes normal input but leaves compositor shortcuts available. It is **not a lockscreen**.

Bork stays attached to the terminal; Ctrl-C stops it. It does not install autostart entries or change your Hyprland config.

A full input-free break counts at any point. Locking hides the effects; input-free lock and suspend time count toward the break.

## Configuration

Run `bork preview` to change work, grace, and break durations or drag the interference curve. **Save settings** applies changes to the running timer. **Live Preview** tries the effect on your desktop for five seconds after confirmation.

Settings live in `~/.config/bork/settings.json` (or `$XDG_CONFIG_HOME/bork/settings.json`). For CLI configuration:

```sh
bork config list
bork config set timing.work_seconds 1500
bork config set timing.break_seconds 300
bork config unset timing.work_seconds
```

### Overtime card

The background is the shipped image `assets/overtime.png`. Keep `assets/` beside the executable when installing Bork. Bork reads this file directly; it does not generate or install card images or bundle card fonts. Text overlays and the live countdown use installed system fonts, falling back to an installed sans-serif family when the requested family is unavailable.

```json
{
  "overtime": {
    "background": { "image": "/home/YOU/Pictures/break.png", "fit": "cover" },
    "font_family": "SauceCodePro NF",
    "color": "#eeeeee",
    "halign": "center",
    "valign": "center",
    "labels": [
      {
        "text": "TRANSMISSION PAUSED",
        "font_size": 18,
        "position": ["0%", "13.5%"]
      },
      { "text": "TAKE A BREAK", "font_size": 62, "position": ["0%", "4.5%"] },
      {
        "text": "{remaining} / {total}",
        "font_size": 34,
        "position": ["0%", "-5%"]
      }
    ]
  }
}
```

Omit any setting to use its default; `null` is not an override. Labels inherit the outer font, color, and alignment; each can override them. A supplied `labels` array replaces the default list. Positions accept pixels or percentages, with positive Y upward from the alignment anchor. Font sizes and pixel offsets scale from a 720-pixel-high reference. Colors accept `#RRGGBB` or `#RRGGBBAA`; image fitting supports `cover`, `contain`, and `stretch`. Countdown placeholders are built-in, not shell commands.

Use `bork config list` for effective defaults. Restart the preview after editing card settings; the running timer reloads saved settings normally. Preview and actual overtime share the same renderer. No card editor is needed.

For bar integrations, `bork status` returns JSON; `bork status --format break` prints a break countdown. Use `bork --help` for commands.

## Development

Rust code lives in `src/`; integration and isolated display tests live in `tests/`. Run `mise run check` before submitting changes and `mise run release` for distribution changes (requires Docker). On NixOS, use `nix develop path:./nix` for graphics development.

Contributions welcome. Open an issue or pull request on [GitHub](https://github.com/hschne/bork). Keep changes small and focused. Test display changes in isolated sessions, not on your active desktop.

## License

[MIT](LICENSE) © Hans Schnedlitz
