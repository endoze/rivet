{
  description = "Dev shell for the Rivet workspace";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { nixpkgs, flake-utils, fenix, ... }:
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

        windowsTarget = "x86_64-pc-windows-msvc";

        # nixpkgs' rustc ships only the host's standard library.
        windowsToolchain = with fenix.packages.${system}; combine [
          stable.rustc
          stable.cargo
          stable.rust-std
          targets.${windowsTarget}.stable.rust-std
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

        # Cross-compiles Rivet for Windows and runs its tests under Wine:
        #   cargo xwin build --target x86_64-pc-windows-msvc
        #   cargo xwin test --workspace --no-fail-fast --target x86_64-pc-windows-msvc
        # The app itself cannot start under Wine yet: Wine's DirectWrite does
        # not implement the font matching gpui needs.
        devShells.windows = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            windowsToolchain
            cargo-xwin
            llvmPackages.clang-unwrapped
            llvmPackages.lld
            llvmPackages.llvm
            cmake
            nasm
            wineWow64Packages.stable
          ];

          WINEDEBUG = "-all";
          # Rivet needs neither .NET nor a browser engine, and their install
          # prompts would block a fresh prefix waiting on a click.
          WINEDLLOVERRIDES = "mscoree,mshtml=";
          # A project-local Wine prefix keeps ~/.wine untouched.
          shellHook = ''
            export WINEPREFIX="$PWD/target/wine"
          '';
        };
      });
}
