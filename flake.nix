{
  description = "Pirate Unified Wallet - Privacy-first cryptocurrency wallet for Pirate Chain";

  inputs = {
    # Pin to specific commit for reproducibility
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
    
    # SBOM and provenance tools
    syft = {
      url = "github:anchore/syft";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay, flutter-nix, syft }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
          config.allowUnfree = true; # Required for Android SDK
          config.android_sdk.accept_license = true;
        };

        # Latest stable Rust version (auto-tracks latest)
        # Using "latest" instead of pinning to specific version for fresh starts
        rustVersion = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" "clippy" "rustfmt" ];
          targets = [ 
            "x86_64-unknown-linux-gnu"
            "x86_64-pc-windows-gnu"
            "x86_64-apple-darwin"
            "aarch64-apple-darwin"
            "aarch64-linux-android"
            "armv7-linux-androideabi"
            "x86_64-linux-android"
            "i686-linux-android"
            "aarch64-apple-ios"
            "x86_64-apple-ios"
          ];
        };

        # Flutter setup
        flutterPkg = flutter-nix.packages.${system}.flutter;

        # Android SDK components (latest versions)
        androidComposition = pkgs.androidenv.composeAndroidPackages {
          platformVersions = [ "34" "35" ];  # Latest stable
          buildToolsVersions = [ "34.0.0" "35.0.0" ];  # Latest
          includeNDK = true;
          ndkVersions = [ "27.0.12077973" ];  # Latest NDK
          cmakeVersions = [ "3.30.5" ];  # Latest CMake
          includeEmulator = false;
          includeSystemImages = false;
        };

        androidSdk = androidComposition.androidsdk;

        # Build inputs common to all platforms
        commonBuildInputs = with pkgs; [
          # Rust toolchain
          rustVersion
          
          # Rust dependency management & security tools
          cargo-audit          # Check for known security vulnerabilities
          cargo-deny           # Lint your Cargo.toml for security/license issues
          cargo-outdated       # Check for outdated dependencies
          cargo-edit           # cargo add, cargo rm, cargo upgrade commands
          cargo-upgrades       # Show available version upgrades
          cargo-tarpaulin      # Code coverage
          cargo-watch          # Watch for changes and run commands
          cargo-nextest        # Next-generation test runner
          
          # Flutter toolchain
          flutterPkg

          # Go (for gomobile) - Latest stable
          go_1_23

          # Build tools
          pkg-config
          cmake
          ninja
          protobuf

          # Development tools
          git
          curl
          wget
          gnupg
          jq                   # JSON processor for scripts
          yq                   # YAML processor
          
          # Documentation
          mdbook
          
          # Code quality tools
          shellcheck           # Shell script linter
        ];

        # Platform-specific inputs
        darwinInputs = with pkgs; lib.optionals stdenv.isDarwin [
          darwin.apple_sdk.frameworks.Security
          darwin.apple_sdk.frameworks.CoreFoundation
          darwin.apple_sdk.frameworks.CoreServices
          darwin.apple_sdk.frameworks.SystemConfiguration
          libiconv
        ];

        linuxInputs = with pkgs; lib.optionals stdenv.isLinux [
          openssl
          sqlite
          zlib
          libsodium
          
          # GTK for Linux desktop
          gtk3
          glib
          pcre
          util-linux
          libselinux
          libsepol
          libthai
          libdatrie
          xorg.libXdmcp
          libxkbcommon
          dbus
          at-spi2-core
          libsecret
          jsoncpp
          
          # Wayland support
          wayland
          wayland-protocols
        ];

        # Shell environment
        shellEnv = {
          ANDROID_HOME = "${androidSdk}/libexec/android-sdk";
          ANDROID_SDK_ROOT = "${androidSdk}/libexec/android-sdk";
          JAVA_HOME = "${pkgs.jdk21}";  # Latest LTS (was 17)
          
          # Flutter configuration
          FLUTTER_ROOT = "${flutterPkg}";
          
          # Rust configuration
          RUST_BACKTRACE = "1";
          RUSTFLAGS = "-D warnings";  # Treat warnings as errors in CI
          CARGO_HOME = "${placeholder "out"}/.cargo";
          
          # Build optimization
          CARGO_BUILD_JOBS = "8";
          CARGO_INCREMENTAL = "1";  # Enable incremental compilation in dev
          
          # Security: disable telemetry
          FLUTTER_SUPPRESS_ANALYTICS = "true";
          DART_SUPPRESS_ANALYTICS = "true";
          DO_NOT_TRACK = "1";  # Universal opt-out
        };

      in
      {
        # Development shell
        devShells.default = pkgs.mkShell {
          buildInputs = commonBuildInputs 
            ++ darwinInputs 
            ++ linuxInputs
            ++ [ androidSdk pkgs.jdk21 ];  # Latest LTS

          inherit shellEnv;

          shellHook = ''
            echo " Pirate Unified Wallet Development Environment"
            echo ""
            echo " Versions:"
            echo "  Rust:    $(rustc --version)"
            echo "  Cargo:   $(cargo --version)"
            echo "  Flutter: $(flutter --version | head -n1)"
            echo "  Dart:    $(dart --version 2>&1 | head -n1)"
            echo "  Go:      $(go version)"
            echo "  Java:    $(java -version 2>&1 | head -n1)"
            echo ""
            echo " Android SDK: $ANDROID_HOME"
            echo ""
            echo " Available commands:"
            echo "  make bootstrap       - Install dependencies"
            echo "  make build:all       - Build all targets"
            echo "  make test:all        - Run all tests"
            echo "  make ci              - Run CI checks locally"
            echo "  make check-updates   - Check for outdated dependencies"
            echo ""
            echo " Security tools available:"
            echo "  cargo audit          - Check for vulnerabilities"
            echo "  cargo outdated       - Check for outdated crates"
            echo "  flutter pub outdated - Check for outdated packages"
            echo ""
            
            # Set up git hooks if not already done
            if [ ! -f .git/hooks/pre-commit ]; then
              echo "Setting up git hooks..."
              mkdir -p .git/hooks
              cat > .git/hooks/pre-commit << 'EOF'
#!/usr/bin/env bash
set -e
echo "Running pre-commit checks..."
make lint
make test:rust
EOF
              chmod +x .git/hooks/pre-commit
              echo "✅ Git hooks installed"
            fi
            
            # Verify Android licenses
            if [ ! -d "$ANDROID_HOME/licenses" ]; then
              echo "⚠️  Android SDK licenses not accepted"
              echo "Run: flutter doctor --android-licenses"
            fi
            
            export PATH="$PWD/bin:$PATH"
          '';
        };

        # CI/CD shell (minimal, reproducible)
        devShells.ci = pkgs.mkShell {
          buildInputs = commonBuildInputs ++ [ androidSdk pkgs.jdk21 ];
          inherit shellEnv;
        };

        # Minimal shell for building only
        devShells.build = pkgs.mkShell {
          buildInputs = with pkgs; [
            rustVersion
            flutterPkg
            pkg-config
            cmake
            protobuf
          ] ++ darwinInputs ++ linuxInputs;
          
          inherit shellEnv;
        };

        # Formatter
        formatter = pkgs.nixpkgs-fmt;

        # Reproducible build helpers
        reproducibleBuildEnv = {
          # SOURCE_DATE_EPOCH for reproducible timestamps
          SOURCE_DATE_EPOCH = toString self.lastModified;
          
          # Disable network during build
          HOME = "/homeless-shelter";
          
          # Rust reproducibility
          RUSTFLAGS = "-C strip=none -C debuginfo=0 -C opt-level=3";
          CARGO_BUILD_INCREMENTAL = "false";
          
          # Flutter reproducibility
          FLUTTER_SUPPRESS_ANALYTICS = "true";
          DART_SUPPRESS_ANALYTICS = "true";
        };

        # Packages for each platform
        packages = rec {
          # Android APK (signed)
          android-apk = pkgs.stdenv.mkDerivation {
            pname = "pirate-unified-wallet-android";
            version = "1.0.4";
            
            src = ./.;
            
            buildInputs = commonBuildInputs ++ [ androidSdk pkgs.jdk21 ];
            
            buildPhase = ''
              export SOURCE_DATE_EPOCH=${toString self.lastModified}
              cd app
              flutter build apk --release
            '';
            
            installPhase = ''
              mkdir -p $out
              cp build/app/outputs/flutter-apk/app-release.apk $out/
            '';
          };
          
          # Android AAB (signed, Play Store ready)
          android-bundle = pkgs.stdenv.mkDerivation {
            pname = "pirate-unified-wallet-android-bundle";
            version = "1.0.4";
            
            src = ./.;
            
            buildInputs = commonBuildInputs ++ [ androidSdk pkgs.jdk21 ];
            
            buildPhase = ''
              export SOURCE_DATE_EPOCH=${toString self.lastModified}
              cd app
              flutter build appbundle --release
            '';
            
            installPhase = ''
              mkdir -p $out
              cp build/app/outputs/bundle/release/app-release.aab $out/
            '';
          };
          
          # Linux AppImage
          linux-appimage = pkgs.stdenv.mkDerivation {
            pname = "pirate-unified-wallet-linux";
            version = "1.0.4";
            
            src = ./.;
            
            buildInputs = commonBuildInputs ++ linuxInputs ++ [ pkgs.appimage-run ];
            
            buildPhase = ''
              export SOURCE_DATE_EPOCH=${toString self.lastModified}
              cd app
              flutter build linux --release
            '';
            
            installPhase = ''
              mkdir -p $out
              cp -r build/linux/x64/release/bundle $out/
            '';
          };
          
          # macOS DMG (universal binary)
          macos-dmg = pkgs.stdenv.mkDerivation {
            pname = "pirate-unified-wallet-macos";
            version = "1.0.4";
            
            src = ./.;
            
            buildInputs = commonBuildInputs ++ darwinInputs;
            
            buildPhase = ''
              export SOURCE_DATE_EPOCH=${toString self.lastModified}
              cd app
              flutter build macos --release
            '';
            
            installPhase = ''
              mkdir -p $out
              cp -r build/macos/Build/Products/Release/Pirate\ Unified\ Wallet.app $out/
            '';
          };
          
          # Windows MSIX
          windows-msix = pkgs.stdenv.mkDerivation {
            pname = "pirate-unified-wallet-windows";
            version = "1.0.4";
            
            src = ./.;
            
            buildInputs = commonBuildInputs;
            
            buildPhase = ''
              export SOURCE_DATE_EPOCH=${toString self.lastModified}
              cd app
              flutter build windows --release
            '';
            
            installPhase = ''
              mkdir -p $out
              cp -r build/windows/runner/Release $out/
            '';
          };
          
          # Default to Linux
          default = linux-appimage;
        };
      }
    );
}

