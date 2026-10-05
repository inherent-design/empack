# Helpers and acceptance tests

Target contract for v0.5.0-alpha.1. Code blocks are design sketches unless the
[implementation ledger](implementation.md) identifies a compiled API.

## 19. Shared helpers and utilities

Utilities should be cohesive modules with narrow purposes, not a large `utils.rs` or another generic framework.

| Helper | Responsibility and contract | Where used |
|---|---|---|
| `SelectorParser` | Parse user syntax without claiming canonical resolution. | CLI, import URL normalization. |
| `CanonicalIdentityIndex` | Index logical records and observed files by canonical identity; return ambiguity rather than first match. | Add, sync, remove, adopt. |
| `DocumentPatch` | Apply exact logical changes against expected raw document revision; preserve unrelated syntax/fields. | Manifest/backend publication. |
| `CanonicalHasher` | Domain-separated, versioned, stable encoding; sort unordered maps, preserve meaningful sequence order. | Semantic revisions, recipes, plan grants. |
| `DigestVerifier` | Incremental algorithms and expected-size checks; report all declared supported digest mismatches. | Acquisition, manual association, artifact verification. |
| `ContentProbe` | Bounded type/header/archive inspection with observed digest and provenance. | Direct JAR/ZIP and local file add. |
| `PathCollisionIndex` | Detect duplicate and portable collisions without rewriting spellings. | Imports, layered projections, artifact parsing. |
| `LayerResolver` | Apply declared common/side precedence and emit replacement evidence. | Import round trips and builds. |
| `ReadSetBuilder` | Track files, absence, and directory membership; separate raw and semantic revisions. | Snapshotter, publisher conflict checks. |
| `BoundedCopy` | Stream with checked byte count and a bounded extra read to detect oversized input. | Downloads, extraction, staging. |
| `QuotaWriter` | Aggregate actual expanded/output bytes across entries and stop before unauthorized writes. | Archive handling. |
| `AtomicFileWriter` | Same-filesystem staged replacement, permission policy, durability result. | Publisher, durable host-state documents. |
| `ExpectedOldCheck` | Compare kind/content/root identity immediately within the publication protocol. | File changes and recovery. |
| `CandidateHashCache` | Hash stable manual-download candidates once per discovery pass. | Restricted downloads. |
| `Deadline` and `Clock` | Shared monotonic budgets, testable fake time, no retry resetting total deadline. | HTTP, process supervision, waits. |
| `RetryClassifier` | Distinguish not-found, unauthorized, rate-limited, transient, permanent, and unknown failures. | Provider transport. |
| `CredentialRedactor` | Secret-safe debug/display types; strip signed query/header details. | Logs, receipts, error context. |
| `ToolProbe` | Bounded process probe requiring valid status and capability data. | Lazy tool resolution. |
| `AdmissionGate` | Atomic close versus admission/task registration; explicit busy/oversized states. | Runtime scopes. |
| `ResultRegistry` | Retain terminal results independently of lossy progress notifications. | Engine handles, CLI, embedding. |
| `PostconditionReporter` | Stable diagnostic codes identifying expected versus actual domain state. | Verify and recovery. |

Prefer established parsers, HTTP clients, cryptographic implementations, archive libraries, and OS primitives behind these wrappers. Write the **policy composition** empack needs; do not implement cryptographic algorithms or platform process ownership from scratch unnecessarily.

### 19.1 A bounded-copy helper with an explicit contract

This standalone sketch shows the intended utility style: one concrete function, ordinary error handling, no generalized pipeline framework.

```rust
use std::io::{self, Read, Write};

pub fn copy_bounded<R: Read, W: Write>(
    source: &mut R,
    destination: &mut W,
    limit: u64,
) -> io::Result<u64> {
    let mut buffer = [0_u8; 32 * 1024];
    let mut copied = 0_u64;

    loop {
        let remaining = limit - copied;
        if remaining == 0 {
            // Read at most one additional byte to distinguish exact-size EOF
            // from an oversized input. Never write that extra byte.
            let mut probe = [0_u8; 1];
            let n = loop {
                match source.read(&mut probe) {
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    result => break result?,
                }
            };
            return if n == 0 {
                Ok(copied)
            } else {
                Err(io::Error::new(io::ErrorKind::InvalidData, "input exceeds limit"))
            };
        }

        let allowed = remaining.min(buffer.len() as u64) as usize;
        let n = match source.read(&mut buffer[..allowed]) {
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if n == 0 {
            return Ok(copied);
        }
        destination.write_all(&buffer[..n])?;
        copied += n as u64; // Bounded by `remaining`, so cannot exceed `limit`.
    }
}
```

This function limits bytes, not time, decompression CPU, or retained memory elsewhere. A blocking reader can still stall. The async transfer layer enforces cancellation/deadlines; a blocking extraction worker adds checkpoints or process isolation where warranted. A failed copy leaves partial output in quarantine, never a published destination.

Wrap this functionality for hashing and aggregate quotas rather than duplicating loops. Tests cover limit zero, exact EOF, one byte over, interrupted reads, short writes, write failure, and a maximum representable limit.

### 19.2 Canonical encoding contract

Do not hash Rust `Debug` output, randomized `HashMap` traversal, raw YAML formatting, or platform-native absolute paths. `CanonicalHasher` consumes tagged, length-delimited values with a schema/domain prefix such as `empack.recipe.v1`.

Map ordering is canonical by validated key. Order-sensitive URL alternatives, template precedence, or command arguments remain ordered. Include distinctions such as missing versus explicitly empty only where they affect semantics. Store raw document hashes separately for edit protection.

No helper named `normalize` should silently alter content meaning. Prefer `parse_selector`, `canonicalize_provider_identity`, `validate_rel_path`, `resolve_layers`, and `fingerprint_recipe` so callers can see which contract is established.


## 20. Contract tests and failure injection

### 20.1 Compile and architecture gates

Add a build gate proving `empack-core` has no runtime/I/O dependencies. Keep private-proof constructor visibility covered by compile-fail tests or carefully selected external-crate tests. Require all proposed `Arc<dyn Port>` traits to be compiled in at least one concrete mock and one real composition root.

Turn public usage examples into checked examples as the API lands. Until then, keep them explicitly marked as design sketches. No unchecked `todo!`, success-returning placeholder verifier, or permissive mock default should enter a production execution path.

### 20.2 Reusable adapter suites

| Suite | Required behaviors |
|---|---|
| Provider catalog | Equivalent slug/ID/URL identity; pin ownership; type discovery; distinguish authorization, not-found, rate limit, and outage. |
| Import adapter | Preserve common/side layers, requirements, URLs/digests, embedded files, layout inference; reject unsupported conversions before live effects. |
| Acquisition | Chunked/false-length limits; stalled headers/body cancellation; digest mismatch; scoped redirects; no persistent writes under read-only cache policy. |
| Filesystem | Symlink/junction ancestors, case collisions, absent targets, hardlink copy safety, permissions, cross-device publication rejection. |
| Backend | Observed exact pin/identity, dependency additions, structured partial failure, success-with-missing-output rejection. |
| Artifact writer/reader | Inventory completeness for all sources; duplicate paths; optional choices; deterministic settings where promised. |
| Process runner | Closed pipes with live child; live descendant after parent exit; output flooding; timeout during drain; cancellation and job retirement. |
| Publisher | Every durable crash point; expected-old conflict; before/after/neither recovery classification; prior artifact retained. |
| Runtime | Close/admit race, cancel-before-start, cancel-after-result, late result from superseded attempt, permit retention, lost progress notification. |

Unit-test fakes should be strict: unsupported operations fail, not silently succeed. Integration tests of native confinement, process trees, locks, and durability need real OS primitives. An in-memory filesystem proves planner logic, not native filesystem safety.

### 20.3 Cross-command properties

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

Equality means the relevant semantic state, not timestamps or nonessential diagnostic ordering. Property generators should vary provider/local/URL/embedded sources, aliases, pins, environments, paths, empty/missing files, and partial backend behavior.

A particularly valuable scenario is `add -> forced re-add under alias -> sync -> build -> remove -> sync`. It catches contracts that look correct in isolation but disagree across command boundaries.

### 20.4 Publication fault model

Inject abrupt termination after: preimage write, preimage sync, journal-intent sync, candidate replace, target-directory sync, progress-record write, post-publication verification, committed-record sync, and preimage reclamation. Restart a fresh process after each injected failure.

Assert one of three truthful states: unchanged and retryable; committed and verified; or explicitly recovery-required with preserved recovery data. Never accept silent mixed state as a configured/healthy project.

Also inject disk-full, permission changes, antivirus/in-use-file failures where reproducible, cross-volume staging, corrupted/truncated journal, unknown schema, concurrent external edit, and missing retained content. SQLite's crash-testing approach is a useful precedent for focusing on durable boundaries rather than only ordinary exception paths. [R8](https://sqlite.org/atomiccommit.html)

### 20.5 Feature and conformance fixtures

Retain real pack fixtures and cross-platform smoke tests. Compare semantic artifacts rather than trusting archive file size or success logs. Hash verification should be tested with deliberately different bytes served under the same URL/name.

Run both minimum dependency/tool versions and the supported managed toolchain where practical. Record exactly which head/tool digest a live E2E result tested. Coverage percentage is supplemental evidence; it does not establish that every workflow uses the shared contract.
