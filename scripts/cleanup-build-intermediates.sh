#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

log() {
    echo -e "${GREEN}[$(date +'%Y-%m-%d %H:%M:%S')]${NC} $1"
}

warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

show_disk() {
    if command -v df >/dev/null 2>&1; then
        df -h "$PROJECT_ROOT" || true
    fi
}

CLEAN_RUST_TARGET="${CLEAN_RUST_TARGET:-0}"

remove_dir_if_exists() {
    local dir="$1"
    local label="$2"
    if [[ -d "$dir" ]]; then
        local size="unknown"
        if command -v du >/dev/null 2>&1; then
            size="$(du -sh "$dir" 2>/dev/null | awk '{print $1}')"
        fi
        rm -rf "$dir"
        log "Removed $label ($size): $dir"
    fi
}

log "Reclaiming workspace disk (preserving dist artifacts)..."
log "Disk usage before cleanup:"
show_disk

# Safe to remove after packaging; artifacts are already copied to dist/.
# Keep Rust target by default so SBOM can inspect existing build outputs.
if [[ "$CLEAN_RUST_TARGET" == "1" ]]; then
    remove_dir_if_exists "$PROJECT_ROOT/crates/target" "Rust build artifacts"
else
    log "Preserving Rust build artifacts: $PROJECT_ROOT/crates/target"
fi
remove_dir_if_exists "$PROJECT_ROOT/app/build" "Flutter build output"
remove_dir_if_exists "$PROJECT_ROOT/app/.dart_tool" "Dart tool cache"
remove_dir_if_exists "$PROJECT_ROOT/app/android/.gradle" "Android Gradle cache"

if [[ ! -d "$PROJECT_ROOT/dist" ]]; then
    warn "dist/ directory not found; ensure packaging copied outputs before cleanup."
fi

log "Disk usage after cleanup:"
show_disk
log "Workspace cleanup complete."
