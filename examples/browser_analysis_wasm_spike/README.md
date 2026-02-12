# Browser Analysis WASM Spike (Deliverable 10)

This example validates a worker-based browser transport model for
`trust-wasm-analysis` diagnostics, hover, and completion.

## Scope

- Static analysis only (diagnostics, hover, completion).
- Worker transport with request IDs, timeout handling, and cancellation tokens.
- No runtime/debug adapter integration in this spike.

## Prerequisites

- Rust toolchain.
- `wasm-pack` installed:

```bash
cargo install wasm-pack
```

## Build

From repository root:

```bash
scripts/build_browser_analysis_wasm_spike.sh
```

The script builds:
- WASM package into `target/browser-analysis-wasm/pkg/`
- browser host files into `target/browser-analysis-wasm/web/`

## Run

```bash
python3 -m http.server 4173 --directory target/browser-analysis-wasm/web
```

Open:

`http://127.0.0.1:4173/`

## Protocol Contract (Worker)

Request:

```json
{
  "id": "req-1",
  "method": "diagnostics",
  "params": { "uri": "memory:///main.st" },
  "timeoutMs": 1500
}
```

Response success:

```json
{
  "id": "req-1",
  "result": []
}
```

Response failure:

```json
{
  "id": "req-1",
  "error": {
    "code": "engine_error",
    "message": "document 'memory:///main.st' is not loaded"
  }
}
```

Cancellation:

```json
{
  "method": "cancel",
  "params": { "requestId": "req-1" }
}
```
