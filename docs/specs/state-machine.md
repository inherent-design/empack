---
spec: state-machine
status: draft
created: 2026-04-04
updated: 2026-04-08
depends: [overview, types]
---

# Pack State Machine

empack discovers project state from the working directory. The state machine does not persist a canonical state value beyond the interruption marker file used for build and clean recovery.

## Discovery Rules

Discovery order matters:

| Order | Condition | Result |
| --- | --- | --- |
| 1 | `.empack-state` exists and contains `building` or `cleaning` | `Interrupted { was: Building | Cleaning }` |
| 2 | `dist/` contains canonical build artifacts | `Built` |
| 3 | `empack.yml` exists or `pack/pack.toml` exists | `Configured` |
| 4 | none of the above | `Uninitialized` |

`validate_state()` is stricter than `discover_state()`. It checks the expected layout for the requested state, not just a minimal presence signal.

## Layout Rules

| State | Required layout when validated |
| --- | --- |
| `Uninitialized` | No validation beyond the transition rule |
| `Configured` | `empack.yml` plus `pack/pack.toml` |
| `Built` | Configured layout plus build artifacts under `dist/` |
| `Building` | Same layout as `Configured` |
| `Cleaning` | `dist/` directory exists |
| `Interrupted { was }` | Underlying `was` state validates after unwrapping nested interruptions |

## Orchestrated Transitions

```
Uninitialized -> Configured     Initialize
Configured -> Configured        RefreshIndex
Configured -> Built             Build
Built -> Built                  Build
Built -> Configured             Clean
Configured -> Uninitialized     Clean
Interrupted(Building) -> Built  Build
Interrupted(Building) -> Configured  RefreshIndex or Clean
Interrupted(Cleaning) -> Configured or Uninitialized  Clean recovery
```

`Initialize` from `Configured` is allowed only for progressive re-initialization. That path requires `empack.yml` without full pack metadata or build artifacts.

## Marker Transitions

Marker transitions are internal and always validate disk layout before entry.

| Marker | Allowed from | Meaning |
| --- | --- | --- |
| `Building` | `Configured`, `Built`, `Interrupted(Building)` | Build pipeline is in progress |
| `Cleaning` | `Built` | Build artifact cleanup is in progress |

If a marker remains on disk, the next discovery returns `Interrupted { was }`.

## Operation Semantics

### Initialize

`Initialize` creates the base directory structure, writes `empack.yml` when it does not already exist, and runs `packwiz init`.

Current behavior:

- `create_initial_structure()` creates `pack/`, `templates/`, and `dist/`.
- `execute_initialize()` writes a generated `empack.yml` only if the file is missing.
- `run_packwiz_init()` populates `pack/pack.toml` and related packwiz files.
- Failure during initialization cleans partial configuration where possible.
- Template scaffolding is not part of the pure state transition. Command handlers install templates after the transition succeeds.

### Refresh Index

`RefreshIndex` validates manifest consistency, emits warnings for mismatches, and then runs `packwiz refresh`.

The resulting state stays `Configured`.

### Build

`Build` delegates the actual work to `BuildOrchestrator::execute_build_pipeline()`. Marker writing and cleanup happen inside the build pipeline, not in the outer transition switch.

Successful build returns `Built`.

### Clean

`Clean` has two current behaviors:

| Starting state | Behavior | Result |
| --- | --- | --- |
| `Built` | Remove build artifacts from `dist/` | `Configured` |
| `Configured` | Remove `empack.yml` and `pack/` | `Uninitialized` |

Additional clean rules:

- `Clean` from `Uninitialized` is idempotent.
- `Clean` from `Interrupted { .. }` removes the marker first, then re-discovers the underlying filesystem state and cleans accordingly.
- `clean --cache` is a command-layer operation. It is not part of the `PackState` transition model.
