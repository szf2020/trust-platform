# OpenPLC Interop v1 Example (ST-focused)

This example is an end-to-end bundle for OpenPLC-origin PLCopen XML in the
current truST ST-only interoperability contract.

## What Is Included

- Native ST project source under `sources/`:
  - `main.st`: standard IEC edge logic (`R_TRIG`) that compiles and runs in truST.
- OpenPLC PLCopen XML import sample under `interop/`:
  - `openplc.xml`: includes `R_EDGE` alias usage + one unsupported `SFC` POU to
    exercise deterministic migration diagnostics.

## Scope (v1)

- In scope: ST project migration/import/export using PLCopen XML.
- Out of scope: graphical FBD/LD/SFC semantic import (unsupported by product
  decision in this phase).

## Preconditions

- `trust-runtime` is built and available in your shell.
- Run commands from repository root (`trust-platform/`).

## Flow A: Import OpenPLC XML -> truST Project

```bash
mkdir -p /tmp/trust-openplc-import
trust-runtime plcopen import \
  --input examples/openplc_interop_v1/interop/openplc.xml \
  --project /tmp/trust-openplc-import --json
```

Expected JSON/report indicators:

- `detected_ecosystem = "openplc"`
- `applied_library_shims` includes `R_EDGE -> R_TRIG`
- `unsupported_diagnostics` includes unsupported non-ST body classification for
  the fixture `SFC` POU
- migration report written to:
  - `/tmp/trust-openplc-import/interop/plcopen-migration-report.json`

## Flow B: Export Imported Project -> PLCopen XML

```bash
trust-runtime plcopen export \
  --project /tmp/trust-openplc-import \
  --output /tmp/trust-openplc-import/interop/roundtrip-openplc.xml --json
```

Expected artifacts:

- `/tmp/trust-openplc-import/interop/roundtrip-openplc.xml`
- `/tmp/trust-openplc-import/interop/roundtrip-openplc.source-map.json`

## Flow C: Export Native Source Bundle -> PLCopen XML

```bash
trust-runtime plcopen export \
  --project examples/openplc_interop_v1 \
  --output examples/openplc_interop_v1/interop/native-export.xml --json
```

This validates the reverse path (truST native ST -> PLCopen XML) in the same
sample bundle.

## Related Docs

- `docs/guides/OPENPLC_INTEROP_V1.md`
- `docs/guides/PLCOPEN_INTEROP_COMPATIBILITY.md`
- `docs/guides/VENDOR_LIBRARY_COMPATIBILITY.md`
