#!/usr/bin/env bash
set -euo pipefail

ST_RUNTIME="${ST_RUNTIME:-trust-runtime}"
PROJECT="${1:-docs/internal/testing/bundles/sample-runtime}"
DURATION_HOURS="${DURATION_HOURS:-24}"
INTERVAL_SEC="${INTERVAL_SEC:-60}"
OUT="${OUT:-runtime-soak-$(date +%Y%m%d_%H%M%S).log}"

echo "Starting runtime for soak test..."
"$ST_RUNTIME" play --project "$PROJECT" >"${OUT}.runtime.log" 2>&1 &
PID=$!

cleanup() {
  "$ST_RUNTIME" ctl --project "$PROJECT" shutdown >/dev/null 2>&1 || true
  kill "$PID" >/dev/null 2>&1 || true
}
trap cleanup EXIT

sleep 1
echo "# timestamp status cpu_pct mem_rss_kb" >"$OUT"

end=$(( $(date +%s) + DURATION_HOURS * 3600 ))
while [ "$(date +%s)" -lt "$end" ]; do
  ts="$(date --iso-8601=seconds)"
  status="$("$ST_RUNTIME" ctl --project "$PROJECT" status 2>/dev/null || echo "state=unknown")"
  cpu="$(ps -p "$PID" -o %cpu= | tr -d ' ')"
  rss="$(ps -p "$PID" -o rss= | tr -d ' ')"
  echo "$ts $status cpu=${cpu:-0} mem_rss_kb=${rss:-0}" >>"$OUT"
  sleep "$INTERVAL_SEC"
done

echo "Soak test complete. Log: $OUT"
