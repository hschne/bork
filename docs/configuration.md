# Configuration

[Back to README](../README.md)

The preview and CLI share `$XDG_CONFIG_HOME/bork/settings.json`, normally `~/.config/bork/settings.json`. There are no per-project settings or local/global scopes. Missing settings use defaults.

The running timer checks for saved changes once per second without resetting elapsed work or observed inactivity. **Shortening a duration can immediately advance a phase or complete a break.**

## Timing and intensity

Use the preview’s **Timing** controls for durations in minutes, and the draggable curve for interference intensity over grace. Choose **Save settings** to apply them. The CLI and JSON file use seconds:

```sh
bork config set timing.work_seconds 1200
bork config set timing.grace_seconds 180
bork config set timing.break_seconds 300
```

| Key                    | Default             | Accepted values                                            |
| ---------------------- | ------------------- | ---------------------------------------------------------- |
| `timing.work_seconds`  | `1200` (20 minutes) | Whole seconds, 1–4294967295.                               |
| `timing.grace_seconds` | `180` (3 minutes)   | Whole seconds, 0–4294967295. Zero skips interference.      |
| `timing.break_seconds` | `300` (5 minutes)   | Whole seconds, 1–4294967295.                               |
| `preview_image`        | `null`              | Local image path string, or `null` for the bundled sample. |

## Intensity curve

`curve` is an array of four points. `progress` is the fraction of grace elapsed (0–1); `intensity` is 0–100. Defaults are:

```json
[
  {"progress": 0.0, "intensity": 35.0},
  {"progress": 0.33333334, "intensity": 50.0},
  {"progress": 0.6666667, "intensity": 72.0},
  {"progress": 1.0, "intensity": 100.0}
]
```

The first and last progress values must be 0 and 1; the last intensity must be 100. Points must be at least 0.01 apart in progress and cannot decrease in intensity. Shape-preserving cubic interpolation joins the points without overshoot. Changing intensity does not change the break deadline or individual effect thresholds.

```sh
bork config get curve
bork config unset curve  # Restore the default points
```

The old scalar exponent and `strength` setting are replaced by this curve. In an existing file, replace the numeric `curve` with the array above and remove `strength`. A missing `curve` uses the default points.

## Configuration commands

```sh
bork config list                        # Sorted dotted-key=value lines, including defaults
bork config list --json                 # One JSON object using the same dotted keys
bork config get timing.break_seconds    # One JSON scalar; 300 by default
bork config unset timing.break_seconds  # Restore only this key’s default
bork config path                        # Print the settings file location
```

Reading configuration does not require a running timer, compositor, or graphics libraries, and does not create a settings file.

`set` accepts a JSON value. Numbers and booleans must not be JSON strings. For an image path, retain the JSON double quotes inside shell quotes:

```sh
bork config set preview_image '"/home/YOU/Pictures/desktop.png"'
bork config set preview_image null
```

`set` and `unset` are silent on success. They validate the complete settings before atomically saving, preserve other effective values, and leave the file untouched on failure. Unknown keys, object-valued groups, and invalid values produce an error and nonzero exit status. `unset` writes the default value rather than deleting the key.

Configuration commands do not activate effects themselves. Saved changes can affect the running timer’s phase and display through its normal reload.

## Individual effects

Individual effect tuning is available through the CLI or settings file, not the preview controls.

Each of `tears`, `colour`, `bands`, `noise`, and `ghost` has:

| Suffix    | Accepted values   | Meaning                                       |
| --------- | ----------------- | --------------------------------------------- |
| `enabled` | `true` or `false` | Enable or disable this effect.                |
| `entry`   | Number from 0–99  | Glitchiness level at which the effect starts. |
| `maximum` | Number from 0–1   | Maximum effect strength.                      |

Use `bork config list` to inspect current values and defaults.

```sh
bork config set tears.enabled false
bork config set colour.maximum 0.5
```

Interference bursts have irregular gaps and lengths, with independent timing for each effect and monitor. Each run gets a fresh random seed, retained while tuning and recompiling the shader. Glitchiness controls average burst frequency and severity, not a repeating loop.
