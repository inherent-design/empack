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

This suite requires public Mojang/Fabric/Quilt/Maven access and Java 21, selected
through `JAVA_HOME` or `PATH`. Set `EMPACK_TEST_JAVA8_HOME` to a Java 8 installation
for historical Forge. It verifies official metadata and bytes, copies prepared
runtimes into disposable directories, then runs their actual launchers. Modern
cases must print Minecraft's help options. Forge 1.7.10 and 1.12.2 ignore `--help`;
those cases must reach the EULA refusal and leave `eula=false`. No test accepts
the EULA or claims gameplay verification.

The maintained matrix covers vanilla, both Fabric launcher layouts, Quilt,
Forge 1.7.10/1.12.2/1.16.5/1.20.1 and both early/current NeoForge artifact families.
A separate case validates six official installer profiles. Ordinary offline runs
ignore these cases; the explicit task selects every case, limits concurrency to
two, and fails on unavailable prerequisites or providers. The suite complements
CLI E2E while command cutover is still pending.

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
