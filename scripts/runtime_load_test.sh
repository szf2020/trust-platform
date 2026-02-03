#!/usr/bin/env bash
set -euo pipefail

ST_RUNTIME="${ST_RUNTIME:-trust-runtime}"
PROJECT="${1:-docs/internal/testing/bundles/sample-runtime}"
DURATION="${DURATION:-30}"
INTERVAL="${INTERVAL:-1}"
OUT="${OUT:-runtime-load-$(date +%Y%m%d_%H%M%S).log}"

echo "Starting runtime for load test..."
"$ST_RUNTIME" play --project "$PROJECT" >"${OUT}.runtime.log" 2>&1 &
PID=$!

cleanup() {
  "$ST_RUNTIME" ctl --project "$PROJECT" shutdown >/dev/null 2>&1 || true
  kill "$PID" >/dev/null 2>&1 || true
}
trap cleanup EXIT

sleep 1
echo "Collecting task stats every ${INTERVAL}s for ${DURATION}s..."
echo "# timestamp task stats" >"$OUT"

end=$(( $(date +%s) + DURATION ))
while [ "$(date +%s)" -lt "$end" ]; do
  ts="$(date --iso-8601=seconds)"
  printf '%s ' "$ts" >>"$OUT"
  "$ST_RUNTIME" ctl --project "$PROJECT" stats >>"$OUT" || echo "stats=unavailable" >>"$OUT"
  sleep "$INTERVAL"
done

echo "Load test complete. Stats: $OUT"
