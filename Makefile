.PHONY: help bootstrap build build\:all build\:rust build\:android build\:ios build\:desktop \
        test test\:all test\:rust test\:flutter test\:integration \
        lint format clean ci \
        audit security \
        run run\:android run\:ios run\:desktop

.DEFAULT_GOAL := help

# Colors for output
BLUE := \033[0;34m
GREEN := \033[0;32m
YELLOW := \033[1;33m
RED := \033[0;31m
NC := \033[0m # No Color

# Project directories
RUST_DIR := crates
APP_DIR := app
CI_DIR := .github/workflows

# Detect OS
ifeq ($(OS),Windows_NT)
    DETECTED_OS := Windows
    FLUTTER := flutter.bat
    CARGO := cargo.exe
else
    DETECTED_OS := $(shell uname -s)
    FLUTTER := flutter
    CARGO := cargo
endif

##@ General

help: ## Display this help message
	@echo "$(BLUE)🏴‍☠️ Pirate Unified Wallet - Makefile$(NC)"
	@echo ""
	@awk 'BEGIN {FS = ":.*##"; printf "Usage:\n  make $(YELLOW)<target>$(NC)\n"} /^[a-zA-Z_0-9\:-]+:.*?##/ { printf "  $(GREEN)%-20s$(NC) %s\n", $$1, $$2 } /^##@/ { printf "\n$(BLUE)%s$(NC)\n", substr($$0, 5) } ' $(MAKEFILE_LIST)

##@ Setup

bootstrap: ## Bootstrap development environment
	@echo "$(BLUE)🔧 Bootstrapping development environment...$(NC)"
	@echo "$(YELLOW)Detected OS: $(DETECTED_OS)$(NC)"
	@$(MAKE) --no-print-directory bootstrap-check
	@$(MAKE) --no-print-directory bootstrap-rust
	@$(MAKE) --no-print-directory bootstrap-flutter
	@echo "$(GREEN)✅ Bootstrap complete!$(NC)"

bootstrap-check: ## Check required tools are installed
	@echo "$(BLUE)Checking required tools...$(NC)"
	@command -v $(CARGO) >/dev/null 2>&1 || { echo "$(RED)❌ Rust not found. Install from https://rustup.rs$(NC)"; exit 1; }
	@command -v $(FLUTTER) >/dev/null 2>&1 || { echo "$(RED)❌ Flutter not found. Install from https://flutter.dev$(NC)"; exit 1; }
	@command -v protoc >/dev/null 2>&1 || { echo "$(RED)❌ protoc not found. Install protocol buffers compiler$(NC)"; exit 1; }
	@echo "$(GREEN)✅ All required tools found$(NC)"
	@echo ""
	@rustc --version
	@$(CARGO) --version
	@$(FLUTTER) --version | head -n1
	@protoc --version

bootstrap-rust: ## Install Rust dependencies and tools
	@echo "$(BLUE)Setting up Rust environment...$(NC)"
	@rustup component add clippy rustfmt rust-src
	@$(CARGO) install cargo-audit cargo-deny cargo-edit || true
	@echo "$(GREEN)✅ Rust environment ready$(NC)"

bootstrap-flutter: ## Install Flutter dependencies
	@echo "$(BLUE)Setting up Flutter environment...$(NC)"
	@$(FLUTTER) doctor
	@cd $(APP_DIR) && $(FLUTTER) pub get
	@echo "$(GREEN)✅ Flutter environment ready$(NC)"

##@ Codegen

frb: ## Generate flutter_rust_bridge bindings
	@echo "$(BLUE)🔗 Generating flutter_rust_bridge bindings...$(NC)"
	@command -v flutter_rust_bridge_codegen >/dev/null 2>&1 || $(CARGO) install flutter_rust_bridge_codegen --locked
	@flutter_rust_bridge_codegen generate --config-file crates/pirate-ffi-frb/frb.toml
	@echo "$(GREEN)✅ FRB bindings generated$(NC)"

params: ## Pre-warm proving parameters (Sapling/Orchard) - embedded loader
	@echo "$(BLUE)📦 Ensuring proving parameters are available...$(NC)"
	@echo "Sapling params are embedded via wagyu-zcash-parameters; Orchard keys are built in-memory."
	@echo "$(GREEN)✅ Params ready (no download needed)$(NC)"

##@ Building

build\:all: frb build\:rust build\:android build\:ios build\:desktop ## Build all targets (platform-dependent)

build: build\:rust ## Build Rust core libraries
	@echo "$(GREEN)✅ Build complete$(NC)"

build\:rust: ## Build Rust workspace
	@echo "$(BLUE)🦀 Building Rust workspace...$(NC)"
	@cd $(RUST_DIR) && $(CARGO) build --release --all-features
	@echo "$(GREEN)✅ Rust build complete$(NC)"

build\:android: ## Build Android app
	@echo "$(BLUE)🤖 Building Android app...$(NC)"
	@cd $(APP_DIR) && $(FLUTTER) build apk --release
	@cd $(APP_DIR) && $(FLUTTER) build appbundle --release
	@echo "$(GREEN)✅ Android build complete$(NC)"
	@echo "APK: $(APP_DIR)/build/app/outputs/flutter-apk/app-release.apk"
	@echo "AAB: $(APP_DIR)/build/app/outputs/bundle/release/app-release.aab"

build\:ios: ## Build iOS app (macOS only)
ifeq ($(DETECTED_OS),Darwin)
	@echo "$(BLUE)🍎 Building iOS app...$(NC)"
	@cd $(APP_DIR) && $(FLUTTER) build ios --release --no-codesign
	@echo "$(GREEN)✅ iOS build complete$(NC)"
else
	@echo "$(RED)❌ iOS builds require macOS$(NC)"
	@exit 1
endif

build\:desktop: ## Build desktop app (platform-dependent)
ifeq ($(DETECTED_OS),Linux)
	@echo "$(BLUE)🐧 Building Linux desktop app...$(NC)"
	@cd $(APP_DIR) && $(FLUTTER) build linux --release
	@echo "$(GREEN)✅ Linux build complete$(NC)"
else ifeq ($(DETECTED_OS),Darwin)
	@echo "$(BLUE)🍎 Building macOS desktop app...$(NC)"
	@cd $(APP_DIR) && $(FLUTTER) build macos --release
	@echo "$(GREEN)✅ macOS build complete$(NC)"
else ifeq ($(DETECTED_OS),Windows)
	@echo "$(BLUE)🪟 Building Windows desktop app...$(NC)"
	@cd $(APP_DIR) && $(FLUTTER) build windows --release
	@echo "$(GREEN)✅ Windows build complete$(NC)"
else
	@echo "$(RED)❌ Unsupported OS for desktop builds$(NC)"
	@exit 1
endif

##@ Testing

test\:all: test\:rust test\:flutter test\:integration ## Run all tests

test: test\:rust ## Run Rust tests

test\:rust: ## Run Rust unit and integration tests
	@echo "$(BLUE)🦀 Running Rust tests...$(NC)"
	@cd $(RUST_DIR) && $(CARGO) test --all-features --workspace
	@echo "$(GREEN)✅ Rust tests passed$(NC)"

test\:flutter: ## Run Flutter tests
	@echo "$(BLUE)🎯 Running Flutter tests...$(NC)"
	@cd $(APP_DIR) && $(FLUTTER) test --coverage
	@echo "$(GREEN)✅ Flutter tests passed$(NC)"

test\:integration: ## Run integration tests
	@echo "$(BLUE)🔗 Running integration tests...$(NC)"
	@cd $(APP_DIR) && $(FLUTTER) test integration_test/
	@echo "$(GREEN)✅ Integration tests passed$(NC)"

##@ Code Quality

lint: lint\:rust lint\:flutter ## Run all linters

lint\:rust: ## Run Rust linter (clippy)
	@echo "$(BLUE)🦀 Running Rust linter...$(NC)"
	@cd $(RUST_DIR) && $(CARGO) clippy --all-features --workspace -- -D warnings
	@echo "$(GREEN)✅ Rust linting passed$(NC)"

lint\:flutter: ## Run Flutter analyzer
	@echo "$(BLUE)🎯 Running Flutter analyzer...$(NC)"
	@cd $(APP_DIR) && $(FLUTTER) analyze
	@echo "$(GREEN)✅ Flutter analysis passed$(NC)"

format: format\:rust format\:flutter ## Format all code

format\:rust: ## Format Rust code
	@echo "$(BLUE)🦀 Formatting Rust code...$(NC)"
	@cd $(RUST_DIR) && $(CARGO) fmt --all
	@echo "$(GREEN)✅ Rust code formatted$(NC)"

format\:flutter: ## Format Flutter/Dart code
	@echo "$(BLUE)🎯 Formatting Flutter code...$(NC)"
	@cd $(APP_DIR) && dart format .
	@echo "$(GREEN)✅ Flutter code formatted$(NC)"

##@ Security

audit: ## Run security audit on dependencies
	@echo "$(BLUE)🔒 Running security audit...$(NC)"
	@cd $(RUST_DIR) && $(CARGO) audit
	@cd $(RUST_DIR) && $(CARGO) deny check
	@echo "$(GREEN)✅ Security audit passed$(NC)"

security: audit ## Alias for audit

##@ CI/CD

ci: ## Run all CI checks locally
	@echo "$(BLUE)🚀 Running CI checks...$(NC)"
	@$(MAKE) --no-print-directory bootstrap-check
	@$(MAKE) --no-print-directory frb
	@$(MAKE) --no-print-directory format
	@$(MAKE) --no-print-directory lint
	@$(MAKE) --no-print-directory audit
	@$(MAKE) --no-print-directory test\:rust
	@$(MAKE) --no-print-directory test\:flutter
	@$(MAKE) --no-print-directory build\:rust
	@echo "$(GREEN)✅ All CI checks passed$(NC)"

##@ Running

run: ## Run Flutter app in development mode
	@echo "$(BLUE)🚀 Running app...$(NC)"
	@cd $(APP_DIR) && $(FLUTTER) run

run\:android: ## Run on Android device/emulator
	@echo "$(BLUE)🤖 Running on Android...$(NC)"
	@cd $(APP_DIR) && $(FLUTTER) run -d android

run\:ios: ## Run on iOS device/simulator (macOS only)
ifeq ($(DETECTED_OS),Darwin)
	@echo "$(BLUE)🍎 Running on iOS...$(NC)"
	@cd $(APP_DIR) && $(FLUTTER) run -d ios
else
	@echo "$(RED)❌ iOS requires macOS$(NC)"
	@exit 1
endif

run\:desktop: ## Run on desktop (platform-dependent)
ifeq ($(DETECTED_OS),Linux)
	@echo "$(BLUE)🐧 Running on Linux...$(NC)"
	@cd $(APP_DIR) && $(FLUTTER) run -d linux
else ifeq ($(DETECTED_OS),Darwin)
	@echo "$(BLUE)🍎 Running on macOS...$(NC)"
	@cd $(APP_DIR) && $(FLUTTER) run -d macos
else ifeq ($(DETECTED_OS),Windows)
	@echo "$(BLUE)🪟 Running on Windows...$(NC)"
	@cd $(APP_DIR) && $(FLUTTER) run -d windows
else
	@echo "$(RED)❌ Unsupported OS$(NC)"
	@exit 1
endif

##@ Cleanup

clean: clean\:rust clean\:flutter ## Clean all build artifacts

clean\:rust: ## Clean Rust build artifacts
	@echo "$(BLUE)🧹 Cleaning Rust build...$(NC)"
	@cd $(RUST_DIR) && $(CARGO) clean
	@echo "$(GREEN)✅ Rust build cleaned$(NC)"

clean\:flutter: ## Clean Flutter build artifacts
	@echo "$(BLUE)🧹 Cleaning Flutter build...$(NC)"
	@cd $(APP_DIR) && $(FLUTTER) clean
	@echo "$(GREEN)✅ Flutter build cleaned$(NC)"

distclean: clean ## Deep clean (including dependencies)
	@echo "$(BLUE)🧹 Deep cleaning...$(NC)"
	@rm -rf $(APP_DIR)/.dart_tool
	@rm -rf $(APP_DIR)/build
	@rm -rf $(RUST_DIR)/target
	@echo "$(GREEN)✅ Deep clean complete$(NC)"

##@ Documentation

docs: docs\:rust ## Generate documentation

docs\:rust: ## Generate Rust documentation
	@echo "$(BLUE)📚 Generating Rust documentation...$(NC)"
	@cd $(RUST_DIR) && $(CARGO) doc --all-features --no-deps --open
	@echo "$(GREEN)✅ Documentation generated$(NC)"

##@ Utilities

check-updates: ## Check for outdated dependencies
	@echo "$(BLUE)🔍 Checking for updates...$(NC)"
	@cd $(RUST_DIR) && $(CARGO) outdated
	@cd $(APP_DIR) && $(FLUTTER) pub outdated

version: ## Show version information
	@echo "$(BLUE)📦 Version Information$(NC)"
	@echo "Rust:    $$(rustc --version)"
	@echo "Cargo:   $$($(CARGO) --version)"
	@echo "Flutter: $$($(FLUTTER) --version | head -n1)"
	@echo "Dart:    $$(dart --version 2>&1)"
	@echo "OS:      $(DETECTED_OS)"

tree: ## Show project structure
	@echo "$(BLUE)📁 Project Structure$(NC)"
	@tree -L 3 -I 'target|build|node_modules|.dart_tool' || echo "Install 'tree' command for directory visualization"

