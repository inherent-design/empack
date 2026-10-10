# Helpers and acceptance tests

Contract for v0.6.0-beta. Callable types and signatures are defined in the
[verification](../../crates/empack-lib/src/engine/verification.rs), [bounded I/O](../../crates/empack-lib/src/engine/io.rs) and [artifact inspection](../../crates/empack-lib/src/engine/artifacts.rs). This page specifies their behavior and ownership.

## Shared helpers and utilities

Utilities should be cohesive modules with narrow purposes, not a large `utils.rs` or another generic framework.

| Helper | Responsibility and contract | Where used |
|---|---|---|
| Selector parsing | Parse user syntax without claiming canonical resolution. | CLI, import URL normalization. |
| Canonical identity lookup | Index logical records and observed files by canonical identity; return ambiguity rather than first match. | Add, sync, remove, adopt. |
| Document replacement | Apply exact logical changes against expected raw document revision; preserve unrelated syntax/fields. | Author and instance document publication. |
| Canonical encoding | Domain-separated, versioned, stable encoding; sort unordered maps, preserve meaningful sequence order. | Semantic revisions, recipes, plan grants. |
| Digest verification | Incremental algorithms and expected-size checks; report all declared supported digest mismatches. | Acquisition, manual association, artifact verification. |
| Content inspection | Bounded type/header/archive inspection with observed digest and provenance. | Direct JAR/ZIP and local file add. |
| Collision indexing | Detect duplicate and portable collisions without rewriting spellings. | Imports, layered projections, artifact parsing. |
| Layer projection | Apply declared common/side precedence and emit replacement evidence. | Import round trips and builds. |
| Native observation | Track files, absence, and directory membership; separate raw and semantic revisions. | Snapshotter, publisher conflict checks. |
| Bounded streaming | Stream with checked byte count and a bounded extra read to detect oversized input. | Downloads, extraction, staging. |
| Archive byte accounting | Aggregate actual expanded/output bytes across entries and stop before unauthorized writes. | Archive handling. |
| Durable file replacement | Same-filesystem staged replacement, permission policy, durability result. | Publisher, durable host-state documents. |
| Expected-old validation | Compare kind/content/root identity immediately within the publication protocol. | File changes and recovery. |
| Download candidate observations | Hash stable manual-download candidates once per discovery pass. | Restricted downloads. |
| Monotonic deadlines and clock control | Shared monotonic budgets, testable fake time, no retry resetting total deadline. | HTTP, process supervision, waits. |
| Retry classification | Distinguish not-found, unauthorized, rate-limited, transient, permanent, and unknown failures. | Provider transport. |
| Credential redaction | Secret-safe debug/display types; strip signed query/header details. | Logs, receipts, error context. |
| Runtime capability checks | Bounded process probe requiring valid status and capability data. | Runtime preparation. |
| Task admission | Atomic close versus admission/task registration; explicit busy/oversized states. | Runtime scopes. |
| Retained outcomes | Retain terminal results independently of lossy progress notifications. | Engine handles, CLI, embedding. |
| Verification diagnostics | Stable diagnostic codes identifying expected versus actual domain state. | Verify and recovery. |

Prefer established parsers, HTTP clients, cryptographic implementations, archive libraries, and OS primitives behind these wrappers. Write the **policy composition** empack needs; do not implement cryptographic algorithms or platform process ownership from scratch unnecessarily.

### Bounded I/O

The native I/O and acquisition helpers check cancellation and byte counts during
streaming. A byte bound does not limit time or decompression CPU: network acquisition
also enforces deadlines, while blocking workers require cooperative retirement.
Failures leave partial bytes in private staging rather than published destinations.

### Canonical encoding contract

Do not hash Rust `Debug` output, randomized `HashMap` traversal, raw YAML formatting, or platform-native absolute paths. Canonical encoding uses explicit domain/schema boundaries and deterministic values.

Map ordering is canonical by validated key. Order-sensitive URL alternatives, template precedence, or command arguments remain ordered. Include distinctions such as missing versus explicitly empty only where they affect semantics. Store raw document hashes separately for edit protection.

No helper named `normalize` should silently alter content meaning. Prefer `parse_selector`, `canonicalize_provider_identity`, `validate_rel_path`, `resolve_layers`, and `fingerprint_recipe` so callers can see which contract is established.

The public [diagnostic envelope](../../crates/empack-lib/src/engine/diagnostics.rs)
provides stable codes and optional expected/observed evidence. Classification uses
typed causes; unclassified failures retain an explicit generic code. Human-readable
context remains available without becoming the automation contract.

## Contract tests and failure injection

### Compile and architecture gates

`scripts/check-core-boundary.py` checks that `empack-core` has no runtime/I/O dependencies. Keep private-proof constructor visibility covered by compile-fail tests or carefully selected external-crate tests. Require object-safe infrastructure traits to be compiled in at least one concrete mock and one real composition root.

Public API examples compile as doctests; callable signatures live with their Rust definitions. No unchecked `todo!`, success-returning placeholder verifier, or permissive mock default should enter a production execution path.

### Reusable adapter suites

| Suite | Required behaviors |
|---|---|
| Provider catalog | Equivalent slug/ID/URL identity; pin ownership; type discovery; distinguish authorization, not-found, rate limit, and outage. |
| Import adapter | Preserve common/side layers, requirements, URLs/digests, embedded files, layout inference; reject unsupported conversions before live effects. |
| Acquisition | Chunked/false-length limits; stalled headers/body cancellation; digest mismatch; scoped redirects; no persistent writes under read-only cache policy. |
| Filesystem | Symlink/junction ancestors, case collisions, absent targets, hardlink copy safety, permissions, cross-device publication rejection. |
| Instance | Three-way ownership, seed preservation, choices, exact release, structured conflicts and launch gating. |
| Artifact writer/reader | Inventory completeness for all sources; duplicate paths; optional choices; deterministic settings where promised. |
| Process runner | Closed pipes with live child; live descendant after parent exit; output flooding; timeout during drain; cancellation and job retirement. |
| Publisher | Every durable crash point; expected-old conflict; before/after/neither recovery classification; prior artifact retained. |
| Runtime | Close/admit race, cancel-before-start, cancel-after-result, late result from superseded attempt, permit retention, lost progress notification. |

Unit-test fakes should be strict: unsupported operations fail, not silently succeed. Integration tests of native confinement, process trees, locks, and durability need real OS primitives. An in-memory filesystem proves planner logic, not native filesystem safety.

### Cross-command properties

```text
sync(sync(project)) == sync(project)                 # Convergence
add(slug) ≡ add(id) ≡ add(provider_url)                # Identity equivalence
rename_alias(project) preserves installed identity   # Label independence
preview(request, project) leaves durable state equal # All request variants
import/export preserves supported semantic inventory
successful_build implies every required inventory obligation discharged
cancelled_task retains reservation until actual retirement
publication conflict implies no live changes by that attempted publication
```

Equality means the relevant semantic state, not timestamps or nonessential diagnostic ordering. Property generators should vary provider/local/URL/embedded sources, aliases, pins, environments, paths, empty/missing files, and partial provider behavior.

A particularly valuable scenario is `add -> forced re-add under alias -> sync -> build -> remove -> sync`. It catches contracts that look correct in isolation but disagree across command boundaries.

### Publication fault model

Inject abrupt termination after: preimage write, preimage sync, journal-intent sync, candidate replace, target-directory sync, progress-record write, post-publication verification, committed-record sync, and preimage reclamation. Restart a fresh process after each injected failure.

Assert one of three truthful states: unchanged and retryable; committed and verified; or explicitly recovery-required with preserved recovery data. Never accept silent mixed state as a configured/healthy project.

Also inject disk-full, permission changes, antivirus/in-use-file failures where reproducible, cross-volume staging, corrupted/truncated journal, unknown schema, concurrent external edit, and missing retained content. SQLite's crash-testing approach is a useful precedent for focusing on durable boundaries rather than only ordinary exception paths. [R8](https://sqlite.org/atomiccommit.html)

### Feature and conformance fixtures

Retain real pack fixtures and cross-platform smoke tests. Compare semantic artifacts rather than trusting archive file size or success logs. Hash verification should be tested with deliberately different bytes served under the same URL/name.

Run both minimum dependency/tool versions and the supported managed toolchain where practical. Record exactly which head/tool digest a live E2E result tested. Coverage percentage is supplemental evidence; it does not establish that every workflow uses the shared contract.

## Consumer and instance acceptance

Exercise actual Prism imports, platform archive readers and generated server startup
commands. Matching an empack writer with an empack reader is necessary but does not
establish consumer compatibility. Format validity and hosting eligibility have
separate fixtures. Unrepresentable optional groups, URLs and placements fail explicitly.

Publish release A, install client and server, change local configuration and choices,
then publish B with additions, removals and runtime changes. Verify expected bytes,
retained user files, world sentinels and installed release state after update, repair,
interruption/recovery and managed rollback. Failed preparation retains A; uncertain
publication blocks launch.

Trust tests cover wrong keys, unknown algorithms, duplicate fields, changed payloads,
expired channels, sequence replay, same-sequence substitution, wrong pack/channel,
key rotation/revocation and explicit rollback without lowering the trust floor.
Test redirection and credential isolation independently from signature verification.

Compile external API fixtures that cannot construct authentication, approval or
publication proofs directly. Keep production ports honest when using test doubles.
