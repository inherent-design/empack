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

The library unit suite also runs without `test-utils`:

```bash
cargo nextest run -p empack-lib --lib --no-default-features
```

PR lint checks this configuration separately. Workspace feature unification must
not hide dependencies on helpers needed by ordinary unit tests.

Strict E2E requires the managed or configured packwiz backend, Java, network
access and provider credentials required by the selected fixtures. Missing
prerequisites fail strict execution. Never store credentials in test reports.

Run the live normalized-runtime checks explicitly:

```bash
mise run smoke:runtime
```

This suite requires public Mojang/Fabric/Quilt/Maven access and Java 21, selected
through `JAVA_HOME` or `PATH`. Set `EMPACK_TEST_JAVA8_HOME` to a Java 8 installation
for historical Forge. It enters the build `Engine` through read-only preparation
and an explicit execution grant, verifies official metadata and bytes, builds full
server archives, publishes them through the journal, extracts them
into disposable directories, then runs their generated startup scripts. A current
configuration file must also survive packaging. Modern
cases must print Minecraft's help options. Forge 1.7.10 and 1.12.2 ignore `--help`;
those cases must reach the EULA refusal and leave `eula=false`. No test accepts
the EULA or claims gameplay verification.

The maintained matrix covers vanilla, both Fabric launcher layouts, Quilt,
Forge 1.7.10/1.12.2/1.16.5/1.20.1 and both early/current NeoForge artifact families.
A separate case validates six official installer profiles. Ordinary offline runs
ignore these cases; the explicit task selects every case, limits concurrency to
two, and fails on unavailable prerequisites or providers. The suite complements
CLI E2E while command cutover is still pending.

Run official provider identity and byte verification with:

```bash
mise run smoke:providers
```

Set `EMPACK_KEY_CURSEFORGE` for this explicit suite. Missing credentials fail the
CurseForge case. The three probes resolve Modrinth mod/resource-pack and CurseForge
mod selectors, verify exact file ownership, download the selected file and check
its original digest and size assertions. The resource-pack case additionally builds
and publishes a full-client ZIP through the Engine from a lock with no stored URL,
then checks its member against the original source digest and verifies that the
intent and lock did not change. Three further probes resolve compatible Modrinth and
CurseForge versions under an explicit stable-preferred policy and verify the selected
files against their original hashes and sizes. Terralith exercises datapack selection
from a project that also publishes mod versions. These three compatible probes also
require official search to retain the expected canonical project among its choices.
Search ranking is not treated as installation authority. Ordinary offline runs ignore these
network cases.

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


The content-pool and large-local-build regressions run in isolated child processes.
On Unix the child has a 256-descriptor limit; Windows runs the same content and
publication assertions under its native handle model. The build fixture publishes
600 distinct configuration files into both a client ZIP and an mrpack, then reads
all packaged bytes. These checks exercise retained readers and actual assembly,
not only the resource ledger. The parent test process keeps its normal limits.

### Normalized import adapter smoke

Set `EMPACK_TEST_IMPORT_ARCHIVES` to existing mrpack/CurseForge archive paths, using
the host path-list separator (`:` on Unix, `;` on Windows), then run
`mise run smoke:import:adapters`. Missing input fails the opt-in test. It captures
each source privately, uses the normalized engine adapter and verifies every
referenced embedded member against its original declarations. It does not download
manifest files, resolve provider references or publish a project.

Local verification covered Fabulously Optimized 1.20.1 in both formats: 51 mrpack
file declarations and 27 embedded members; 46 CurseForge exact references and 32
embedded members. CurseForge fixture file `4800279` was acquired with the provider's
size/hash assertions checked. This is adapter evidence, not end-to-end import parity.

The three exact-provider smoke cases also identify the acquired bytes through the
same provider and require the returned canonical owner and matching file role.
They cover a Modrinth mod, a resource pack and CurseForge content. The original
provider integrity declarations remain unchanged. Each exact case also resolves its
version/file ID without a supplied project and compares canonical ownership and every
file assertion with the project-qualified lookup. The three compatible-version probes
remain separate checks within the `smoke:providers` suite. A seventh case expands
Reese's Sodium Options, requires the Sodium edge to remain present and verifies every
selected file against its original size/digests. This covers live closure evidence and
acquisition, not yet add/sync command composition.


Run normalized import acquisition, candidate assembly and temporary-project publication with
`EMPACK_TEST_IMPORT_ARCHIVES` set to local archive paths and
`mise run smoke:import:content`. CurseForge inputs require the provider key.
Missing manual content fails this probe; it is not counted as complete acquisition.
For paired fixtures, `EMPACK_TEST_IMPORT_USE_PRIOR_BYTES=1` explicitly permits bytes
verified from an earlier archive to satisfy a later manual obligation. Association
requires the original digest and size; acquisition checks those bytes again. This
fixture policy is not a production download-discovery heuristic.

The deterministic acquisition tests cover changed remote bytes, later provider
failure, cumulative catalog/transfer allowances, restricted-file preflight, wrong
supplied content, retained resource ownership and client/server/optional semantics.
The probe also assembles coherent intent/lock documents using explicit fixture choices,
publishes into an existing empty temporary directory, and rechecks every placed file
against its original assertions. It explicitly excludes only the known generated
CurseForge `modlist.html` report and records that decision. Unknown auxiliary members
still fail. Deterministic tests cover forced replacement from a malformed manifest,
unrelated-file preservation, publication conflicts, cancellation and layered re-export.
These checks do not yet establish Engine import approval or CLI parity.
