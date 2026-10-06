# Verification for the v0.5 target

Every target guarantee needs a test at the layer that owns it. The normative
[contract suites and fault model](design/verification.md) replace test-count goals
as the release gate. Retain existing tests where they exercise intended behavior; replace tests that
encode obsolete behavior.

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

Run the live normalized-runtime checks explicitly:

```bash
mise run smoke:runtime
```

This suite requires public Mojang/Fabric/Quilt/Maven access and Java 17 or 21, selected
through `JAVA_HOME` or `PATH`. It verifies official metadata and bytes, copies the
prepared runtime into disposable directories, then runs the actual server launcher
with `--help`. It checks vanilla, both historical/modern Fabric layouts and Quilt
without accepting the EULA. These tests are marked ignored in ordinary offline runs; the
explicit task selects all of them and fails on missing prerequisites or providers.
It complements CLI E2E while command cutover is still pending.

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

Historical test evidence remains in Git history. It does not establish coverage
for the new engine. New evidence identifies the tested revision and actual commands.
