#!/usr/bin/env bash
set -euo pipefail

# Guard script for witness/anchor invariants.
# Fails fast if forbidden legacy/query paths reappear.

if ! command -v rg >/dev/null 2>&1; then
  echo "error: ripgrep (rg) is required" >&2
  exit 2
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_FILE="${REPO_ROOT}/crates/pirate-storage-sqlite/src/repository.rs"
SPENDABILITY_FILE="${REPO_ROOT}/crates/pirate-storage-sqlite/src/spendability_state.rs"

if [[ ! -f "${REPO_FILE}" ]]; then
  echo "error: repository file missing: ${REPO_FILE}" >&2
  exit 2
fi

if [[ ! -f "${SPENDABILITY_FILE}" ]]; then
  echo "error: spendability file missing: ${SPENDABILITY_FILE}" >&2
  exit 2
fi

forbidden_patterns=(
  "temp_v_sapling_shard_unscanned_ranges"
  "temp_v_orchard_shard_unscanned_ranges"
  "temp_candidate_note_scope"
  "sapling_tip_unscanned"
  "orchard_tip_unscanned"
  "queue only the note's own height window"
)

for pattern in "${forbidden_patterns[@]}"; do
  if rg -n --fixed-strings "${pattern}" "${REPO_FILE}" >/dev/null; then
    echo "forbidden witness/anchor pattern found in repository.rs: ${pattern}" >&2
    exit 1
  fi
done

if ! rg -n --fixed-strings "fn check_witnesses(" "${REPO_FILE}" >/dev/null; then
  echo "required entrypoint missing: fn check_witnesses(" >&2
  exit 1
fi

if ! rg -n --fixed-strings "subtree-derived" "${REPO_FILE}" >/dev/null; then
  echo "required queueing model marker missing: subtree-derived" >&2
  exit 1
fi

if ! rg -n --fixed-strings "self.scan_queue_extrema()?" "${SPENDABILITY_FILE}" >/dev/null; then
  echo "required canonical anchor derivation missing: self.scan_queue_extrema()?" >&2
  exit 1
fi

if rg -n --fixed-strings "derive_chain_tip_height(" "${SPENDABILITY_FILE}" >/dev/null; then
  echo "forbidden tip shortcut found in spendability derivation: derive_chain_tip_height(" >&2
  exit 1
fi

echo "witness/anchor forbidden-path guard passed"
