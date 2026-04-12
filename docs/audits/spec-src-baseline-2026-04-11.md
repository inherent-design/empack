# Recursive Spec/Source Baseline Audit

Baseline date: `2026-04-11`

Scope: full-stack baseline only. This audit maps public UX contracts, draft specs, runtime seams, and proof coverage across the current repo state. It does not patch code, rewrite specs, or reinterpret explicit deferrals as bugs.

## 1. Title and scope

This audit covers:

- Public docs: [README.md](/Users/zer0cell/production/empack/README.md:5), [docs/usage.md](/Users/zer0cell/production/empack/docs/usage.md:17), [docs/testing.md](/Users/zer0cell/production/empack/docs/testing.md:3)
- Draft specs: every file under [docs/specs/](/Users/zer0cell/production/empack/docs/specs/overview.md:1)
- CLI and command entrypoints: [application/cli.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/cli.rs:7), [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:32), [lib.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/lib.rs:53)
- Runtime seams: [application/session.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/session.rs:249), [empack/state.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/empack/state.rs:98), [empack/builds.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/empack/builds.rs:1433), [application/sync.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/sync.rs:8), [networking/mod.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/networking/mod.rs:53), [platform/packwiz_bin.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/platform/packwiz_bin.rs:15), [logger/mod.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/logger/mod.rs:42), [display/mod.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/display/mod.rs:10), [terminal/capabilities.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/terminal/capabilities.rs:4), [api/dependency_graph.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/api/dependency_graph.rs:17), [application/session_security.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/session_security.rs:1)
- Proof layer: [application/commands.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.test.rs:112), [empack/state.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/empack/state.test.rs:804), [empack/search.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/empack/search.test.rs:129), [api/dependency_graph.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/api/dependency_graph.test.rs:75), [display/display.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/display/display.test.rs:57), [terminal/capabilities.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/terminal/capabilities.test.rs:75), [crates/empack-tests/tests/build_continue.rs](/Users/zer0cell/production/empack/crates/empack-tests/tests/build_continue.rs:9), [crates/empack-tests/tests/e2e_restricted_build.rs](/Users/zer0cell/production/empack/crates/empack-tests/tests/e2e_restricted_build.rs:62), [crates/empack-tests/tests/clean_command.rs](/Users/zer0cell/production/empack/crates/empack-tests/tests/clean_command.rs:19), [crates/empack-tests/tests/sync_workflow.rs](/Users/zer0cell/production/empack/crates/empack-tests/tests/sync_workflow.rs:33)

## 2. Contract arbitration rules

- Contract precedence is decided per issue, not globally.
- For user-visible behavior, mature public docs and observed CLI shape outrank draft internal specs when those public artifacts are more complete.
- For internal seams, draft specs can win when they are clearly more complete and source/tests are lagging.
- When a draft spec is stale or contradicted by stronger source plus proof, source plus proof wins.
- When only one artifact exists, the default classification is usually `Undefined` unless that artifact is clearly the stable public contract.
- Green tests are evidence, not automatic truth.
- [docs/specs/overview.md](/Users/zer0cell/production/empack/docs/specs/overview.md:58) provides a broad “live source first” guideline, but this audit intentionally uses finer-grained arbitration because the repo is in mixed draft, partial, and deferred states.

## 3. Top-level findings summary

### Alignment checkpoint

`A-RESTRICTED-CONTINUE-001`

- Status: `Aligned`
- Winning contract: public docs plus CLI/docs specs for restricted-download continuation
- Why it matters: this is the strongest end-to-end user workflow in the current repo, with consistent public docs, runtime behavior, and proof
- Evidence:
  - Public docs: [README.md](/Users/zer0cell/production/empack/README.md:19), [docs/usage.md](/Users/zer0cell/production/empack/docs/usage.md:146)
  - Draft specs: [docs/specs/cli-surface.md](/Users/zer0cell/production/empack/docs/specs/cli-surface.md:99), [docs/specs/build-and-distribution.md](/Users/zer0cell/production/empack/docs/specs/build-and-distribution.md:91)
  - Runtime: [application/cli.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/cli.rs:131), [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:2828), [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:2981), [empack/restricted_build.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/empack/restricted_build.rs:89)
  - Proof: [crates/empack-tests/tests/build_continue.rs](/Users/zer0cell/production/empack/crates/empack-tests/tests/build_continue.rs:9), [crates/empack-tests/tests/e2e_restricted_build.rs](/Users/zer0cell/production/empack/crates/empack-tests/tests/e2e_restricted_build.rs:62)

### Finding register

| ID | Category | Severity | Summary |
| --- | --- | --- | --- |
| `F-CLI-CLEAN-001` | `Miswired` | `S1` | `README.md` says bare `empack clean` removes cache, but docs/specs/runtime/tests agree it defaults to `builds` only. |
| `F-CLI-EXIT-001` | `Undefined` | `S1` | CLI exit semantics are only explicit for Ctrl+C `130`; broader error-code behavior is not a stable contract. |
| `F-CONFIG-DEFAULT-001` | `Miswired` | `S2` | Programmatic `AppConfig::default()` does not match the CLI’s documented built-in CurseForge key default. |
| `F-SESSION-MOCK-001` | `Unwired` | `S2` | Test-only session scaffolding leaves `archive()`, `packwiz()`, and `state()` unimplemented despite the broader session contract. |
| `F-NET-RES-MGR-001` | `Unwired` | `S2` | `NetworkingManager` exists and is spec-described, but live command paths still bypass it. |
| `F-DISPLAY-TERM-001` | `Undefined` | `S2` | `display/*` and `terminal/*` are live, tested subsystems with no dedicated spec contract. |
| `F-DEPGRAPH-001` | `Undefined` | `S2` | The dependency graph API is live in add/remove flows and tested, but has no dedicated spec. |
| `F-SESSION-SEC-001` | `Undefined` | `S2` | Filesystem security expectations are under-specified, and the current tests overstate what the live provider proves. |
| `F-STATE-INIT-001` | `Miswired` | `S3` | `state-machine.md` overstates what pure initialization creates and uses stale function naming. |

## 4. Findings by subsystem

### CLI surface

Contract sources:

- [docs/usage.md](/Users/zer0cell/production/empack/docs/usage.md:17)
- [docs/specs/cli-surface.md](/Users/zer0cell/production/empack/docs/specs/cli-surface.md:9)
- [application/cli.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/cli.rs:7)

Runtime sources:

- [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:32)
- [lib.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/lib.rs:63)
- [application/session.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/session.rs:865)

Proof sources:

- [application/commands.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.test.rs:114)
- [crates/empack-tests/tests/e2e_version.rs](/Users/zer0cell/production/empack/crates/empack-tests/tests/e2e_version.rs:4)

Baseline:

- Command parsing and root option shape are mostly consistent across docs, clap, and runtime.
- The main user-visible drift is on `clean`.
- Exit behavior is only partially defined.

#### `F-CLI-CLEAN-001`

- ID: `F-CLI-CLEAN-001`
- Category: `Miswired`
- Severity: `S1`
- Winning contract: [docs/usage.md](/Users/zer0cell/production/empack/docs/usage.md:209), [docs/specs/cli-surface.md](/Users/zer0cell/production/empack/docs/specs/cli-surface.md:182), and runtime plus proof in [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:3396) and [crates/empack-tests/tests/clean_command.rs](/Users/zer0cell/production/empack/crates/empack-tests/tests/clean_command.rs:70)
- Conflicting sources: [README.md](/Users/zer0cell/production/empack/README.md:47) and [README.md](/Users/zer0cell/production/empack/README.md:58)
- Why this classification is correct: the stronger user-facing contract is the usage guide plus CLI spec because both match the runtime and tests. `README.md` is the only artifact claiming bare `empack clean` removes cache too.
- User/runtime impact: users following the README may expect cache eviction and get only build cleanup.
- Fix direction: align `README.md` with current runtime, or change runtime plus tests if cache-by-default is the intended UX.
- Evidence:
  - Contract: [README.md](/Users/zer0cell/production/empack/README.md:47), [README.md](/Users/zer0cell/production/empack/README.md:58), [docs/usage.md](/Users/zer0cell/production/empack/docs/usage.md:209), [docs/specs/cli-surface.md](/Users/zer0cell/production/empack/docs/specs/cli-surface.md:196)
  - Runtime: [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:3396)
  - Proof: [application/commands.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.test.rs:5443), [crates/empack-tests/tests/clean_command.rs](/Users/zer0cell/production/empack/crates/empack-tests/tests/clean_command.rs:70)

#### `F-CLI-EXIT-001`

- ID: `F-CLI-EXIT-001`
- Category: `Undefined`
- Severity: `S1`
- Winning contract: no stable general CLI exit-code contract exists; only the Ctrl+C path in [docs/specs/session-providers.md](/Users/zer0cell/production/empack/docs/specs/session-providers.md:77), [application/session.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/session.rs:881), and [lib.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/lib.rs:71) is mature enough to count
- Conflicting sources: [application/commands.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.test.rs:6535) explicitly describes missing future error-code work rather than a finished contract
- Why this classification is correct: CLI users can observe exit codes, but the repo only nails down `130` for interrupts. General failures still lack a documented and proven taxonomy.
- User/runtime impact: shell automation cannot rely on stable non-zero categories beyond interrupt handling.
- Fix direction: define an explicit exit/error contract, then add CLI-level tests around real subprocess exit codes.
- Evidence:
  - Contract: [docs/specs/session-providers.md](/Users/zer0cell/production/empack/docs/specs/session-providers.md:77), [docs/specs/testing-architecture.md](/Users/zer0cell/production/empack/docs/specs/testing-architecture.md:49)
  - Runtime: [application/session.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/session.rs:881), [lib.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/lib.rs:71)
  - Proof gap: [application/commands.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.test.rs:6535)

### init and import

Contract sources:

- [docs/usage.md](/Users/zer0cell/production/empack/docs/usage.md:54)
- [docs/specs/import-pipeline.md](/Users/zer0cell/production/empack/docs/specs/import-pipeline.md:9)

Runtime sources:

- [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:231)
- [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:926)
- [empack/import.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/empack/import.rs:1)

Proof sources:

- [application/commands.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.test.rs:389)
- [crates/empack-tests/tests/init_workflows.rs](/Users/zer0cell/production/empack/crates/empack-tests/tests/init_workflows.rs:253)

Baseline:

- `init` and archive/remote `init --from` are broadly aligned.
- The only major import gap found here is an explicit packwiz-directory deferral recorded in Section 6.

### add and sync

Contract sources:

- [docs/usage.md](/Users/zer0cell/production/empack/docs/usage.md:109)
- [docs/specs/search-and-resolution.md](/Users/zer0cell/production/empack/docs/specs/search-and-resolution.md:9)
- [docs/specs/config-and-manifest.md](/Users/zer0cell/production/empack/docs/specs/config-and-manifest.md:98)

Runtime sources:

- [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:1496)
- [application/sync.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/sync.rs:93)
- [empack/content.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/empack/content.rs:13)
- [empack/search.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/empack/search.rs:1)

Proof sources:

- [empack/search.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/empack/search.test.rs:129)
- [empack/content.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/empack/content.test.rs:95)
- [crates/empack-tests/tests/sync_workflow.rs](/Users/zer0cell/production/empack/crates/empack-tests/tests/sync_workflow.rs:33)

Baseline:

- Search order, add-contract planning, and sync reconciliation are source-backed and tested.
- Direct `.zip` URL classification is intentionally broader than `add`; the command layer still rejects non-JAR direct downloads, and docs/specs say so. That is an explicit deferral, not drift.

### remove and clean

Contract sources:

- [docs/usage.md](/Users/zer0cell/production/empack/docs/usage.md:197)
- [docs/specs/cli-surface.md](/Users/zer0cell/production/empack/docs/specs/cli-surface.md:169)
- [docs/specs/state-machine.md](/Users/zer0cell/production/empack/docs/specs/state-machine.md:93)

Runtime sources:

- [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:2545)
- [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:3372)
- [api/dependency_graph.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/api/dependency_graph.rs:91)

Proof sources:

- [application/commands.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.test.rs:5443)
- [crates/empack-tests/tests/clean_command.rs](/Users/zer0cell/production/empack/crates/empack-tests/tests/clean_command.rs:19)
- [crates/empack-tests/tests/remove_command.rs](/Users/zer0cell/production/empack/crates/empack-tests/tests/remove_command.rs:33)

Baseline:

- `remove` is operationally wired.
- `clean` runtime behavior is well proven, but Section 4 CLI surface records a public-doc drift.
- Orphan detection uses the dependency graph API, which itself lacks a dedicated contract and is recorded separately as `F-DEPGRAPH-001`.

### build pipeline

Contract sources:

- [docs/usage.md](/Users/zer0cell/production/empack/docs/usage.md:146)
- [docs/specs/build-and-distribution.md](/Users/zer0cell/production/empack/docs/specs/build-and-distribution.md:9)

Runtime sources:

- [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:2828)
- [empack/builds.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/empack/builds.rs:1433)

Proof sources:

- [application/commands.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.test.rs:4111)
- [crates/empack-tests/tests/e2e_restricted_build.rs](/Users/zer0cell/production/empack/crates/empack-tests/tests/e2e_restricted_build.rs:62)

Baseline:

- The target set, `all` expansion, restricted-build early-stop behavior, and continuation handoff are coherent.
- No direct build-pipeline miswire was found in this baseline pass.

### restricted-download continuation

Contract sources:

- [README.md](/Users/zer0cell/production/empack/README.md:19)
- [docs/usage.md](/Users/zer0cell/production/empack/docs/usage.md:176)
- [docs/specs/build-and-distribution.md](/Users/zer0cell/production/empack/docs/specs/build-and-distribution.md:91)

Runtime sources:

- [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:2894)
- [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:2981)
- [empack/restricted_build.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/empack/restricted_build.rs:89)

Proof sources:

- [crates/empack-tests/tests/build_continue.rs](/Users/zer0cell/production/empack/crates/empack-tests/tests/build_continue.rs:9)
- [crates/empack-tests/tests/e2e_restricted_build.rs](/Users/zer0cell/production/empack/crates/empack-tests/tests/e2e_restricted_build.rs:62)

Baseline:

- This area is currently the cleanest aligned public contract in the repo.
- The proof layer checks both injected-session logic and subprocess reachability.

### config and manifest

Contract sources:

- [docs/usage.md](/Users/zer0cell/production/empack/docs/usage.md:230)
- [docs/specs/config-and-manifest.md](/Users/zer0cell/production/empack/docs/specs/config-and-manifest.md:9)
- [docs/specs/cli-surface.md](/Users/zer0cell/production/empack/docs/specs/cli-surface.md:23)

Runtime sources:

- [application/config.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/config.rs:52)
- [empack/config.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/empack/config.rs:136)

Proof sources:

- [application/loader.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/loader.test.rs:158)
- [crates/empack-tests/tests/sync_workflow.rs](/Users/zer0cell/production/empack/crates/empack-tests/tests/sync_workflow.rs:33)

Baseline:

- The `empack.yml` versus `pack.toml` split is coherent and well described.
- The main internal drift is between CLI parse defaults and programmatic config defaults.

#### `F-CONFIG-DEFAULT-001`

- ID: `F-CONFIG-DEFAULT-001`
- Category: `Miswired`
- Severity: `S2`
- Winning contract: the public CLI/config surface in [docs/usage.md](/Users/zer0cell/production/empack/docs/usage.md:21), [docs/specs/cli-surface.md](/Users/zer0cell/production/empack/docs/specs/cli-surface.md:27), and clap-backed config loading in [application/config.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/config.rs:79)
- Conflicting sources: programmatic `Default` in [application/config.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/config.rs:126) and test/session constructors that rely on it, including [application/session.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/session.rs:1446)
- Why this classification is correct: the same conceptual config object has two different defaults. CLI users get a built-in CurseForge key, but direct `AppConfig::default()` callers do not.
- User/runtime impact: live CLI behavior and programmatic/test session behavior can diverge in networked CurseForge flows.
- Fix direction: make `Default` match the clap surface, or explicitly stop using `AppConfig::default()` where CLI parity is required.
- Evidence:
  - Contract: [docs/usage.md](/Users/zer0cell/production/empack/docs/usage.md:21), [docs/specs/cli-surface.md](/Users/zer0cell/production/empack/docs/specs/cli-surface.md:27), [application/config.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/config.rs:79)
  - Runtime: [application/config.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/config.rs:126)
  - Proof / parity check: [application/loader.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/loader.test.rs:158), [application/session.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/session.rs:1446)

### pack state machine

Contract sources:

- [docs/specs/state-machine.md](/Users/zer0cell/production/empack/docs/specs/state-machine.md:9)
- [docs/specs/types.md](/Users/zer0cell/production/empack/docs/specs/types.md:51)

Runtime sources:

- [empack/state.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/empack/state.rs:98)
- [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:784)

Proof sources:

- [empack/state.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/empack/state.test.rs:804)
- [crates/empack-tests/tests/init_workflows.rs](/Users/zer0cell/production/empack/crates/empack-tests/tests/init_workflows.rs:253)

Baseline:

- Discovery and clean semantics are source-backed and well tested.
- The draft state spec is stale around pure initialization details.

#### `F-STATE-INIT-001`

- ID: `F-STATE-INIT-001`
- Category: `Miswired`
- Severity: `S3`
- Winning contract: source plus proof in [empack/state.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/empack/state.rs:509), [empack/state.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/empack/state.test.rs:817), and [crates/empack-tests/tests/init_workflows.rs](/Users/zer0cell/production/empack/crates/empack-tests/tests/init_workflows.rs:311)
- Conflicting sources: [docs/specs/state-machine.md](/Users/zer0cell/production/empack/docs/specs/state-machine.md:26) and [docs/specs/state-machine.md](/Users/zer0cell/production/empack/docs/specs/state-machine.md:74)
- Why this classification is correct: the spec says `create_initial_structure()` creates `dist/` and refers to `validate_state()`, but source and tests show pure initialization creates only `pack/` and `templates/`; `dist/*` comes later from template scaffolding.
- User/runtime impact: mostly spec hygiene, but it weakens contract arbitration around init and clean recovery.
- Fix direction: update the state spec to match pure transition behavior and separate command-layer scaffolding from state-transition responsibilities.
- Evidence:
  - Contract: [docs/specs/state-machine.md](/Users/zer0cell/production/empack/docs/specs/state-machine.md:26), [docs/specs/state-machine.md](/Users/zer0cell/production/empack/docs/specs/state-machine.md:74)
  - Runtime: [empack/state.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/empack/state.rs:509)
  - Proof: [empack/state.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/empack/state.test.rs:817), [crates/empack-tests/tests/init_workflows.rs](/Users/zer0cell/production/empack/crates/empack-tests/tests/init_workflows.rs:311)

### session/providers

Contract sources:

- [docs/specs/session-providers.md](/Users/zer0cell/production/empack/docs/specs/session-providers.md:9)

Runtime sources:

- [application/session.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/session.rs:249)
- [application/session.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/session.rs:995)

Proof sources:

- [application/session.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/session.rs:1431)
- [docs/specs/testing-architecture.md](/Users/zer0cell/production/empack/docs/specs/testing-architecture.md:24)

Baseline:

- The live `Session` seam is central and mostly coherent.
- One test-only session path is not wired to the full seam it claims to represent.

#### `F-SESSION-MOCK-001`

- ID: `F-SESSION-MOCK-001`
- Category: `Unwired`
- Severity: `S2`
- Winning contract: [docs/specs/session-providers.md](/Users/zer0cell/production/empack/docs/specs/session-providers.md:11) and [docs/specs/session-providers.md](/Users/zer0cell/production/empack/docs/specs/session-providers.md:94)
- Conflicting sources: [application/session.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/session.rs:1431)
- Why this classification is correct: the session contract says commands operate through session accessors for archive, packwiz, and state, but the test session used in `session.rs` leaves those accessors as `unimplemented!()`.
- User/runtime impact: this is an internal seam drift and proof-risk, not a live CLI regression. It narrows what that scaffolding can actually validate.
- Fix direction: either wire those accessors in the test session or narrow the contract so this helper is explicitly partial.
- Evidence:
  - Contract: [docs/specs/session-providers.md](/Users/zer0cell/production/empack/docs/specs/session-providers.md:11), [docs/specs/session-providers.md](/Users/zer0cell/production/empack/docs/specs/session-providers.md:94)
  - Runtime: [application/session.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/session.rs:1431)

### networking and rate budgets

Contract sources:

- [docs/specs/networking-and-rate-budgets.md](/Users/zer0cell/production/empack/docs/specs/networking-and-rate-budgets.md:9)

Runtime sources:

- [networking/mod.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/networking/mod.rs:53)
- [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:1534)

Proof sources:

- [empack/search.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/empack/search.test.rs:129)
- [networking/mod.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/networking/mod.rs:192)

Baseline:

- Shared clients, caching, and rate budgets are real live seams.
- The dedicated concurrency manager remains partially disconnected from CLI command paths.

#### `F-NET-RES-MGR-001`

- ID: `F-NET-RES-MGR-001`
- Category: `Unwired`
- Severity: `S2`
- Winning contract: [docs/specs/networking-and-rate-budgets.md](/Users/zer0cell/production/empack/docs/specs/networking-and-rate-budgets.md:108) plus the runtime abstraction in [networking/mod.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/networking/mod.rs:71)
- Conflicting sources: live commands still use provider-level clients and resolvers directly, for example [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:1557)
- Why this classification is correct: the manager exists, calculates optimal jobs, and is described as runtime infrastructure, but command execution does not route batch resolution through it.
- User/runtime impact: rate budgets exist, but the broader “resource-aware networking manager” promise is not an end-to-end CLI contract yet.
- Fix direction: integrate `NetworkingManager` into batch command flows, or explicitly demote it to a library-only helper and update the spec.
- Evidence:
  - Contract: [docs/specs/networking-and-rate-budgets.md](/Users/zer0cell/production/empack/docs/specs/networking-and-rate-budgets.md:108)
  - Runtime: [networking/mod.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/networking/mod.rs:71), [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:1534), [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:1557)
  - Proof: [empack/search.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/empack/search.test.rs:129)

### platform and managed tooling

Contract sources:

- [README.md](/Users/zer0cell/production/empack/README.md:34)
- [docs/specs/platform-and-managed-tooling.md](/Users/zer0cell/production/empack/docs/specs/platform-and-managed-tooling.md:9)

Runtime sources:

- [platform/packwiz_bin.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/platform/packwiz_bin.rs:15)
- [application/session.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/session.rs:1028)

Proof sources:

- [crates/empack-tests/tests/e2e_version.rs](/Users/zer0cell/production/empack/crates/empack-tests/tests/e2e_version.rs:63)

Baseline:

- Managed `packwiz-tx` resolution is a relatively mature contract.
- No baseline drift was found here.

### logging and telemetry

Contract sources:

- [docs/testing.md](/Users/zer0cell/production/empack/docs/testing.md:31)
- [docs/specs/logging-and-telemetry.md](/Users/zer0cell/production/empack/docs/specs/logging-and-telemetry.md:9)

Runtime sources:

- [logger/mod.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/logger/mod.rs:42)
- [application/session.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/session.rs:1037)
- [lib.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/lib.rs:71)

Proof sources:

- [crates/empack-tests/tests/e2e_version.rs](/Users/zer0cell/production/empack/crates/empack-tests/tests/e2e_version.rs:24)

Baseline:

- Logger lifecycle and telemetry gating are coherent.
- The unresolved CLI exit taxonomy from `F-CLI-EXIT-001` is adjacent to this area but not a direct logger miswire.

### testing architecture

Contract sources:

- [docs/testing.md](/Users/zer0cell/production/empack/docs/testing.md:20)
- [docs/specs/testing-architecture.md](/Users/zer0cell/production/empack/docs/specs/testing-architecture.md:9)

Runtime sources:

- Test harnesses under [crates/empack-tests/tests/](/Users/zer0cell/production/empack/crates/empack-tests/tests/e2e_version.rs:1)
- Session and command test scaffolding under [application/session.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/session.rs:1431) and [application/commands.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.test.rs:112)

Proof sources:

- [docs/testing.md](/Users/zer0cell/production/empack/docs/testing.md:52)
- [docs/specs/testing-architecture.md](/Users/zer0cell/production/empack/docs/specs/testing-architecture.md:67)

Baseline:

- The layered testing story is clearly described.
- Several tests still prove helper behavior, reachability, or future intent rather than a stable product contract. Section 7 records the main drift points.

### display/terminal

Contract sources:

- Only the high-level subsystem map in [docs/specs/overview.md](/Users/zer0cell/production/empack/docs/specs/overview.md:21)

Runtime sources:

- [display/mod.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/display/mod.rs:10)
- [terminal/capabilities.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/terminal/capabilities.rs:4)

Proof sources:

- [display/display.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/display/display.test.rs:57)
- [terminal/capabilities.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/terminal/capabilities.test.rs:75)

Baseline:

- Both subsystems are real runtime surfaces.
- Neither has a dedicated spec or stable public contract beyond overview-level mentions.

#### `F-DISPLAY-TERM-001`

- ID: `F-DISPLAY-TERM-001`
- Category: `Undefined`
- Severity: `S2`
- Winning contract: none at dedicated-subsystem granularity; only overview-level references plus source/tests exist
- Conflicting sources: no direct contradiction, but there is no mature contract to arbitrate against
- Why this classification is correct: output capability detection, global display initialization, and styled status/progress behavior are live and tested, but no dedicated spec describes what is intentionally stable.
- User/runtime impact: future UX changes can drift silently because arbitration falls back to implementation details.
- Fix direction: add dedicated display and terminal specs, or explicitly designate a smaller stable output contract.
- Evidence:
  - Contract gap: [docs/specs/overview.md](/Users/zer0cell/production/empack/docs/specs/overview.md:21)
  - Runtime: [display/mod.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/display/mod.rs:10), [terminal/capabilities.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/terminal/capabilities.rs:4)
  - Proof: [display/display.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/display/display.test.rs:57), [terminal/capabilities.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/terminal/capabilities.test.rs:75)

### dependency graph API

Contract sources:

- Only the subsystem reference in [docs/specs/overview.md](/Users/zer0cell/production/empack/docs/specs/overview.md:33)

Runtime sources:

- [api/dependency_graph.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/api/dependency_graph.rs:17)
- [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:1534)
- [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:2661)

Proof sources:

- [api/dependency_graph.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/api/dependency_graph.test.rs:75)

Baseline:

- This API is not dead code. It participates in add/remove flows today.
- It still lacks a dedicated contract source.

#### `F-DEPGRAPH-001`

- ID: `F-DEPGRAPH-001`
- Category: `Undefined`
- Severity: `S2`
- Winning contract: none at dedicated-spec granularity; only source/tests and the overview mention exist
- Conflicting sources: no stable spec conflicts because no dedicated spec exists
- Why this classification is correct: add and remove flows depend on graph semantics such as topological ordering and orphan detection, but there is no dedicated spec to define what must remain stable.
- User/runtime impact: behavior changes in orphan detection or dependency traversal will be hard to judge as regressions versus refactors.
- Fix direction: add a dedicated dependency-graph spec or explicitly mark the API as internal-only and keep it out of product-contract claims.
- Evidence:
  - Contract gap: [docs/specs/overview.md](/Users/zer0cell/production/empack/docs/specs/overview.md:33)
  - Runtime: [api/dependency_graph.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/api/dependency_graph.rs:91), [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:1534), [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:2669)
  - Proof: [api/dependency_graph.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/api/dependency_graph.test.rs:75)

### security-sensitive filesystem session behavior

Contract sources:

- No dedicated spec file
- Test commentary in [application/session_security.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/session_security.rs:1)

Runtime sources:

- [application/session.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/session.rs:280)

Proof sources:

- [application/session_security.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/session_security.rs:12)

Baseline:

- This is the weakest contract area in the audit.
- The tests describe canonicalization and traversal prevention more strongly than the live provider implementation does.

#### `F-SESSION-SEC-001`

- ID: `F-SESSION-SEC-001`
- Category: `Undefined`
- Severity: `S2`
- Winning contract: none; current comments and tests are not mature enough to serve as a stable security contract
- Conflicting sources: [application/session_security.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/session_security.rs:1) versus the raw filesystem operations in [application/session.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/session.rs:289)
- Why this classification is correct: the tests describe path-traversal prevention via canonicalization, but the live provider performs direct `std::fs` reads, writes, directory creation, and removals without base-directory enforcement.
- User/runtime impact: this is a security-relevant understanding gap. The repo currently lacks a stable statement of what filesystem boundary, if any, is guaranteed.
- Fix direction: define the intended filesystem trust boundary first, then write tests that prove that exact boundary instead of incidental nonexistent-path failures.
- Evidence:
  - Contract gap / overclaim: [application/session_security.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/session_security.rs:1), [application/session_security.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/session_security.rs:46), [application/session_security.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/session_security.rs:71)
  - Runtime: [application/session.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/session.rs:289), [application/session.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/session.rs:320), [application/session.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/session.rs:386)

## 5. Undefined surface inventory

| Surface | Why undefined | Live reachability | Evidence | Related finding |
| --- | --- | --- | --- | --- |
| CLI error and exit taxonomy | Only Ctrl+C `130` is explicit; general non-zero categories are not defined | Public CLI | [application/session.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/session.rs:881), [lib.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/lib.rs:79), [application/commands.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.test.rs:6535) | `F-CLI-EXIT-001` |
| `display/*` | Live surface and tests exist, but no dedicated spec file | High | [display/mod.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/display/mod.rs:10), [display/display.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/display/display.test.rs:57) | `F-DISPLAY-TERM-001` |
| `terminal/*` | Same as display: only overview-level contract | High | [terminal/capabilities.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/terminal/capabilities.rs:4), [terminal/capabilities.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/terminal/capabilities.test.rs:75) | `F-DISPLAY-TERM-001` |
| Dependency graph API | Used by add/remove, but no dedicated spec | Medium | [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:1534), [api/dependency_graph.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/api/dependency_graph.rs:91) | `F-DEPGRAPH-001` |
| Filesystem session security behavior | Security expectations are commentary-driven, not contract-driven | Medium | [application/session_security.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/session_security.rs:1), [application/session.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/session.rs:289) | `F-SESSION-SEC-001` |

## 6. Explicitly deferred surface inventory

#### `D-IMPORT-PACKWIZDIR-001`

- ID: `D-IMPORT-PACKWIZDIR-001`
- Category: `Explicitly Deferred`
- Severity: `S2`
- Winning contract: [docs/specs/import-pipeline.md](/Users/zer0cell/production/empack/docs/specs/import-pipeline.md:15) and [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:1143)
- Conflicting sources: none; source, spec, and tests agree this is not supported yet
- Why this classification is correct: the repo detects local packwiz directories, then explicitly rejects them with a “not yet implemented” error.
- User/runtime impact: `init --from <packwiz-dir>` is unavailable, but that absence is honestly represented.
- Fix direction: keep it deferred until product intent changes, then wire it end-to-end instead of weakening the current explicit reject.
- Evidence:
  - Contract: [docs/specs/import-pipeline.md](/Users/zer0cell/production/empack/docs/specs/import-pipeline.md:15)
  - Runtime: [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:1143)
  - Proof: [application/commands.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.test.rs:1293)

#### `D-ADD-NONJAR-001`

- ID: `D-ADD-NONJAR-001`
- Category: `Explicitly Deferred`
- Severity: `S2`
- Winning contract: [docs/usage.md](/Users/zer0cell/production/empack/docs/usage.md:130), [docs/specs/cli-surface.md](/Users/zer0cell/production/empack/docs/specs/cli-surface.md:160), [docs/specs/search-and-resolution.md](/Users/zer0cell/production/empack/docs/specs/search-and-resolution.md:64), and [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:1717)
- Conflicting sources: none; the broader URL classifier also recognizes direct `.zip` URLs, but the command contract explicitly limits `add` to `.jar`
- Why this classification is correct: the repo intentionally separates “classifiable direct download URL” from “supported add flow.” Import can use direct `.zip` URLs; `add` cannot.
- User/runtime impact: direct-download `.zip` URLs are rejected by `add`, but that is currently disclosed rather than misrepresented.
- Fix direction: leave it deferred unless the product wants direct archive add support.
- Evidence:
  - Contract: [docs/usage.md](/Users/zer0cell/production/empack/docs/usage.md:130), [docs/specs/cli-surface.md](/Users/zer0cell/production/empack/docs/specs/cli-surface.md:160), [docs/specs/search-and-resolution.md](/Users/zer0cell/production/empack/docs/specs/search-and-resolution.md:64)
  - Runtime: [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:1717), [empack/content.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/empack/content.rs:88)
  - Proof: [empack/content.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/empack/content.test.rs:107)

#### `D-VERSIONS-FORGESELECT-001`

- ID: `D-VERSIONS-FORGESELECT-001`
- Category: `Explicitly Deferred`
- Severity: `S3`
- Winning contract: source commentary in [empack/versions.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/empack/versions.rs:771)
- Conflicting sources: none found in public docs
- Why this classification is correct: the code explicitly labels `@latest`, `@recommended`, and `@version_id` filtering for Forge CLI-mode selectors as future implementation.
- User/runtime impact: low today; this is an internal future-work marker rather than a user-visible broken contract.
- Fix direction: keep as deferred until the selector UX is productized.
- Evidence:
  - Runtime: [empack/versions.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/empack/versions.rs:771)

Non-runtime deferred note:

- Containerized E2E is explicitly outside the active task path in [docs/specs/testing-architecture.md](/Users/zer0cell/production/empack/docs/specs/testing-architecture.md:119). This is a testing non-goal, not a runtime drift item.

## 7. Test-coverage drift inventory

| ID | Drift type | Why it matters | Evidence |
| --- | --- | --- | --- |
| `T-DRIFT-001` | Tested helper with no stable live contract | `Commands::requires_modpack()` and `execution_order()` are tested metadata helpers, but live dispatch does not use them. They should be treated as internal-only until wired or specified. | [application/cli.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/cli.rs:321), [application/cli.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/cli.rs:409), [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:56) |
| `T-DRIFT-002` | Mock/default config parity gap | Many tests and test sessions use `AppConfig::default()`, which does not match the live CLI’s built-in CurseForge key default. | [application/config.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/config.rs:79), [application/config.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/config.rs:126), [application/session.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/session.rs:1446), [crates/empack-tests/tests/clean_command.rs](/Users/zer0cell/production/empack/crates/empack-tests/tests/clean_command.rs:53) |
| `T-DRIFT-003` | Security tests over-claim the live guarantee | `session_security.rs` talks about canonicalization and traversal prevention, but its assertions mostly prove nonexistent-path failures or absence of leakage in the specific temp setup, not a real sandbox boundary. | [application/session_security.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/session_security.rs:1), [application/session.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/session.rs:289) |
| `T-DRIFT-004` | PTY/E2E reachability is not a full UX contract | Current PTY coverage intentionally validates reachability and persisted state, not prompt strings or the full public interaction contract. | [docs/testing.md](/Users/zer0cell/production/empack/docs/testing.md:52), [docs/specs/testing-architecture.md](/Users/zer0cell/production/empack/docs/specs/testing-architecture.md:67), [crates/empack-tests/tests/e2e_restricted_build.rs](/Users/zer0cell/production/empack/crates/empack-tests/tests/e2e_restricted_build.rs:62) |

## 8. Recommended remediation ordering

1. Resolve the public `clean` contract drift first: `F-CLI-CLEAN-001`.
2. Define and prove CLI exit/error semantics next: `F-CLI-EXIT-001`.
3. Decide whether the session seam contract requires fully wired test sessions, then either implement or narrow it: `F-SESSION-MOCK-001`.
4. Decide whether `NetworkingManager` is supposed to be live CLI infrastructure or library-only scaffolding: `F-NET-RES-MGR-001`.
5. Align programmatic config defaults with CLI parse defaults so tests and helper sessions stop drifting: `F-CONFIG-DEFAULT-001`.
6. Refresh stale state-machine language after the higher-value runtime contract issues above: `F-STATE-INIT-001`.
7. Add dedicated specs for display, terminal, dependency graph, and filesystem session security before expanding proof claims in those areas: `F-DISPLAY-TERM-001`, `F-DEPGRAPH-001`, `F-SESSION-SEC-001`.
8. Keep explicit deferrals deferred unless product intent changes: `D-IMPORT-PACKWIZDIR-001`, `D-ADD-NONJAR-001`, `D-VERSIONS-FORGESELECT-001`.

## 9. Appendix: contract map

### 9.1 Public workflow contract map

| Workflow | Contract sources | Runtime source | Proof | Baseline |
| --- | --- | --- | --- | --- |
| CLI root/options | [docs/usage.md](/Users/zer0cell/production/empack/docs/usage.md:17), [docs/specs/cli-surface.md](/Users/zer0cell/production/empack/docs/specs/cli-surface.md:13) | [application/cli.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/cli.rs:13), [application/config.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/config.rs:52) | [application/loader.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/loader.test.rs:158) | `Partial` |
| `requirements` | [README.md](/Users/zer0cell/production/empack/README.md:38), [docs/usage.md](/Users/zer0cell/production/empack/docs/usage.md:38) | [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:151) | [application/commands.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.test.rs:153), [crates/empack-tests/tests/e2e_version.rs](/Users/zer0cell/production/empack/crates/empack-tests/tests/e2e_version.rs:63) | `Aligned` |
| `version` | [README.md](/Users/zer0cell/production/empack/README.md:41), [docs/usage.md](/Users/zer0cell/production/empack/docs/usage.md:46) | [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:204) | [application/commands.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.test.rs:114), [crates/empack-tests/tests/e2e_version.rs](/Users/zer0cell/production/empack/crates/empack-tests/tests/e2e_version.rs:4) | `Aligned` |
| `init` | [docs/usage.md](/Users/zer0cell/production/empack/docs/usage.md:54), [docs/specs/cli-surface.md](/Users/zer0cell/production/empack/docs/specs/cli-surface.md:59) | [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:231), [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:784) | [application/commands.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.test.rs:389), [crates/empack-tests/tests/init_workflows.rs](/Users/zer0cell/production/empack/crates/empack-tests/tests/init_workflows.rs:253) | `Aligned` |
| `init --from` | [docs/usage.md](/Users/zer0cell/production/empack/docs/usage.md:84), [docs/specs/import-pipeline.md](/Users/zer0cell/production/empack/docs/specs/import-pipeline.md:11) | [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:926), [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:1139) | [application/commands.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.test.rs:1116), [application/commands.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.test.rs:1293) | `Aligned` with explicit deferred local-packwiz path |
| `add` | [docs/usage.md](/Users/zer0cell/production/empack/docs/usage.md:109), [docs/specs/search-and-resolution.md](/Users/zer0cell/production/empack/docs/specs/search-and-resolution.md:52) | [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:1496), [application/sync.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/sync.rs:171) | [empack/search.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/empack/search.test.rs:129), [empack/content.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/empack/content.test.rs:95) | `Aligned` with explicit deferred non-JAR direct-download path |
| `sync` | [docs/usage.md](/Users/zer0cell/production/empack/docs/usage.md:137), [docs/specs/config-and-manifest.md](/Users/zer0cell/production/empack/docs/specs/config-and-manifest.md:123) | [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:3474), [application/sync.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/sync.rs:93) | [crates/empack-tests/tests/sync_workflow.rs](/Users/zer0cell/production/empack/crates/empack-tests/tests/sync_workflow.rs:33) | `Aligned` |
| `remove` | [docs/usage.md](/Users/zer0cell/production/empack/docs/usage.md:197), [docs/specs/cli-surface.md](/Users/zer0cell/production/empack/docs/specs/cli-surface.md:169) | [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:2545) | [crates/empack-tests/tests/remove_command.rs](/Users/zer0cell/production/empack/crates/empack-tests/tests/remove_command.rs:33), [crates/empack-tests/tests/remove_command.rs](/Users/zer0cell/production/empack/crates/empack-tests/tests/remove_command.rs:188) | `Partial` |
| `build` | [docs/usage.md](/Users/zer0cell/production/empack/docs/usage.md:146), [docs/specs/build-and-distribution.md](/Users/zer0cell/production/empack/docs/specs/build-and-distribution.md:13) | [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:2828), [empack/builds.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/empack/builds.rs:1433) | [application/commands.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.test.rs:4111) | `Aligned` |
| `build --continue` | [README.md](/Users/zer0cell/production/empack/README.md:19), [docs/usage.md](/Users/zer0cell/production/empack/docs/usage.md:176) | [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:2981), [empack/restricted_build.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/empack/restricted_build.rs:134) | [crates/empack-tests/tests/build_continue.rs](/Users/zer0cell/production/empack/crates/empack-tests/tests/build_continue.rs:9), [crates/empack-tests/tests/e2e_restricted_build.rs](/Users/zer0cell/production/empack/crates/empack-tests/tests/e2e_restricted_build.rs:62) | `Aligned` |
| `clean` | [docs/usage.md](/Users/zer0cell/production/empack/docs/usage.md:209), [docs/specs/cli-surface.md](/Users/zer0cell/production/empack/docs/specs/cli-surface.md:182), [README.md](/Users/zer0cell/production/empack/README.md:47) | [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:3372) | [application/commands.test.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.test.rs:5443), [crates/empack-tests/tests/clean_command.rs](/Users/zer0cell/production/empack/crates/empack-tests/tests/clean_command.rs:19) | `Miswired` |

### 9.2 Runtime subsystem coverage table

| Subsystem | Dedicated spec? | Public docs? | Runtime source | Tests? | Status |
| --- | --- | --- | --- | --- | --- |
| CLI surface | Yes: [cli-surface.md](/Users/zer0cell/production/empack/docs/specs/cli-surface.md:1) | Yes | [application/cli.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/cli.rs:7) | Yes | `Partial` |
| init and import | Yes: [import-pipeline.md](/Users/zer0cell/production/empack/docs/specs/import-pipeline.md:1) | Yes | [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:231) | Yes | `Partial` |
| add and sync | Yes: [search-and-resolution.md](/Users/zer0cell/production/empack/docs/specs/search-and-resolution.md:1), [config-and-manifest.md](/Users/zer0cell/production/empack/docs/specs/config-and-manifest.md:1) | Yes | [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:1496), [application/sync.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/sync.rs:1) | Yes | `Partial` |
| remove and clean | Yes: [cli-surface.md](/Users/zer0cell/production/empack/docs/specs/cli-surface.md:169), [state-machine.md](/Users/zer0cell/production/empack/docs/specs/state-machine.md:93) | Yes | [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:2545) | Yes | `Partial` |
| build pipeline | Yes: [build-and-distribution.md](/Users/zer0cell/production/empack/docs/specs/build-and-distribution.md:1) | Yes | [empack/builds.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/empack/builds.rs:1433) | Yes | `Partial` |
| restricted-download continuation | Yes: [build-and-distribution.md](/Users/zer0cell/production/empack/docs/specs/build-and-distribution.md:91) | Yes | [application/commands.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/commands.rs:2981), [empack/restricted_build.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/empack/restricted_build.rs:13) | Yes | `Aligned` |
| config and manifest | Yes: [config-and-manifest.md](/Users/zer0cell/production/empack/docs/specs/config-and-manifest.md:1) | Yes | [application/config.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/config.rs:52), [empack/config.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/empack/config.rs:136) | Yes | `Partial` |
| pack state machine | Yes: [state-machine.md](/Users/zer0cell/production/empack/docs/specs/state-machine.md:1) | No | [empack/state.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/empack/state.rs:98) | Yes | `Stale` |
| session/providers | Yes: [session-providers.md](/Users/zer0cell/production/empack/docs/specs/session-providers.md:1) | No | [application/session.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/session.rs:249) | Yes | `Partial` |
| networking and rate budgets | Yes: [networking-and-rate-budgets.md](/Users/zer0cell/production/empack/docs/specs/networking-and-rate-budgets.md:1) | No | [networking/mod.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/networking/mod.rs:53) | Yes | `Partial` |
| platform and managed tooling | Yes: [platform-and-managed-tooling.md](/Users/zer0cell/production/empack/docs/specs/platform-and-managed-tooling.md:1) | Partial | [platform/packwiz_bin.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/platform/packwiz_bin.rs:15) | Yes | `Partial` |
| logging and telemetry | Yes: [logging-and-telemetry.md](/Users/zer0cell/production/empack/docs/specs/logging-and-telemetry.md:1) | Partial | [logger/mod.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/logger/mod.rs:42) | Yes | `Partial` |
| testing architecture | Yes: [testing-architecture.md](/Users/zer0cell/production/empack/docs/specs/testing-architecture.md:1) | Yes: [docs/testing.md](/Users/zer0cell/production/empack/docs/testing.md:1) | Test harnesses | Yes | `Partial` |
| display | No | No | [display/mod.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/display/mod.rs:10) | Yes | `Undefined` |
| terminal | No | No | [terminal/capabilities.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/terminal/capabilities.rs:4) | Yes | `Undefined` |
| dependency graph API | No | No | [api/dependency_graph.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/api/dependency_graph.rs:17) | Yes | `Undefined` |
| session security | No | No | [application/session_security.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/session_security.rs:1), [application/session.rs](/Users/zer0cell/production/empack/crates/empack-lib/src/application/session.rs:280) | Yes | `Undefined` |

### 9.3 Spec file classification

| Spec file | Classification | Note |
| --- | --- | --- |
| [overview.md](/Users/zer0cell/production/empack/docs/specs/overview.md:1) | `partial` | Good subsystem map, but its generic source-of-truth rule is too coarse for mixed draft/live arbitration. |
| [cli-surface.md](/Users/zer0cell/production/empack/docs/specs/cli-surface.md:1) | `partial` | Mostly current; `clean` drift comes from README, not this spec. |
| [config-and-manifest.md](/Users/zer0cell/production/empack/docs/specs/config-and-manifest.md:1) | `partial` | Strong description of `empack.yml`/`pack.toml`; internal config-default parity still drifts. |
| [import-pipeline.md](/Users/zer0cell/production/empack/docs/specs/import-pipeline.md:1) | `partial` | Broadly current and explicit about deferred packwiz-directory import. |
| [build-and-distribution.md](/Users/zer0cell/production/empack/docs/specs/build-and-distribution.md:1) | `partial` | Closest thing to a mature workflow spec in the repo. |
| [state-machine.md](/Users/zer0cell/production/empack/docs/specs/state-machine.md:1) | `stale` | Pure initialization details and function naming are out of sync with source/tests. |
| [session-providers.md](/Users/zer0cell/production/empack/docs/specs/session-providers.md:1) | `partial` | Good live-session map, but some test scaffolding is less complete than the spec implies. |
| [search-and-resolution.md](/Users/zer0cell/production/empack/docs/specs/search-and-resolution.md:1) | `partial` | Current for search order and URL handling. |
| [networking-and-rate-budgets.md](/Users/zer0cell/production/empack/docs/specs/networking-and-rate-budgets.md:1) | `partial` | Explicitly notes manager-versus-live-CLI gap. |
| [logging-and-telemetry.md](/Users/zer0cell/production/empack/docs/specs/logging-and-telemetry.md:1) | `partial` | Runtime lifecycle matches, but it does not close the broader CLI exit contract. |
| [platform-and-managed-tooling.md](/Users/zer0cell/production/empack/docs/specs/platform-and-managed-tooling.md:1) | `partial` | Broadly current for managed `packwiz-tx`. |
| [platform-modrinth.md](/Users/zer0cell/production/empack/docs/specs/platform-modrinth.md:1) | `partial` | Provider-specific contract exists, but this baseline sampled it indirectly through search/import paths rather than as a standalone drift hotspot. |
| [platform-curseforge.md](/Users/zer0cell/production/empack/docs/specs/platform-curseforge.md:1) | `partial` | Same as Modrinth; sampled indirectly through add/import/build paths. |
| [testing-architecture.md](/Users/zer0cell/production/empack/docs/specs/testing-architecture.md:1) | `partial` | Good layer split, but some tests still prove reachability or helper behavior rather than a stable contract. |
| [types.md](/Users/zer0cell/production/empack/docs/specs/types.md:1) | `partial` | Useful shared type index, but not sufficient to cover undefined subsystems such as display, terminal, and dependency graph behavior. |
