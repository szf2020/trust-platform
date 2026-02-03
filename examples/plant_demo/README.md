# Plant Demo (Structured Text)

This example is a compact IEC 61131-3 Structured Text project designed to exercise multi-file symbol resolution and runtime configuration.

## Structure
- `src/types.st`: Shared enums and structs.
- `src/fb_pump.st`: Function block using the shared types.
- `src/program.st`: Program that instantiates the function block.
- `src/config.st`: Configuration with task and I/O bindings.

## Features Covered
- TYPE declarations: enums and structs.
- FUNCTION_BLOCK definition with a timer (TON).
- PROGRAM that instantiates and calls a function block.
- Configuration with a task and I/O bindings.
- Control flow: IF/ELSE and CASE.
- Enum literals are written as typed literals (e.g., `E_PumpState#Fault`).

## Usage
1. Open this folder in VS Code.
2. Open `src/program.st` to explore the sample.
3. The files should be free of diagnostics with the current trust-lsp.

## Test Plan
1. Open the four files and confirm no diagnostics are reported.
2. Use Go to Definition from `FB_Pump` in `src/program.st` to `src/fb_pump.st`.
3. Hover `E_PumpState` or `ST_PumpCommand` in `src/program.st` and confirm the type info.
4. Run `cargo test -p trust-runtime plant_demo_configuration_binds_io_and_tasks` to verify the configuration compiles.
