#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_FILE="${ROOT_DIR}/docs/internal/prototypes/browser_analysis_wasm_spike/web/analysis-client.test.mjs"

if ! command -v node >/dev/null 2>&1; then
  echo "error: node is required to run browser worker recovery tests."
  exit 1
fi

node --test "${TEST_FILE}"

echo "Browser analysis worker recovery tests passed."
