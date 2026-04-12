---
spec: state-machine
status: stale
created: 2026-04-04
updated: 2026-04-11
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
| 3 | `empack.yml` exists and `pack/pack.toml` exists | `Configured` |
| 4 | none of the above | `Uninitialized` |

Directories with only one core metadata artifact are treated as `Uninitialized` for state discovery. Command handlers may still surface those layouts as incomplete initialization rather than as generic non-project directories.

`validate_state()` checks the expected layout for the requested state.

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
Configured -> Configured        Clean
Interrupted(Building) -> Built  Build
Interrupted(Building) -> Configured  RefreshIndex or Clean
Interrupted(Cleaning) -> Configured or Uninitialized  Clean recovery
```

`Initialize` is the pure `Uninitialized -> Configured` transition. `init --force` performs an explicit command-layer reset of project core files before calling the state transition.

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
- `init --force` resets `empack.yml`, `pack/`, markers, and `dist/` explicitly before initialization. That reset is not part of the `Clean` state transition.

### Refresh Index

`RefreshIndex` validates manifest consistency, emits warnings for mismatches, and then runs `packwiz refresh`.

The resulting state stays `Configured`.

### Build

`Build` delegates the actual work to `BuildOrchestrator::execute_build_pipeline()`. Marker writing and cleanup happen inside the build pipeline, not in the outer transition switch.

Successful build returns `Built`.

### Clean

`Clean` is a non-destructive, idempotent build-artifact cleanup transition.

| Starting state | Behavior | Result |
| --- | --- | --- |
| `Built` | Remove build artifacts from `dist/` | `Configured` |
| `Configured` | Remove build artifacts from `dist/` if present | `Configured` |
| `Uninitialized` | Remove stray build artifacts from `dist/` if present | `Uninitialized` |

Additional clean rules:

- `Clean` from `Uninitialized` is idempotent.
- `Clean` from `Interrupted { .. }` removes the marker first, removes `dist/` if present, then re-discovers the underlying filesystem state.
- `Clean` never removes `empack.yml`.
- `Clean` never removes `pack/`.
- `clean --cache` is a command-layer operation. It is not part of the `PackState` transition model.
