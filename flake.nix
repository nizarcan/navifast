{
  description = "Navidrome, native and fast";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    # rust-toolchain.toml pins the compiler so local builds and CI agree.
    # This reads that file rather than restating the version here.
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
      ...
    }:
    let
      systems = [
        "aarch64-darwin"
        "x86_64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];
      forAllSystems =
        f:
        nixpkgs.lib.genAttrs systems (
          system:
          f (
            import nixpkgs {
              inherit system;
              overlays = [ (import rust-overlay) ];
            }
          )
        );
    in
    {
      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages =
            with pkgs;
            [
              (rust-bin.fromRustupToolchainFile ./rust-toolchain.toml)
              rust-analyzer
              pkg-config
            ]
            ++ lib.optionals stdenv.hostPlatform.isDarwin [
              apple-sdk
            ]
            ++ lib.optionals stdenv.hostPlatform.isLinux [
              dbus
              alsa-lib
              libpulseaudio
              libxkbcommon
              vulkan-loader
              wayland
              libGL
              libx11
              libxcursor
              libxi
              libxrandr
            ];
          # The GUI dlopens its Wayland, X11, Vulkan and GL libraries at run time.
          LD_LIBRARY_PATH = pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux (
            pkgs.lib.makeLibraryPath (
              with pkgs;
              [
                dbus
                libxkbcommon
                vulkan-loader
                wayland
                libGL
                libx11
                libxcursor
                libxi
                libxrandr
              ]
            )
          );
        };
      });

      packages = forAllSystems (
        pkgs:
        let
          navifast =
            let
              toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
              rustPlatform = pkgs.makeRustPlatform {
                cargo = toolchain;
                rustc = toolchain;
              };
              runtimeLibs = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux (
                with pkgs;
                [
                  dbus
                  libxkbcommon
                  vulkan-loader
                  wayland
                  libGL
                  libx11
                  libxcursor
                  libxi
                  libxrandr
                ]
              );
            in
            rustPlatform.buildRustPackage rec {
              pname = "navifast";
              version = (pkgs.lib.importTOML ./Cargo.toml).package.version;
              src = self;

              # The lock file contains git dependencies. fetchCargoVendor includes
              # them in the fixed-output dependency tree, unlike cargoLock alone.
              cargoDeps = pkgs.rustPlatform.fetchCargoVendor {
                pname = "navifast";
                version = (pkgs.lib.importTOML ./Cargo.toml).package.version;
                src = self;
                hash = "sha256-RLQG492Zo6PFRfb7rhKJWGIQHOahn3l7d8ngtqpQRpU=";
              };
              nativeBuildInputs =
                with pkgs;
                [
                  pkg-config
                ]
                ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [ makeWrapper ]
                ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [
                  rcodesign
                  icnsify
                ];
              # The command-alias regression starts its own bus, isolated from
              # the listener's desktop and running application.
              nativeCheckInputs = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [ pkgs.dbus ];
              buildInputs =
                pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux (
                  with pkgs;
                  [
                    libxkbcommon
                    alsa-lib
                    libpulseaudio
                    libGL
                    libx11
                  ]
                )
                ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [ pkgs.apple-sdk ];

              # Nix's Rust check hook deliberately points SSL_CERT_FILE at a
              # missing path. Give request regressions an explicit trust store.
              preCheck = ''
                export SSL_CERT_FILE="${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
              '';

              # The GUI dlopens its Wayland, X11, Vulkan and GL libraries at run time.
              postFixup =
                pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
                  wrapProgram $out/bin/navifast \
                    --prefix LD_LIBRARY_PATH : ${pkgs.lib.makeLibraryPath runtimeLibs}
                ''
                + pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isDarwin ''
                  rcodesign sign "$out/Applications/Navifast.app"
                '';

              postInstall = ''
                # The compatibility binary is a migration reference, not a
                # command owned by this client. Keep upstream installs intact.
                rm -f "$out/bin/fastpotify"
              '' + pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
                  install -Dm644 packaging/applications/navifast.desktop \
                    $out/share/applications/navifast.desktop
                  install -Dm644 packaging/icons/navifast.svg \
                    $out/share/icons/hicolor/scalable/apps/navifast.svg
                  install -Dm644 contrib/omarchy/navifast.json.tpl \
                    $out/share/navifast/omarchy/navifast.json.tpl
                  install -Dm755 contrib/omarchy/navifast-theme \
                    $out/share/navifast/omarchy/navifast-theme
                ''
                + pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isDarwin ''
                  app="$out/Applications/Navifast.app/Contents"
                  mkdir -p "$app/MacOS" "$app/Resources"
                  executable=Navifast
                  identifier=io.github.nizarcan.Navifast
                  cp "$out/bin/navifast" "$app/MacOS/$executable"
                  icnsify packaging/macos/icon-1024.png -o "$app/Resources/navifast.icns"
                  substitute packaging/macos/Info.plist "$app/Info.plist" \
                    --replace-fail __VERSION__ "${version}" \
                    --replace-fail __BUILD__ "${pkgs.lib.head (pkgs.lib.splitString "-" version)}" \
                    --replace-fail __EXECUTABLE__ "$executable" \
                    --replace-fail __IDENTIFIER__ "$identifier"
                '';

              meta = {
                description = "Native Navidrome client with local playback and equalizer";
                homepage = "https://nizarcan.github.io/navifast";
                license = pkgs.lib.licenses.mit;
                mainProgram = "navifast";
              };
            };

        in
        {
          default = navifast;
          inherit navifast;
        }
        // pkgs.lib.optionalAttrs pkgs.stdenv.hostPlatform.isDarwin {
          navifast-app = navifast;
        }
      );

      formatter = forAllSystems (pkgs: pkgs.nixfmt-tree);
    };
}
