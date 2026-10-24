{
  description = "Bork development and isolated graphics verification";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  outputs = { nixpkgs, ... }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in {
      # Test-only dependency fix: Aquamarine 0.15.1 ignores advertised Wayland versions.
      # Never installed on the host; used only inside the isolated nested test sandbox.
      packages = forAllSystems (system:
        let pkgs = nixpkgs.legacyPackages.${system}; in {
          nested-aquamarine = pkgs.aquamarine.overrideAttrs (old: {
            postPatch = (old.postPatch or "") + ''
              substituteInPlace src/backend/Wayland.cpp \
                --replace-fail '&xdg_wm_base_interface, 6)' '&xdg_wm_base_interface, std::min(version, 6U))' \
                --replace-fail '&wl_compositor_interface, 6)' '&wl_compositor_interface, std::min(version, 6U))' \
                --replace-fail '&wl_seat_interface, 9)' '&wl_seat_interface, std::min(version, 9U))'
            '';
          });
        });
      devShells = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          graphics = with pkgs; [ libGL libxkbcommon wayland libX11 libXcursor libXi libXrandr ];
        in {
          default = pkgs.mkShell {
            packages = with pkgs; [ pkg-config xorg-server xdotool ] ++ graphics;
            LD_LIBRARY_PATH = "${pkgs.lib.makeLibraryPath graphics}:/run/opengl-driver/lib";
            shellHook = ''
              echo "Bork: use mise run check; mise run release. No desktop settings changed."
            '';
          };
        });
    };
}
