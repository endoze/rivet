{
  description = "Dev shell for the mod manager workspace";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { nixpkgs, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };

        # Native libraries gpui links against at build time (via pkg-config)
        # and/or loads at runtime (via dlopen, so they need LD_LIBRARY_PATH).
        runtimeLibs = with pkgs; [
          fontconfig
          freetype
          vulkan-loader
          libxkbcommon
          wayland
          libx11
          libxcb
          openssl
          zstd
        ];
      in
      {
        devShells.default = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            pkg-config
            cmake
            clang
            rustc
            cargo
            rustfmt
            clippy
            rust-analyzer
          ];

          buildInputs = runtimeLibs;

          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeLibs;
        };
      });
}
