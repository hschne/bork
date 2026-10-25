# Bar and lockscreen integrations

[Back to README](../README.md)

## Timer status

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

For a display-ready break label, use `bork status --format break`. It prints `MM:SS LEFT` from `break_remaining_seconds` (minutes can exceed 99), or `BREAK COMPLETE`, `BORK PAUSED`, or `BORK STOPPED`. If the timer cannot be queried or its reply is invalid, it prints `BORK UNAVAILABLE`, reports the error on stderr, and exits nonzero. Default `bork status` and explicit `--format json` retain the JSON contract. Both formats bypass graphics initialization.

Pause clears effects and preserves work debt. Paused time does not advance work or complete a break; resume starts fresh idle observation. Repeating pause or resume does not change debt. Reset always starts a fresh running work interval, even from pause or overtime, and cancels desktop trials.

If the timer is absent or does not reply within two seconds, reset falls back to independent guardian shutdown and journal recovery. That fallback reports `stopped`, never launches a daemon, and still reports restoration conflicts as errors. Use Ctrl-C or SIGTERM to shut down a healthy background process completely.

## Lockscreen appearance

Bork does not draw over a security lockscreen. Your locker owns authentication and layout; Bork continues tracking input-free break time while locked. A voluntary lock can complete a break at any point in the cycle. Unlocking early preserves debt; typing or moving the mouse while locked interrupts an incomplete break.

The optional default background is [`assets/intermission.png`](../assets/intermission.png), rendered from Bork's test-card artwork with an empty countdown area. Copy it from this checkout to a stable location:

```sh
install -Dm644 assets/intermission.png ~/.local/share/bork/intermission.png
```

### Hyprlock example

This is documentation, not an installed integration. Keep your existing authentication settings and password input field. In `~/.config/hypr/hyprlock.conf`, replace your screenshot background with the following and add the label. Make sure `bork` is on Hyprlock's PATH, or use the executable's absolute path in the label command. No `jq` or status-formatting script is needed. Use an absolute image path, replacing `/home/YOU` below:

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
    text = cmd[update:1000] bork status --format break
    color = rgba(d9e8b2ff)
    font_family = monospace
    font_size = 24
    position = 0, -10%
    halign = center
    valign = center
}
```

The command polls once per second; Bork supplies the countdown and state messages described above. It uses `break_remaining_seconds`, **not** `remaining_seconds` (which counts toward the test card during work/grace). None of these messages disables authentication or prevents early unlocking.

Choose any PNG/JPEG/WebP by changing `background.path`. Countdown placement is entirely controlled by the label's `position`, `halign`, and `valign`; font and colour are configurable there too. Empty `monitor` applies to every output. Percentage offsets keep the label near the default image's empty strip on 16:9 displays; different aspect ratios may crop the background, so adjust positions per monitor as needed. Move your existing password field below the panel (for example, `position = 0, -29%` with centre alignment), and move or remove other labels that overlap it. Bork does not read or modify your lockscreen configuration.
