{
  description = "Silon: a deterministic TEA framework on top of gpui";

  inputs.nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";

  outputs =
    { nixpkgs, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});

      # Libraries gpui links against or loads with dlopen at runtime on Linux
      # (Vulkan via blade, Wayland/X11 windowing, xkbcommon keymaps, fonts).
      linuxRuntimeLibs =
        pkgs: with pkgs; [
          fontconfig
          freetype
          libxkbcommon
          vulkan-loader
          wayland
          libx11
          libxcursor
          libxi
          libxrandr
          libxcb
        ];
    in
    {
      devShells = forAllSystems (
        pkgs:
        let
          inherit (pkgs) lib stdenv;
          cargoTarget = stdenv.hostPlatform.rust.cargoEnvVarTarget;
        in
        {
          default = pkgs.mkShell (
            {
              nativeBuildInputs = [ pkgs.pkg-config ];
              buildInputs = lib.optionals stdenv.hostPlatform.isLinux (linuxRuntimeLibs pkgs);
              packages =
                with pkgs;
                [
                  cargo
                  rustc
                  clippy
                  rustfmt
                  rust-analyzer
                  curl
                  # Checks that CI runs (see .github/workflows).
                  cargo-deny
                  cargo-mutants
                  typos
                  actionlint
                  shellcheck
                  nixfmt
                ]
                ++ lib.optionals stdenv.hostPlatform.isLinux [ mold ];
            }
            // lib.optionalAttrs stdenv.hostPlatform.isLinux {
              LD_LIBRARY_PATH = lib.makeLibraryPath (linuxRuntimeLibs pkgs);
              # Link with the Nix toolchain so binaries match LD_LIBRARY_PATH,
              # overriding a host-level ~/.cargo/config.toml linker.
              # RUSTFLAGS replaces target.*.rustflags from that file as well.
              "CARGO_TARGET_${cargoTarget}_LINKER" = "cc";
              RUSTFLAGS = "-C link-arg=-fuse-ld=mold";
            }
          );

          # Lightweight shell for lint.yml without Rust/Vulkan/rust-analyzer.
          lint = pkgs.mkShell {
            packages = with pkgs; [
              cargo-deny
              typos
              actionlint
              shellcheck
              nixfmt
            ];
          };

          # Quint and the Apalache model checker it runs, which need a JDK.
          # Kept out of the default shell: `nix develop .#spec`.
          spec = pkgs.mkShell { packages = [ pkgs.quint ]; };
        }
      );

      formatter = forAllSystems (pkgs: pkgs.nixfmt);
    };
}
