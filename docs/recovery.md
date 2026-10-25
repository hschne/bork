# Safety, recovery, and timing

[Back to README](../README.md)

## Before running desktop effects

Interference is intentionally disruptive. The test card consumes ordinary keyboard and pointer input over every output. It is **not a security lock screen**; compositor shortcuts remain available.

Arrange a way to run `bork reset`, such as a compositor shortcut using `/absolute/path/to/bork reset` or another terminal/TTY. Bork does not add bindings itself. Avoid changing the same settings with another shader manager during interference or a desktop trial.

## Clear effects or recover after a crash

Run this in the same Hyprland session:

```sh
bork reset
```

- With a healthy timer, reset clears effects, cancels desktop trials, and starts a fresh running work interval.
- If the timer is absent or does not reply within two seconds, reset attempts independent guardian shutdown and recovery from the saved journal. This does not start a new timer.
- To shut down a healthy timer entirely, use Ctrl-C in its terminal or send it SIGTERM.

If the compositor is disconnected, retry recovery after it becomes reachable. Bork cannot restore an unresponsive or dead compositor.

### Restoration conflicts

Bork records the exact previous shader path, damage tracking, and VFR values before changing them. Recovery restores only settings it still owns. A setting changed externally is left untouched rather than overwritten with an old value.

Conflicts are reported as errors. The retained `journal.json` lives in the private, session-specific `$XDG_RUNTIME_DIR/bork-<hash>/` directory. Inspect it and resolve the conflicting setting deliberately; do not delete the journal while Bork still owns an active shader.

If Hyprland restarts, the new session and configuration own the display. Do not apply an old session’s journal to a different compositor instance.

## Automatic safeguards

A separate guardian process, launched from the same executable, owns all desktop changes. A main-process crash, closed pipe, or missed two-second heartbeat triggers restoration and releases overlays independently of normal exit handlers. Restoration remains subject to compositor availability and ownership conflicts.

The guardian also:

- Enforces five-second desktop trial expiry independently of GUI repainting. Suspension does not extend trials.
- Removes effects and overlays when the session locks, without resetting the timer’s accumulated work.
- Enables continuous compositor redraw only during interference, restoring ordinary rendering for work and the steady test card.
- Restores an existing user shader rather than replacing it with a guessed default.

Required shaders, artwork, and fonts are embedded in the executable. Runtime files use the private session directory; the desktop shader is extracted there for Hyprland.

## Timing rules

- **Short idle periods preserve elapsed work.** Interference continues until a complete break, a security lock, or an explicit pause/reset.
- **A complete break clears elapsed work at any point**, even during the work interval. Inactivity already accumulated during grace counts toward the same configured break on the card.
- **Staying idle after a break does not start another reminder.** If unlocked, first resumed input starts the next work interval. If locked, waking the display or typing a password does not start work; successful unlock does.
- **Pause freezes the timer.** Paused time does not advance work or complete a break. Resume retains elapsed work and starts fresh idle observation.
- **Suspend counts as input-free time.** A short suspend does not erase elapsed work. Bork distinguishes suspension from an application stall using boot-time and monotonic clocks.
- **Locking does not reset the cycle.** Input-free lock time counts toward a break; keyboard or mouse input while locked interrupts an incomplete break. Unlocking early retains elapsed work. Bork never locks or unlocks the session; your existing locker and idle-lock policy remain responsible.
- **Idle observation starts conservatively.** Standard Wayland does not expose pre-launch idle history. Observation has one-second granularity and does not finish a break early through rounding. Input-seat topology changes restart observation rather than backdating inactivity.
