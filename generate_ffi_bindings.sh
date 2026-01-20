#!/bin/bash
# Generate Flutter Rust Bridge bindings for Pirate Unified Wallet
# This script sets up the environment and runs flutter_rust_bridge_codegen

set -e  # Exit on error

# Colors for output
BLUE='\033[0;34m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

echo -e "${BLUE}🔗 Generating Flutter Rust Bridge bindings...${NC}"

# Get the script directory and project root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$SCRIPT_DIR"
cd "$PROJECT_ROOT"

# Set up Flutter path
FLUTTER_PATH="${FLUTTER_PATH:-/mnt/c/src/flutter/bin}"
if [ ! -f "$FLUTTER_PATH/flutter" ]; then
    # Try alternative location
    FLUTTER_PATH="/mnt/c/Users/${USER}/flutter_windows_3.24.3-stable/flutter/bin"
fi

if [ ! -f "$FLUTTER_PATH/flutter" ]; then
    echo -e "${RED}❌ Flutter not found at expected paths${NC}"
    echo "   Tried: /mnt/c/src/flutter/bin/flutter"
    echo "   Tried: /mnt/c/Users/${USER}/flutter_windows_3.24.3-stable/flutter/bin/flutter"
    exit 1
fi

# Add Flutter to PATH
export PATH="$FLUTTER_PATH:$PATH"

# Verify Flutter is accessible
if ! command -v flutter &> /dev/null; then
    echo -e "${YELLOW}⚠️  Flutter not in PATH, creating symlink...${NC}"
    sudo ln -sf "$FLUTTER_PATH/flutter" /usr/local/bin/flutter 2>/dev/null || true
fi

# Verify flutter_rust_bridge_codegen is available
CARGO_BIN="${CARGO_HOME:-$HOME/.cargo}/bin"
FRB_CODEGEN="$CARGO_BIN/flutter_rust_bridge_codegen"
FRB_VERSION="${FRB_CODEGEN_VERSION:-2.11.1}"
if [ ! -f "$FRB_CODEGEN" ]; then
    echo -e "${YELLOW}⚠️  flutter_rust_bridge_codegen not found, installing...${NC}"
    cargo install flutter_rust_bridge_codegen --locked --version "$FRB_VERSION"
    FRB_CODEGEN="$CARGO_BIN/flutter_rust_bridge_codegen"
fi

# Verify Flutter works
echo -e "${BLUE}Checking Flutter installation...${NC}"
if ! flutter --version &> /dev/null; then
    echo -e "${RED}❌ Flutter command failed${NC}"
    exit 1
fi

# Check if Rust code compiles first
echo -e "${BLUE}Checking Rust code compiles...${NC}"
cd "$PROJECT_ROOT/crates"
if ! cargo check --package pirate-ffi-frb --features frb &> /dev/null; then
    echo -e "${YELLOW}⚠️  Rust code has compilation errors. Fixing them first...${NC}"
    cargo check --package pirate-ffi-frb --features frb
    echo -e "${RED}❌ Please fix Rust compilation errors before generating bindings${NC}"
    exit 1
fi
cd "$PROJECT_ROOT"

# Generate bindings using the root config file
echo -e "${BLUE}Generating FFI bindings...${NC}"
if [ -f "flutter_rust_bridge.yaml" ]; then
    CONFIG_FILE="flutter_rust_bridge.yaml"
    echo -e "${BLUE}Using config: $CONFIG_FILE${NC}"
    # Skip ffigen (LLVM not required for basic bindings)
    export FRB_SIMPLE_BUILD_SKIP=1
    # Run codegen and capture output, but don't fail on ffigen errors
    if ! "$FRB_CODEGEN" generate --config-file "$CONFIG_FILE" 2>&1 | tee /tmp/frb_output.log | grep -vE "(ffigen|LLVM|libclang|SEVERE|Couldn't find)" || true; then
        # Check if it's just an ffigen error
        if grep -q "ffigen\|LLVM" /tmp/frb_output.log && [ -f "app/lib/core/ffi/generated/api.dart" ]; then
            echo -e "${YELLOW}⚠️  ffigen failed (LLVM not found), but bindings were generated${NC}"
        fi
    fi
elif [ -f "crates/pirate-ffi-frb/frb.toml" ]; then
    CONFIG_FILE="crates/pirate-ffi-frb/frb.toml"
    echo -e "${BLUE}Using config: $CONFIG_FILE${NC}"
    export FRB_SIMPLE_BUILD_SKIP=1
    if ! "$FRB_CODEGEN" generate --config-file "$CONFIG_FILE" 2>&1 | tee /tmp/frb_output.log | grep -vE "(ffigen|LLVM|libclang|SEVERE|Couldn't find)" || true; then
        if grep -q "ffigen\|LLVM" /tmp/frb_output.log && [ -f "app/lib/core/ffi/generated/api.dart" ]; then
            echo -e "${YELLOW}⚠️  ffigen failed (LLVM not found), but bindings were generated${NC}"
        fi
    fi
else
    echo -e "${RED}❌ No FRB config file found${NC}"
    exit 1
fi

# Verify generated files exist
GENERATED_DIR="app/lib/core/ffi/generated"
mkdir -p "$GENERATED_DIR"

# Check if files were generated in the correct location
if [ -f "$GENERATED_DIR/api.dart" ] && [ -f "$GENERATED_DIR/frb_generated.dart" ]; then
    echo -e "${GREEN}✅ FFI bindings generated successfully!${NC}"
    echo -e "${GREEN}   Generated files in: $GENERATED_DIR${NC}"
    ls -lh "$GENERATED_DIR"/*.dart 2>/dev/null | awk '{print "   - " $9 " (" $5 ")"}' || true
else
    # Check for files in alternative locations (old config paths)
    OLD_LOCATION1="app/flutter/lib/ffi/bridge_generated.dart"
    OLD_LOCATION2="app/lib/ffi/bridge_generated.dart"
    
    if [ -d "$OLD_LOCATION1" ] && [ -f "$OLD_LOCATION1/api.dart" ]; then
        echo -e "${YELLOW}⚠️  Files generated in old location, copying to correct location...${NC}"
        cp -r "$OLD_LOCATION1"/* "$GENERATED_DIR/" 2>/dev/null || true
        echo -e "${GREEN}✅ Files copied to: $GENERATED_DIR${NC}"
    elif [ -d "$OLD_LOCATION2" ] && [ -f "$OLD_LOCATION2/api.dart" ]; then
        echo -e "${YELLOW}⚠️  Files generated in old location, copying to correct location...${NC}"
        cp -r "$OLD_LOCATION2"/* "$GENERATED_DIR/" 2>/dev/null || true
        echo -e "${GREEN}✅ Files copied to: $GENERATED_DIR${NC}"
    else
        echo -e "${RED}❌ Generated files not found${NC}"
        echo -e "${YELLOW}   Searching for generated files...${NC}"
        find . -name "api.dart" -type f 2>/dev/null | head -5
        exit 1
    fi
fi

# Final verification
if [ -f "$GENERATED_DIR/api.dart" ] && [ -f "$GENERATED_DIR/frb_generated.dart" ]; then
    echo -e "${GREEN}✅ FFI bindings ready in: $GENERATED_DIR${NC}"
    ls -lh "$GENERATED_DIR"/*.dart 2>/dev/null | awk '{print "   - " $9 " (" $5 ")"}' || true
else
    echo -e "${RED}❌ Verification failed: files still missing${NC}"
    exit 1
fi

echo -e "${GREEN}✅ Done!${NC}"



