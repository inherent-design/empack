# Verification for the v0.5 target

Every target guarantee needs a test at the layer that owns it. The normative
[contract suites and fault model](design/verification.md) replace test-count goals
as the release gate. Passing legacy suites remains required during migration.

```bash
mise run check
mise run clippy
mise run test
mise run smoke
mise run e2e:strict
```

Strict E2E requires the managed or configured packwiz backend, Java, network
access and provider credentials required by the selected fixtures. Missing
prerequisites fail strict execution. Never store credentials in test reports.

The pure core must compile without runtime or filesystem dependencies. Its tests
cover portable syntax, typed values and pure decisions. Native filesystem,
process, lock and recovery guarantees require real platform tests. A mock default
that succeeds is not evidence for an unimplemented port.

The [implementation ledger](design/implementation.md) links each landing to its
checks. Public API examples become compiled examples when their APIs land.
Unimplemented engine sketches are explicitly excluded from current API promises.

Publication cannot ship on ordinary happy-path tests alone. Restart a fresh
process after every durable boundary listed in the fault model; require unchanged,
verified committed, or explicitly recovery-required state. Preserve prior usable
artifacts on preparation and verification failure.

[Baseline verification](history/verification-0.4.md) records tests, coverage and
curated fixtures by revision. Those counts do not establish coverage for the new
engine. New evidence must identify the tested revision and actual commands.
