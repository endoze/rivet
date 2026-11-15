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

        windowsBuildInputs = with pkgs; [
          windowsToolchain
          cargo-xwin
          llvmPackages.clang-unwrapped
          llvmPackages.lld
          llvmPackages.llvm
          cmake
          nasm
          # Compiles gpui's shaders for a release build; see vendor/gpui-pre-windows.
          vkd3d
        ];

        # Builds Rivet for Windows and runs it in a local Windows VM, reached
        # over SSH at localhost (quickemu forwards port 22220 to the VM's SSH
        # server). The VM needs the OpenSSH server and your public key in
        # C:\ProgramData\ssh\administrators_authorized_keys.
        windowsVm = pkgs.writeShellApplication {
          name = "windows-vm";
          runtimeInputs = windowsBuildInputs ++ [ pkgs.openssh ];
          text = ''
            if [ ! -d crates/rivet-app ]; then
              echo "windows-vm: run this from the Rivet repo root" >&2
              exit 1
            fi

            port="''${RIVET_VM_PORT:-22220}"
            host="''${RIVET_VM_USER:-Quickemu}@localhost"
            # The VM's host key changes whenever it is rebuilt.
            opts=(-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR)

            cargo xwin build -p rivet-app --release --target ${windowsTarget}

            ssh -p "$port" "''${opts[@]}" "$host" 'taskkill /im rivet.exe /f' >/dev/null 2>&1 || true
            scp -P "$port" "''${opts[@]}" target/${windowsTarget}/release/rivet.exe "$host:Desktop/rivet.exe"

            # A program started over SSH gets no window, so a scheduled task
            # starts it in the desktop session instead.
            ssh -p "$port" "''${opts[@]}" "$host" \
              'schtasks /create /tn rivet /tr "cmd /c set RUST_BACKTRACE=1&& cd /d %USERPROFILE%\Desktop&& rivet.exe > %USERPROFILE%\rivet.log 2>&1" /sc once /st 23:59 /it /f && schtasks /run /tn rivet' >/dev/null

            sleep 5

            if ssh -p "$port" "''${opts[@]}" "$host" 'tasklist /fi "imagename eq rivet.exe"' | grep -q rivet.exe; then
              echo "Rivet is running in the VM."
            else
              echo "Rivet exited. Its output:" >&2
              ssh -p "$port" "''${opts[@]}" "$host" 'type %USERPROFILE%\rivet.log' >&2
              exit 1
            fi
          '';
        };
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
        #   cargo xwin build -p rivet-app --release --target x86_64-pc-windows-msvc
        # The app itself cannot start under Wine yet: Wine's DirectWrite does
        # not implement the font matching gpui needs. `nix run .#windows-vm`
        # runs it in a Windows VM instead.
        devShells.windows = pkgs.mkShell {
          nativeBuildInputs = windowsBuildInputs ++ [ pkgs.wineWow64Packages.stable ];

          WINEDEBUG = "-all";
          # Rivet needs neither .NET nor a browser engine, and their install
          # prompts would block a fresh prefix waiting on a click.
          WINEDLLOVERRIDES = "mscoree,mshtml=";
          # A project-local Wine prefix keeps ~/.wine untouched.
          shellHook = ''
            export WINEPREFIX="$PWD/target/wine"
          '';
        };

        apps.windows-vm = {
          type = "app";
          program = "${windowsVm}/bin/windows-vm";
        };
      });
}
