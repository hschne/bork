# Preview

[Back to README](../README.md)

```sh
bork preview
```

The preview renders the same effect shader as the desktop over a bundled sample image or your own screenshot. It can run independently of the background timer on Wayland or X11, with OpenGL 3+/GLES 3+.

## Playback

Play/pause and the timeline compress a clean desktop, the grace curve, and the break card into a 15-second demonstration. Playback stops on the card; playing again restarts it. Scrubbing pauses progression, not shader animation. Only the cycle clock is accelerated.

With zero-length grace, playback goes straight from the clean desktop to the card.

## Settings

**Timing** sets work, grace, and break durations in minutes. The curve under **Interference** sets intensity over grace: drag the starting point vertically, or either middle point in time and intensity. The endpoint stays at 100%. A smooth, non-decreasing curve joins the points without overshoot. The playback marker shows the current position.

Focus a point and use arrow keys to fine-tune it; Shift makes larger adjustments. **Reset curve** restores the default. With the default three-minute grace, intensity starts at 35%, reaches 50% after one minute and 72% after two, then ends at 100%. Zero-length grace disables curve editing.

**Save settings** applies timing and the curve to the shared settings file. A running timer picks up saved changes within one second without resetting elapsed work. Individual effect toggles, entry levels, and maxima remain available through the [configuration CLI](configuration.md); there is no separate strength multiplier.

## Custom images

Use the image menu at the bottom-left to choose a PNG, JPEG, or WebP screenshot. The path is remembered immediately, without saving other unfinished edits. **Reset to sample** appears when a custom image is selected.

Images stay local. Bork does not capture your desktop. Missing or invalid images fall back to the bundled sample with an error. The native file chooser uses the desktop’s XDG portal, or a Zenity fallback.

Files are limited to 64 MiB and 8192 pixels per side, with a 128 MiB decoder allocation budget. Large images are reduced to at most 4096 pixels per side.

## Live Preview

**Live Preview changes the actual desktop.** It pauses playback and asks for confirmation before applying the displayed interference or break card for five seconds. Arrange access to `bork reset` first; see [safety and recovery](recovery.md).

Trials also work while the background timer runs. The display guardian temporarily overrides the effect while the timer continues counting, then restores its latest display state.

Closing the editor ends its trial. The guardian enforces expiry independently of GUI repainting, so an editor failure cannot extend it. Suspension does not extend the deadline, and locking cancels the trial.
