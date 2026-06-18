{
  description = "Wayland devshell (fixes NoWaylandLib)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };

        # Libraries that Wayland/winit/wgpu binaries dlopen() at runtime.
        # wayland-client doesn't link libwayland-client.so — it loads it
        # dynamically, so it must be on LD_LIBRARY_PATH or you get NoWaylandLib.
        runtimeLibs = with pkgs; [
          wayland         # libwayland-client.so.0  <- the NoWaylandLib culprit
          libxkbcommon    # keyboard input
          libGL           # OpenGL
          vulkan-loader   # Vulkan / wgpu / ash
          # winit also dlopens the X11 libs (for its X11 backend):
          xorg.libX11
          xorg.libXcursor
          xorg.libXi
          xorg.libXrandr
        ];
      in {
        devShells.default = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            pkg-config
          ];

          buildInputs = runtimeLibs;

          # The fix: make those dlopen()'d .so files findable at runtime.
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeLibs;
        };
      });
}
