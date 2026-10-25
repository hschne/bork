# Host requirements and runtime libraries

[Back to README](../README.md)

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
