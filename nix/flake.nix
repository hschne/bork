{
  description = "Bork development environment";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  outputs = { nixpkgs, ... }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in {
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
