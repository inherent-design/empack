# empack usage

## Navigation

- Project overview and current status: [`../README.md`](../README.md)
- Trusted verification matrix and exact rerun commands: [`testing/README.md`](testing/README.md)
- Contributor workflow and maintenance expectations: [`../CONTRIBUTING.md`](../CONTRIBUTING.md)

## Scope

This guide describes the current Rust CLI. It does not treat the Bash implementations in `v1/` or `v2/` as active product guidance.

## Project layout

- `empack.yml`: declared project configuration
- `pack/`: managed `packwiz` workspace
- `dist/`: canonical build artifact root

## Command overview

| Command | Purpose | Current verification note |
| --- | --- | --- |
| `empack requirements` | Check external tool availability | Trusted command path with dedicated test coverage |
| `empack version` | Print version information | CLI help verified |
| `empack init` | Initialize a project or complete partial setup | Trusted hermetic workflow path |
| `empack add` | Add projects by name, URL, or project ID | Trusted current workflow and command path |
| `empack sync` | Reconcile declared dependencies with installed state | Trusted in isolated reruns |
| `empack build` | Produce `mrpack` and other build targets | Trusted for promoted workflow suites |
| `empack remove` | Remove projects from the current modpack | Available and covered by targeted command tests |
| `empack clean` | Remove build outputs | Trusted current command and lifecycle path |

## Typical workflow

### Check local prerequisites

```bash
empack requirements
```

Verified by command help and `crates/empack-tests/tests/requirements_command.rs`.

### Initialize a project

```bash
empack init my-pack \
  --pack-name "My Pack" \
  --modloader fabric \
  --mc-version 1.21.1 \
  --author "Your Name" \
  -y
```

Trusted evidence:

- `crates/empack-tests/tests/init_workflows.rs`
- `crates/empack-tests/tests/lifecycle_forge_full.rs`

### Add a dependency

```bash
empack add sodium
```

Optional platform preference:

```bash
empack add jei --platform curseforge
```

Trusted evidence:

- `crates/empack-tests/tests/add_command.rs`
- add and sync parity coverage in the `empack-lib` command tests

### Reconcile declared and installed state

```bash
empack sync
```

Preview only:

```bash
empack sync --dry-run
```

Trusted evidence comes from isolated reruns of:

- `test_sync_workflow_full`
- `test_sync_dry_run_no_modifications`

See [`docs/testing/README.md`](testing/README.md) for the exact isolated rerun commands and the grouped-workflow caveat.

### Build artifacts

```bash
empack build mrpack
```

Build all configured targets after cleaning existing artifacts:

```bash
empack build --clean all
```

Current trusted coverage includes promoted suites for `mrpack`, `server`, `server-full`, `client-full`, and lifecycle-driven build flows. Build outputs are expected under the project-local `dist/` artifact root.

### Remove dependencies

```bash
empack remove sodium
```

Remove a project and then offer orphan cleanup logic:

```bash
empack remove sodium --deps
```

This command exists and is covered by targeted command tests in `crates/empack-lib/src/application/commands.test.rs`, but it is not currently described as a promoted workflow gate in the trusted matrix.

### Clean build artifacts

```bash
empack clean builds
```

`clean` targets the artifact tree under `dist/`. Current tests verify build cleanup behavior without treating `empack.yml` or pack metadata as disposable artifacts.

## Current caveats

### Known grouped-workflow caveat

Grouped reruns of `sync_workflow` can fail with:

`Global configuration already initialized`

For touched sync behavior, prefer isolated reruns instead of the grouped file run.

### Deferred gaps

- Standalone `client-full` missing-installer propagation remains deferred.
- Standalone `server-full` missing-installer propagation remains deferred.

### Historical context only

`v1/` and `v2/` remain useful when tracing product lineage, but they are not the release target and should not be used to describe the current CLI.