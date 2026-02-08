# External Salsa Integration Audit Prompt

You are an independent senior Rust compiler/performance auditor.

Repository:
- /home/johannes/projects/trust-platform
- Branch: spike/salsa-stage1-gate

Scope (STRICT):
Review only the Salsa integration/migration/hardening work. Ignore all unrelated changes (especially runtime UI/web/frontend).

Primary goals:
1. Find functional bugs/regressions.
2. Find memory-safety/leak risks and unbounded growth risks.
3. Find performance bottlenecks/regressions.
4. Validate correctness of Salsa 0.26 migration and query architecture.

Files in scope:
- Cargo.toml
- crates/trust-hir/Cargo.toml
- crates/trust-hir/src/db/mod.rs
- crates/trust-hir/src/db/queries.rs
- crates/trust-hir/src/db/queries/database.rs
- crates/trust-hir/src/db/queries/salsa_backend.rs
- crates/trust-hir/src/db/diagnostics/shared_globals.rs
- crates/trust-lsp/src/state/mod.rs
- crates/trust-lsp/src/handlers/diagnostics.rs
- crates/trust-lsp/src/handlers/features/core.rs
- crates/trust-lsp/src/handlers/features/completion.rs
- crates/trust-lsp/src/handlers/tests/core.rs
- crates/trust-lsp/src/handlers/tests/completion_hover.rs
- crates/trust-lsp/src/perf.rs
- scripts/salsa_hardening_perf_gate.sh
- scripts/salsa_memory_gate.sh
- scripts/salsa_fuzz_gate.sh
- scripts/salsa_miri_gate.sh
- .github/workflows/salsa-hardening.yml
- docs/reports/salsa-upgrade-report.md
- docs/specs/10-runtime.md (Salsa-related sections only)
- docs/diagrams/hir/hir-semantics.puml (Salsa-related semantics only)

Required validation commands:
- cargo fmt --all --check
- cargo clippy --workspace -- -D warnings
- cargo test --workspace
- ./scripts/salsa_spike_gate.sh
- ./scripts/salsa_hardening_perf_gate.sh compare
- ./scripts/salsa_memory_gate.sh compare
- ./scripts/salsa_fuzz_gate.sh smoke
- ./scripts/salsa_miri_gate.sh

Audit checklist (must evaluate explicitly):
- Salsa version is 0.26 and migration is correct (no legacy query_group assumptions).
- Query invalidation correctness:
  - source_revision/synced_revision logic
  - no stale results after edits/removals
  - no unnecessary full-project sync when unchanged
- Parse/cache correctness:
  - no raw reparse bypass where cached parse_green should be used
- Shared global hazard path:
  - no project-wide text clone regressions
  - no semantic drift from previous behavior
- Concurrency/locking:
  - no reentrant borrow panic path
  - no deadlock/livelock risk with Database-owned Salsa state
- Cancellation:
  - LSP request cancellation is cooperative, race-safe, and panic-free
  - salsa_event counters/events are meaningful and correctly wired
- Memory:
  - no obvious leaks
  - no unbounded map/vector growth
  - retained allocations are bounded/expected
- Performance:
  - identify current hotspots in analyze/diagnostics/type_of paths
  - confirm perf gate methodology and baseline-compare validity

Output format (mandatory):
1. Findings by severity (Critical/High/Medium/Low), each with:
   - file:line
   - exact problem
   - proof/repro steps
   - concrete fix proposal
2. "No findings" areas you explicitly verified.
3. Performance summary table:
   - baseline vs current (avg, p95, cpu/op, memory metrics)
   - pass/fail judgment
4. Final verdict:
   - Ship / Ship with conditions / Do not ship
5. Residual risks:
   - what is still not proven (e.g., limits of current Miri/fuzz coverage)
