# EtherCAT EK1100/ELx008 Example (v1)

This project demonstrates the EtherCAT backend v1 using a Beckhoff-style
module chain profile:

- `EK1100` coupler
- `EL1008` (8 digital inputs)
- `EL2008` (8 digital outputs)

For deterministic local runs and CI, `io.toml` uses `adapter = "mock"`.

## Run

```bash
trust-runtime build --project examples/ethercat_ek1100_elx008_v1
trust-runtime validate --project examples/ethercat_ek1100_elx008_v1
trust-runtime --project examples/ethercat_ek1100_elx008_v1
```

## Hardware Bring-Up Note

This example intentionally uses `adapter = "mock"` for deterministic runs.
For hardware bring-up, switch `io.params.adapter` in `io.toml` to your EtherCAT
NIC name (for example `eth0`) and keep the physical module order aligned to the
configured `modules` chain.

## Scope Notes

- v1 focuses on process-image mapping for common digital I/O modules.
- No functional safety or SIL claims.
- No advanced motion profile support in v1.
