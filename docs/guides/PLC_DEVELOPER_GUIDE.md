# truST PLC Developer Guide

This guide is for automation engineers and developers building and deploying PLC project folders.
It assumes you already have the runtime installed.

## Project Layout (Structure)

A PLC project folder contains:

```
runtime.toml
io.toml
program.stbc
sources/
```

- `runtime.toml`: runtime configuration (tasks, control, web, watchdog, retain).
- `io.toml`: I/O driver config and safe-state outputs.
- `program.stbc`: compiled bytecode.
- `sources/`: Structured Text sources.

## Build Flow

Compile sources into bytecode:
```
trust-runtime build --project <project-folder>
```

Validate a project folder (config + bytecode):
```
trust-runtime validate --project <project-folder>
```

Start runtime:
```
trust-runtime --project <project-folder>
```

## Runtime Configuration (runtime.toml)

Key sections:

- `[resource]`: name + cycle time.
- `[runtime.control]`: control endpoint + debug gating.
- `[runtime.web]`: browser UI.
- `[runtime.discovery]`: local mDNS.
- `[runtime.mesh]`: runtime-to-runtime sharing.
- `[runtime.retain]`: retain store.
- `[runtime.watchdog]`: fault policy + safe halt.

## I/O Configuration (io.toml)

See `docs/guides/PLC_IO_BINDING_GUIDE.md` for full examples.

## Browser UI (Operations)

If enabled:
```
runtime.web.enabled = true
runtime.web.listen = "0.0.0.0:8080"
```

Open:
```
http://<device-ip>:8080
```

The browser UI is for operations: status, I/O, settings, deploy.

## Debug Attach (Development)

Debug is off in production mode by default. For development:
```
runtime.control.mode = "debug"
runtime.control.debug_enabled = true
```

Use the VS Code extension or `trust-runtime ctl` for stepping and breakpoints.

## Deploy + Rollback

Deploy a project folder into a versioned store:
```
trust-runtime deploy --project <project-folder> --root <deploy-root>
```

Rollback:
```
trust-runtime rollback --root <deploy-root>
```

## Local Discovery + Mesh

Enable local discovery:
```
runtime.discovery.enabled = true
```

Enable mesh sharing:
```
runtime.mesh.enabled = true
runtime.mesh.publish = ["Status.PLCState"]
[runtime.mesh.subscribe]
"RemoteA:Status.PLCState" = "Local.Status.RemoteState"
```

## Testing

Recommended checks: run the runtime reliability and GPIO hardware checklists before deployment.
