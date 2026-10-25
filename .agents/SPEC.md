# Bork

## Product

A playful but hard-to-ignore break reminder for Hyprland on Linux. It distorts the actual desktop rather than covering it with an ordinary notification.

- After the work interval, enter a grace period. Interference escalates nonlinearly, then a steady **TAKE A BREAK** test card covers every output. Work, grace, and break durations are configurable in settings and the editor; defaults are 20, three, and five minutes. Work and break are positive whole seconds; grace may be zero. Reloading durations preserves debt and observed inactivity.
- A full configured break duration without keyboard **or mouse** input completes a break at any point in the cycle. Reading and watching count; media idle inhibitors must not prevent a break.
- Inactivity accumulated before the test card counts toward the same break. Short pauses retain work debt. After a completed break, resumed input starts the next work interval if unlocked; while locked, input does not restart work, and successful unlock marks the return.
- Locking suppresses effects without resetting debt. Input-free suspend and lock time count toward a break; locked input interrupts an incomplete break. Early unlock retains debt. Session locking and unlocking remain the user's existing locker's responsibility. Observe startup and input-seat changes conservatively rather than inventing prior inactivity.
- Manual pause freezes work debt and clears effects; resume retains debt but starts fresh idle observation. Paused time does not advance work or complete a break.
- Reset starts a fresh running interval even from pause or overtime, clears effects, and cancels desktop trials. If the timer is absent or unresponsive, reset performs independent display recovery without launching a background process. It replaces emergency stop.
- Provide machine-readable timer status for bar integrations without initializing graphics; acknowledge timer commands only after applying display changes. The countdown spans work + grace continuously until the test card, then shows remaining break time. Report elapsed resting time from break completion, frozen during manual pause.
- Glitchiness is continuous from 0 to 100, separate from the deadline/test-card state. The user-edited intensity curve controls glitchiness independently of the grace deadline; there is no separate global strength multiplier.

## Effects and editor

- Desktop and editor use the same embedded GLSL effect implementation, with only rendering-interface adaptation.
- Glitchiness controls spatial strength, average burst frequency, and average duration separately. Burst starts and lengths are irregular; effects and monitors have independent streams, including identical monitors.
- Each application run gets fresh entropy. Keep its seed stable during tuning and shader recompilation; do not expose seed, sweep, or animation-frame-scrubbing controls. Cycle scrubbing changes progression only.
- The egui/glow preview uses neutral system-aware styling: a shader-rendered image, simple play/pause and cycle scrubber, then always-visible timing and a draggable intensity curve with Save. Compress clean signal, configured grace progression, and overtime into a 15-second demo, stopping on the card. Keep shader animation at normal speed. The curve has a vertically adjustable start, two adjustable middle points, and a fixed 100% endpoint. Use smooth monotone interpolation without overshoot; show the playback position on the curve and support keyboard point editing. Individual effect tuning lives in configuration, not UI controls. An image menu on the preview selects a remembered local screenshot or resets to crisp bundled artwork.
- Live Preview pauses playback and confirms the displayed effect or test card. A confirmed desktop trial lasts five seconds and expires independently of GUI repainting. Closing the editor cancels it. It can borrow the background display owner without stopping or resetting the controller; afterward restore the latest background state.
- Settings live in the normal per-user configuration location. Background operation reloads saved changes without resetting debt.

## Implementation boundaries

- One Rust executable provides windowless background operation, the editor, capability diagnostics, timer control and panic recovery, and an internal guardian process. Embed required shaders, artwork, and fonts. Temporary runtime files are allowed.
- Use Hyprland's screen shader through direct IPC, not desktop capture, a plugin, or a `hyprctl` subprocess. Do not require an external scripting interpreter.
- The break controller owns timing and transitions independently of rendering. The effect model maps glitchiness to shader behaviour. Display integration owns installation, overlays, restoration, and recovery.
- Use standard Wayland input-idle v2 notifications, ignoring media inhibitors; no global keyboard capture or input-device permissions.
- Use SCTK layer-shell/shared-memory test cards on every output. Consume ordinary keyboard and pointer input so hidden applications receive neither; compositor emergency shortcuts remain available. Handle output creation/removal, scaling, and rotation. This is not a security lock screen.
- The steady card uses software drawing, not a second continuously animated GPU renderer. Enable continuous compositor redraw only during interference; restore normal rendering during work and the test card.
- The deployable artifact is a dynamically linked Linux executable with host graphics/driver prerequisites, not a fully static binary. NixOS graphics-library discovery reads installed Hyprland ELF metadata and permits environment overrides; it does not install packages or change persistent configuration. See `../README.md` for requirements and commands.

## Recovery

- A separate invocation of the same executable owns display mutations and recovers independently of main-process exit handlers, including parent EOF and missed heartbeats.
- Journal exact prior shader and rendering settings before mutation. Restore only values still owned by Bork; preserve newer external changes and retain unresolved journals for retry.
- Locking independently removes effects and overlays. Trial deadlines use boot time so suspension cannot extend them.
- Emergency reset must remain usable with broken graphics overrides. Compositor disconnection/conflicts are recoverable errors, not permission to guess replacement settings.

## Visual reference and scope

- [Storyboard](assets/storyboard.html) and [desktop overview](assets/storyboard.png) define the visual direction. Their keyboard-only activity labels are historical; the keyboard-and-mouse rule above is authoritative.
- Desktop effects target Hyprland, not other compositors or operating systems. The editor may run independently of Hyprland.
- No security-lock functionality, TUI, accounts, telemetry, cloud services, or automatic system/session configuration.

Build, runtime, testing, and hardware caveats belong in `../README.md`; execution policy belongs in `../AGENTS.md`.
