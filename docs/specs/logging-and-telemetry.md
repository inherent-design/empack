---
spec: logging-and-telemetry
status: draft
created: 2026-04-08
updated: 2026-04-08
depends: [overview]
---

# Logging and Telemetry

empack uses `tracing` with indicatif-aware output and an optional telemetry feature.

## Logger Initialization

`Logger::init()` builds the global subscriber once.

Base stack:

- `EnvFilter`
- formatting layer
- `IndicatifLayer`

Terminal color support comes from detected terminal capabilities, not from log format alone.

## Output Controls

Root config fields map directly to logger configuration.

| Field | Values |
| --- | --- |
| level | error, warning, info, debug, trace |
| format | text, json, yaml |
| output | stderr, stdout |

Log level filtering is built from the selected level unless `RUST_LOG` or other tracing env filters override it.

## Telemetry Feature Gate

Telemetry layers exist only when the crate is built with the `telemetry` feature.

`EMPACK_PROFILE` controls optional telemetry layers:

| Value | Effect |
| --- | --- |
| `chrome` | enable Chrome or Perfetto trace output |
| `otlp` | enable OTLP HTTP export |
| `all` | enable both layers |

If the feature is disabled, `EMPACK_PROFILE` has no telemetry effect.

## Layer Semantics

Current telemetry behavior:

- formatting and indicatif layers use the configured filter
- telemetry layers run unfiltered so instrumented spans remain visible
- separate `EnvFilter` instances are required because the filter is not cloneable
- the indicatif layer must be filtered the same way as the formatting layer to avoid progress-bar state mismatches

## Shutdown Semantics

`Logger::shutdown()` flushes telemetry providers.

Current shutdown actions:

- drop the Chrome flush guard so the writer thread flushes and joins
- call `shutdown_with_timeout(2s)` on the OTLP tracer provider

`global_shutdown()` is a safe no-op when the logger is not initialized.

## Process Lifecycle Integration

Startup and shutdown integration points:

- `CommandSession::build_live_session()` initializes the logger
- terminal interrupt handling calls `global_shutdown()`
- the library entry point also calls `logger::global_shutdown()` during process shutdown

This makes logging and telemetry part of the process lifecycle, not a standalone utility module.
