# Signal Loss

## Product

A playful but hard-to-ignore break reminder for Hyprland on Linux. It distorts the actual desktop rather than covering it with an ordinary notification.

- After 20 minutes of work, enter a three-minute grace period. Interference escalates nonlinearly, then a steady **TAKE A BREAK** test card covers every output.
- Five continuous minutes without keyboard **or mouse** input completes a break. Reading and watching count; media idle inhibitors must not prevent a break.
- Inactivity accumulated before the test card counts toward the same five minutes. Short pauses retain work debt. After a completed break, resumed input starts the next work interval.
- Locking suppresses effects without resetting debt. Input-free suspend and lock time count toward a break. Observe startup and input-seat changes conservatively rather than inventing prior inactivity.
- Glitchiness is continuous from 0 to 100, separate from the deadline/test-card state. Selecting 100 in the editor must not trigger a test card.

## Effects and editor

- Desktop and editor use the same embedded GLSL effect implementation, with only rendering-interface adaptation.
- Glitchiness controls spatial strength, average burst frequency, and average duration separately. Burst starts and lengths are irregular; effects and monitors have independent streams, including identical monitors.
- Each application run gets fresh entropy. Keep its seed stable during tuning and shader recompilation; do not expose seed, sweep, or frame-scrubbing controls.
- The egui/glow editor shows a genuinely shader-rendered sample desktop beside clean controls. Provide exact numeric 0–100 input, individual effect toggles, entry levels, maximum strengths, grace-curve tuning, test-card preview, and save/reopen.
- A confirmed desktop trial lasts five seconds and expires independently of GUI repainting. Closing the editor cancels it. It can borrow the background display owner without stopping or resetting the controller; afterward restore the latest background state.
- Settings live in the normal per-user configuration location. Background operation reloads saved changes without resetting debt.

## Implementation boundaries

- One Rust executable provides windowless background operation, the editor, capability diagnostics, emergency stop, and an internal guardian process. Embed required shaders, artwork, and fonts. Temporary runtime files are allowed.
- Use Hyprland's screen shader through direct IPC, not desktop capture, a plugin, or a `hyprctl` subprocess. Do not require an external scripting interpreter.
- The break controller owns timing and transitions independently of rendering. The effect model maps glitchiness to shader behaviour. Display integration owns installation, overlays, restoration, and recovery.
- Use standard Wayland input-idle v2 notifications, ignoring media inhibitors; no global keyboard capture or input-device permissions.
- Use SCTK layer-shell/shared-memory test cards on every output. Consume ordinary keyboard and pointer input so hidden applications receive neither; compositor emergency shortcuts remain available. Handle output creation/removal, scaling, and rotation. This is not a security lock screen.
- The steady card uses software drawing, not a second continuously animated GPU renderer. Enable continuous compositor redraw only during interference; restore normal rendering during work and the test card.
- The deployable artifact is a dynamically linked Linux executable with host graphics/driver prerequisites, not a fully static binary. NixOS graphics-library discovery reads installed Hyprland ELF metadata and permits environment overrides; it does not install packages or change persistent configuration. See `../README.md` for requirements and commands.

## Recovery

- A separate invocation of the same executable owns display mutations and recovers independently of main-process exit handlers, including parent EOF and missed heartbeats.
- Journal exact prior shader and rendering settings before mutation. Restore only values still owned by Signal Loss; preserve newer external changes and retain unresolved journals for retry.
- Locking independently removes effects and overlays. Trial deadlines use boot time so suspension cannot extend them.
- Emergency stop must remain usable with broken graphics overrides. Compositor disconnection/conflicts are recoverable errors, not permission to guess replacement settings.

## Visual reference and scope

- [Storyboard](assets/storyboard.html) and [desktop overview](assets/storyboard.png) define the visual direction. Their keyboard-only activity labels are historical; the keyboard-and-mouse rule above is authoritative.
- Desktop effects target Hyprland, not other compositors or operating systems. The editor may run independently of Hyprland.
- No security-lock functionality, TUI, accounts, telemetry, cloud services, or automatic system/session configuration.

Build, runtime, testing, and hardware caveats belong in `../README.md`; execution policy belongs in `../AGENTS.md`.
