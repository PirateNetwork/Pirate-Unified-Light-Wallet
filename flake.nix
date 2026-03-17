{
  description = "Pirate Unified Wallet";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.11";
    flake-utils.url = "github:numtide/flake-utils";

    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    flutter-nix = {
      url = "github:maximoffua/flutter.nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    rust-overlay,
    flutter-nix,
  }:
    flake-utils.lib.eachSystem [
      "x86_64-linux"
      "aarch64-linux"
      "x86_64-darwin"
      "aarch64-darwin"
    ] (
      system:
      let
        overlays = [ rust-overlay.overlays.default ];
        pkgs = import nixpkgs {
          inherit system overlays;
          config.allowUnfree = true;
          config.android_sdk.accept_license = true;
        };
        lib = pkgs.lib;

        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        flutterPkg = flutter-nix.packages.${system}.flutter;

        androidSdk =
          if pkgs.stdenv.isLinux then
            (
              pkgs.androidenv.composeAndroidPackages {
                platformVersions = [ "34" ];
                buildToolsVersions = [ "34.0.0" ];
                includeNDK = true;
                ndkVersions = [ "26.1.10909125" ];
                cmakeVersions = [ "3.22.1" ];
                includeEmulator = false;
                includeSystemImages = false;
              }
            ).androidsdk
          else
            null;

        commonPackageInputs =
          (with pkgs; [
            bash
            git
            curl
            wget
            jq
            zip
            unzip
            file
            pkg-config
            cmake
            ninja
            protobuf
            rustToolchain
            flutterPkg
          ])
          ++ lib.optionals pkgs.stdenv.isLinux [
            pkgs.go_1_23
            pkgs.openssl
            pkgs.sqlite
            pkgs.zlib
            pkgs.libsodium
          ]
          ++ lib.optionals pkgs.stdenv.isDarwin [
            pkgs.cocoapods
            pkgs.libiconv
          ];

        commonShellInputs =
          commonPackageInputs
          ++ lib.optionals pkgs.stdenv.isLinux [
            pkgs.android-tools
            pkgs.flatpak
            pkgs.flatpak-builder
            pkgs.dpkg
          ]
          ++ lib.optionals pkgs.stdenv.isDarwin [
            pkgs.cocoapods
          ];

        shellEnv =
          {
            FLUTTER_SUPPRESS_ANALYTICS = "true";
            DART_SUPPRESS_ANALYTICS = "true";
            RUST_BACKTRACE = "1";
            CARGO_INCREMENTAL = "0";
          }
          // lib.optionalAttrs pkgs.stdenv.isLinux {
            ANDROID_HOME = "${androidSdk}/libexec/android-sdk";
            ANDROID_SDK_ROOT = "${androidSdk}/libexec/android-sdk";
            JAVA_HOME = "${pkgs.jdk21}";
          };

        mkNativeScriptPackage =
          {
            pname,
            script,
            args ? "",
            extraNativeBuildInputs ? [ ],
            installPhase,
          }:
          pkgs.stdenv.mkDerivation (
            shellEnv
            // {
              inherit pname;
              version = "1.0.6";
              src = ./.;
              nativeBuildInputs = commonPackageInputs ++ extraNativeBuildInputs;
              dontConfigure = true;
              buildPhase = ''
                export HOME="$TMPDIR/home"
                export PUB_CACHE="$TMPDIR/pub-cache"
                mkdir -p "$HOME" "$PUB_CACHE"
                export SOURCE_DATE_EPOCH="${toString self.lastModified}"
                chmod +x ./scripts/*.sh
                bash ${script} ${args}
              '';
              inherit installPhase;
            }
          );

        linuxPackages = lib.optionalAttrs pkgs.stdenv.isLinux {
          android-apk = mkNativeScriptPackage {
            pname = "pirate-unified-wallet-android-apk";
            script = ./scripts/build-android.sh;
            args = "apk";
            extraNativeBuildInputs = [ androidSdk pkgs.jdk21 ];
            installPhase = ''
              mkdir -p "$out"
              cp -r dist/android/. "$out/"
            '';
          };

          android-bundle = mkNativeScriptPackage {
            pname = "pirate-unified-wallet-android-bundle";
            script = ./scripts/build-android.sh;
            args = "bundle";
            extraNativeBuildInputs = [ androidSdk pkgs.jdk21 ];
            installPhase = ''
              mkdir -p "$out"
              cp -r dist/android/. "$out/"
            '';
          };

          linux-appimage = mkNativeScriptPackage {
            pname = "pirate-unified-wallet-linux-appimage";
            script = ./scripts/build-linux.sh;
            args = "appimage";
            installPhase = ''
              mkdir -p "$out"
              cp -r dist/linux/. "$out/"
            '';
          };

          linux-flatpak = mkNativeScriptPackage {
            pname = "pirate-unified-wallet-linux-flatpak";
            script = ./scripts/build-linux.sh;
            args = "flatpak";
            extraNativeBuildInputs = [ pkgs.flatpak pkgs.flatpak-builder ];
            installPhase = ''
              mkdir -p "$out"
              cp -r dist/linux/. "$out/"
            '';
          };

          linux-deb = mkNativeScriptPackage {
            pname = "pirate-unified-wallet-linux-deb";
            script = ./scripts/build-linux.sh;
            args = "deb";
            extraNativeBuildInputs = [ pkgs.dpkg ];
            installPhase = ''
              mkdir -p "$out"
              cp -r dist/linux/. "$out/"
            '';
          };
        };

        darwinPackages = lib.optionalAttrs pkgs.stdenv.isDarwin {
          macos-dmg = mkNativeScriptPackage {
            pname = "pirate-unified-wallet-macos-dmg";
            script = ./scripts/build-macos.sh;
            installPhase = ''
              mkdir -p "$out"
              cp -r dist/macos/. "$out/"
            '';
          };

          ios-ipa = mkNativeScriptPackage {
            pname = "pirate-unified-wallet-ios-ipa";
            script = ./scripts/build-ios.sh;
            args = "false";
            extraNativeBuildInputs = [ pkgs.cocoapods ];
            installPhase = ''
              mkdir -p "$out"
              cp -r dist/ios/. "$out/"
            '';
          };
        };

        fallbackPackage = pkgs.runCommand "pirate-unified-wallet-no-native-package" { } ''
          mkdir -p "$out"
          cat > "$out/README.txt" <<'EOF'
This flake does not define native release packages for the current system.
Use one of the supported native systems:
- Linux for Android and Linux packages
- macOS for macOS and iOS packages
EOF
        '';
      in
      {
        devShells.default = pkgs.mkShell (
          shellEnv
          // {
            packages =
              commonShellInputs
              ++ lib.optionals pkgs.stdenv.isLinux [ androidSdk pkgs.jdk21 ];
            shellHook = ''
              echo "Pirate Unified Wallet development shell"
              echo "Rust:    $(rustc --version)"
              echo "Cargo:   $(cargo --version)"
              echo "Flutter: $(flutter --version | head -n1)"
              ${lib.optionalString pkgs.stdenv.isLinux ''
                echo "Android SDK: $ANDROID_SDK_ROOT"
                echo "Java:       $(java -version 2>&1 | head -n1)"
              ''}
            '';
          }
        );

        devShells.ci = pkgs.mkShell (
          shellEnv
          // {
            packages =
              commonPackageInputs
              ++ lib.optionals pkgs.stdenv.isLinux [ androidSdk pkgs.jdk21 ];
          }
        );

        devShells.build = pkgs.mkShell (
          shellEnv
          // {
            packages =
              commonPackageInputs
              ++ lib.optionals pkgs.stdenv.isLinux [ androidSdk pkgs.jdk21 ];
          }
        );

        formatter = pkgs.nixpkgs-fmt;

        packages = linuxPackages // darwinPackages // {
          default =
            if pkgs.stdenv.isLinux then
              linuxPackages.linux-appimage
            else if pkgs.stdenv.isDarwin then
              darwinPackages.macos-dmg
            else
              fallbackPackage;
        };
      }
    );
}
