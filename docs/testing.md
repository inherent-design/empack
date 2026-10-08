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

On macOS, use Nextest 0.9.145 or newer. Earlier runners can report sibling test
capture pipes as leaked when tests start concurrently; this was fixed in
[Nextest 0.9.145](https://www.nexte.st/changelog/#09145---2026-09-16).
A current runner does not excuse leaks from empack's own processes.

The library unit suite also runs without `test-utils`:

```bash
cargo nextest run -p empack-lib --lib --no-default-features
```

PR lint checks this configuration separately. Workspace feature unification must
not hide dependencies on helpers needed by ordinary unit tests.

Strict E2E requires the prerequisites of the selected fixtures: native archive and
restricted-continuation cases need the built executable; live bootstrap/runtime cases
need network access and sometimes Java; provider cases need their declared credentials.
Native workflows and their fixtures do not bootstrap or require packwiz-tx.
Missing prerequisites fail
strict execution. Never store credentials in test reports.

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
two, and fails on unavailable prerequisites or providers. The suite complements CLI E2E by checking actual Java launchers and historical
runtime families through the same native engine.

Run official provider identity and byte verification with:

```bash
mise run smoke:providers
```

Set `EMPACK_KEY_CURSEFORGE` for this explicit suite. Missing credentials fail the
CurseForge case. The seven probes cover Modrinth and CurseForge selectors, compatible selections,
required closure and mixed datapack projects. They verify exact file ownership,
download selected files and check original digest and size assertions. The resource-pack case additionally builds
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
acquisition. Native command-composition tests separately cover add and repeated sync.


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
These adapter checks are supplemented by engine approval tests and native executable
import/continuation/sync/build tests; adapter success alone does not establish CLI parity.


### Coverage profile collection

`mise run coverage` requires all selected tests to pass, including strict E2E in CI.
LLVM merges the valid profiles and reports malformed profiles as warnings. A corrupt
profile from an interrupted process cannot discard the rest of a successful run; a
collection with no valid profiles still fails. Missing counters contribute no execution
evidence. This does not enable `--ignore-run-fail`, exclude source files, or lower any
test requirement. The behavior follows LLVM's documented
[`--failure-mode=all`](https://llvm.org/docs/CommandGuide/llvm-profdata.html#cmdoption-llvm-profdata-merge-failure-mode).

CI prints one coverage summary in its logs and uses that same report for the job summary.
The per-file table reports line counts, rather than mixing region counts with line percentages.
Inline test code remains included in compiler coverage; a percentage is not proof of feature
completion or a production-only coverage measurement.

The native build E2E family constructs tracked files through the executable and checks
packaged bytes with independent ZIP readers. Restricted-build fixtures use native intent,
lock and saved recipes with an unavailable packwiz executable. They verify read-only
preview, failed-byte preservation, exact slot association, resumed mrpack content and
all-target artifact preservation before explicit saved-recipe cleanup.

## Curated executable workflows

`mise run smoke:import:curated` imports seven maintained packs, including historical
Forge and both provider formats. Each fixture owns an isolated native state/cache
root. The harness supplies explicit optional and auxiliary-content choices, exercises
restricted-file `init --continue` with a read-only preview, checks two unchanged syncs,
and inspects a full-client archive. It does not read v0.4 continuation files or retry
failed builds through an alternate implementation. Stage failures remain failures.

The offline driver contracts run with `python3 -m unittest discover -s scripts/tests`
and are included in `mise run test`. The executable override is resolved before any
child working-directory change.
