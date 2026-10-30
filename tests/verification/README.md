# Verification registry

`requirements.json` contains the stable UX and planned portable-authoring
obligations adopted for the v0.8.4 test overhaul. Requirements are not execution
evidence. `case_bindings_status` distinguishes pending, partial and complete
decomposition. Empty `cases` lists identify work that still needs concrete bindings;
they must not contribute passing coverage. Portable requirements remain assigned
to the v0.9 implementation checkpoints.

## Discovery

Run from the repository root. Save outputs outside the tracked source tree:

```bash
cargo nextest list --workspace --all-features --message-format json > /tmp/empack-tests-all.json
cargo nextest list --workspace --no-default-features --message-format json > /tmp/empack-tests-minimal.json
cargo run --quiet -p empack-tests --example cli-inventory > /tmp/empack-cli.json
python3 scripts/discover-python-tests.py > /tmp/empack-python-tests.json
python3 scripts/check-test-map.py --discover-tooling target/verification-inventory
```

Nextest supplies compiled Rust identities, including ignored tests. The external
`cli-inventory` example uses the compiled Clap tree; it adds no product command or
test control. Global options have one root definition. Leaf commands, local
arguments, option values, aliases and groups remain separately visible. Python
discovery uses `unittest.TestLoader` and fails on import errors or an empty suite.
Each required platform and feature selection needs its own inventory.
The registry command validates native membership for the selected host and feature
graph; cases for other platforms remain bound without being claimed as executed.
Python, rustdoc and consumer-parser identities use their own producers rather than
being searched for in Nextest's executable-test inventory.
Native Python subprocess checks of the registry are supplemental `N` evidence for
UX-088; they do not replace its required product-executable (`X`) checks.

Publisher key setup has separate executable, terminal and native cases. The
terminal case checks closed/piped input, preview and refusal before accepting the
same path with explicit approval. Destination cases preserve publisher work and
external secrets when paths are unsafe or already occupied. Native cases cover
cancellation and exclusive-publication collisions after preparation. Private
operation checkpoints also cover write, file/directory synchronization, exclusive
linking and cleanup failures on real files. Those controlled failures do not prove
kernel-generated partial writes or physical crash durability. The signing group
also binds the public Windows refusal for a shared directory, its unchanged DACLs
and the shared key-input confinement cases. Those bindings complete its definitions;
required host/feature observations remain separate acceptance obligations.

Immutable staging has separate source-validation, public inventory/signature,
approval and final-archive cases. Its case decomposition covers the original
staging requirements: selected bytes and exact release identity, signer context,
preserved immutable output, preview/refusal and five-target package execution.
This completes the staging definitions, not their acceptance: without the required
source/feature/target observations the group remains unexecuted. Hosted publication
and channel advancement have their own requirements.

Publisher enrollment binds its public decision/input case, the connected
publisher/player journey and host tests for copied roots and stale trust edits.
Its definitions cover fresh and installed contexts without treating enrollment as
installation or implicit key replacement. Execution on each host/feature graph
remains required; defining the cases supplies no passing observations.

Channel authentication combines the public publisher/player journey with pure
freshness/replay boundaries and host prelaunch refusal tests. Correctly signed
wrong-pack and wrong-channel envelopes must preserve the entire instance, not
merely its game files. Rejected high sequences cannot raise the replay floor;
accepted observation survives a later content conflict. Clock, expiry and replay
diagnostics remain distinct. Controlled host transport also proves that failed
authentication cannot fetch a release or choose an offline fallback. These cases
complete the authentication definitions; required execution is still separate.
Hosted publication and actual consumer startup have their own requirements.

Trust rotation binds public multiple-key approval, preview/refusal and revocation
to the connected update journey. Both old and replacement signers are exercised
against exact channel and release bytes. Native publication tests interrupt a
subscription replacement after durable intent, target replacement and commit,
then reopen the native state before finishing or restoring an uncommitted change.
Keys may change; the observation, subscription identity and replay floor must not
disappear. Recovery is idempotent, and retained metadata is reverified against the
recovered keys. The private hook injects controlled errors on real files; it does
not simulate an actual process kill, kernel I/O failure or power loss. These are
case definitions until the required host/feature observations are admitted.

Instance inspection has a public-command case that installs two exact snapshots,
retains a selected optional choice, and checks the displayed release history.
Both human inspection and JSON metadata status preserve the instance, external
host store and exported inputs. Invalid completed records and changed retained
descriptors must remain available for diagnosis rather than being normalized by
inspection. Metadata status still does not claim content verification; the
pending/recovery inspection matrix remains a separate obligation.

`mise run test:map` runs the same discovery and classification check as the PR
tooling job and `mise run test`. It lists Python tests, compiles the CLI inventory,
and asks rustdoc for both the complete and ignored workspace doctest listings.
Bounded logs and inventories are saved under `target/verification-inventory`.
A failed discovery cannot fall back to a prior inventory. Compilation and listing
do not execute the acceptance cases.

## Binding format

A Rust identity is `BINARY_ID::TEST_NAME`, using both values exactly as Nextest
reports them. Python identities use `python::MODULE.CLASS.METHOD`. Classification
files contain a `tests` array with exact identities and evidence boundaries. The
boundary vocabulary is:

| Code | Evidence boundary |
| --- | --- |
| U | Pure value, parser or planning check |
| H | In-process host or engine integration |
| N | Native filesystem, process, terminal or recovery |
| X | Spawned product executable |
| I | Interactive terminal workflow |
| L | Live external service or runtime dependency |
| C | Actual consumer parser, import or startup |
| P | Extracted distribution package |
| W | Actual browser host |
| M | Recorded manual acceptance |

CLI bindings contain `surfaces`, each with the exact discovered `id`, its
`definition`, and nonempty owning `requirements`. Definition comparison catches
changed option values, aliases, environment-variable names, arity, positional
indices, actions and parsing flags, as well as newly added commands. Environment
values are never included. Generated help routes belong to the help obligation;
classifying those routes does not qualify the operations they describe.

`python-bindings.json` assigns each unittest identity an evidence boundary and
requirement owner. `scripts/verify.py python` executes that fixed inventory and retains
lifecycle callbacks, including subtest and setup failures. Admission checks exact
source/input/classification identities, interpreter version/digest and complete
outcomes. Python cases use `independent`, with no empack executable identity. Pure
parser checks, controlled helper calls and actual script processes remain distinct
from executing empack or a consumer. CI rejects absent or failed tooling records;
reports retain failed attempts alongside later passes.

`doctest-bindings.json` records rustdoc identities, execution modes and reviewed ignore policy.
Compile-only examples and expected constructor-access failures do not establish
runtime behavior. Rustdoc source-line moves require an explicit binding update;
they cannot silently remove a case. Binding checks reject unknown and vanished
identities. Run `python3 scripts/verify.py doctest --test-features minimal` and
`python3 scripts/verify.py doctest --test-features all` to retain actual observations.
The pinned stable rustdoc output is checked against compiler discovery, mode and
ignore policy; missing, duplicate, filtered or unfamiliar output fails closed.
Compile-only (`compile`) and expected rejection (`compile_fail`) are separate from
executed examples (`run`). Records retain source, compiler, feature graph, bounded
logs and per-example results. The native CI gate requires both graphs on each host.
`scripts/report-verification.py` rechecks explicit receipts and retains failures
alongside later passes. These observations do not establish a product executable,
consumer launch or final-package qualification. Doctest cases declare their required
mode and both workspace feature graphs separately from product features. Reports
retain each host/graph result and source snapshot; an all-features pass cannot fill
a missing minimal run. Admission also requires the recorded requirement registry
to match the current registry, so unchanged test names cannot credit new assertions
using earlier results. A failed attempt remains a failure even when its compiler
output cannot supply individual rows. The candidate gate rechecks the same records
through `--doctest`; release jobs produce both graphs on Linux, macOS and Windows.
These unit-level observations supplement the package-boundary and executable-docs
requirements. They cannot complete fresh-user acceptance or prove the documented
engine operations actually ran merely because a `no_run` example compiled.

```bash
python3 scripts/check-test-map.py --inventory /tmp/empack-tests-all.json --bindings tests/verification/test-bindings.json --test-features all
python3 scripts/check-test-map.py --inventory /tmp/empack-tests-minimal.json --bindings tests/verification/test-bindings.json --test-features minimal
python3 scripts/check-test-map.py --python-inventory /tmp/empack-python-tests.json --python-bindings tests/verification/python-bindings.json
python3 scripts/check-test-map.py --cli-inventory /tmp/empack-cli.json --cli-bindings tests/verification/cli-bindings.json
```

`test-bindings.json` uses schema 3 to classify the minimal and all-features Rust test graphs across
macOS arm64, Linux x86_64 and Windows x86_64. Each exact Nextest identity has its
binary ID/name, boundary, owning API responsibility, required platforms, explicit
`test_features` membership and reviewed ignore policy. Each selected graph is
compared independently; matching test names across graphs do not substitute for
executing both. The minimal workspace graph disables default features but retains
features requested by dependencies, including the test harness’s `test-utils`.
Product feature selection remains separate. The registry contains expectations, not execution results. Its
platform union comes from compiler discovery; new platform-neutral tests explicitly
require every listed host before those variants have run. The `api` field identifies
implementation responsibility, not measured call coverage or completion of every
requirement routed through that API.

The runner checks this classification before executing every fixed Rust suite. Full
workspace discovery must include every expected host test and binary. Narrow suites
may omit only binaries outside their explicit selection; test filters still require
complete discovery within the selected binaries. Unknown targets, tests, ignore-policy
changes and vanished tests fail. CI rechecks the inventory and classification digest
alongside the JUnit join. Cross compilation cannot substitute for host discovery.
The two additional release execution targets (Intel macOS and ARM64 Linux) require
the corresponding OS membership too. These are expectations awaiting discovery and
execution on those hosts, not inferred passing evidence. Both minimal and all-features
membership must be checked against their own compiler inventories.

These checks validate structure and classification only. CLI, Python and doctest
bindings are also checked against actual discovery. After revalidating each selected
Rust receipt against its compiler inventory and JUnit, the CI gate joins exact test
outcomes to the concrete cases and required platform/product-feature variants.
Each variant retains its artifact and receipt digests. A missing variant is `NOT_RUN`;
conflicting results cannot erase a failed attempt. All requirements stay visible,
and a partial or pending case decomposition cannot become a passing requirement.
Neither source ownership nor a nearby test at another boundary can substitute for a
case's declared evidence. This report is not final release qualification: remaining
decomposition, non-Rust/consumer/device evidence and final-archive acceptance still
need their required evidence. A successful structural check does not qualify a release.

The entry-point group has a complete case decomposition for identity, recursive
help and usage errors at executable and package boundaries. Each test runs outside
the checkout in a path containing spaces and Unicode, with isolated clean or hostile
state/cache/home paths and no tools on `PATH`. The help matrix uses the reviewed
command registry and includes malformed `.env` and `.env.local` files. It checks
status, output, unchanged inputs and absence of new root entries. A local proxy
observes ordinary HTTP transport attempts; it is not an OS network sandbox. Package
evidence additionally requires archive admission on each of the five native targets.
Complete decomposition does not mean those execution variants have passed. Ordinary
commands still load configuration: the separate malformed-dotenv case asserts that
`version` and no-subcommand invocations report configuration errors while help/version
display flags remain usable. With valid configuration, invoking no command returns
help guidance without creating state. Recursive help checks each selected command's
own usage line, so mistakenly returning root help cannot satisfy a leaf's case.

Configuration cases connect parsed values and host loading to ordinary executable
effects. Competing dotenv, environment and CLI roots must select only the intended
workspace and invocation-relative state/cache paths. The input fixture places different
bytes and malformed dotenv in the selected workspace to expose accidental rebasing.
Preview preserves every fixture file and creates no storage; application installs the
invocation's bytes and leaves unselected workspaces intact. A separate dry-run matrix
checks whether an artifact is actually retained or removed. These complete the group's
case decomposition; platform and product-feature observations still determine acceptance.

The fixed `consumer::prism::managed-config-grammar` identity belongs to the Qt probe,
not Rust discovery. Its `independent` feature variant records that no empack product
was executed. It requires every fixture's observations, a supported dependency/build
environment, immutable source/input identity and the probe digest. The CI and candidate
gates recheck those observations before joining them to `UX-053.prism-parser`.
Unsupported or mismatched records are rejected; recorded failures remain failures.
Other managed-integration cases still need public builder, executable and actual
consumer evidence. Python tests of admission use explicitly synthetic records and
are classified as helper/script evidence, never as consumer executions.

Authoring-conformance cases use literal YAML through `manifest check --json` and
human explanation. They compare shorthand and expanded provider, URL and local
sources, check exact binding placement, and reject duplicate or competing authority.
URLs require explicit assertions or an explicit first-observation policy. Readable
flow syntax retains its editing limitations. Every case preserves author bytes and
requires absent state/cache outputs. These executable cases complement editor-schema
checks; they do not establish provider access, payload validity or complete schema
coverage. `UX-006` remains partially bound while those other obligations are joined.

Manifest explanation cases distinguish a stale document revision from an unsatisfied
selection. Literal local sources exercise source, automatic-directory and binding
edits through JSON/human inspection, materialized synchronization and a second
converged synchronization. Provider host cases cover pins, selector identity,
automatic selection and retained required dependencies using controlled catalog
records. Expected identities and destinations are literal assertions; they are not
computed by the planner under test. Inspection preserves input/storage bytes, and
unknown-key diagnostics stay on stderr. A missing declared source names the attempted
path and preserves the prior lock/materialized files. Native acquisition tests retain
the underlying I/O error and verify resource release for missing files and parents.
These checks do not claim provider freshness.

Workspace-status cases pair ordinary executable JSON and human reports with literal
malformed documents, absent/current/stale locks and mixed workspace markers. Their
expected states come from the fixture edits, not the production document decoder.
A current lock's displayed digest is checked against its raw bytes; modified payloads
remain explicitly unverified. Host cases separately cover installed layouts, root
binding, pending input, enrollment, active runtime ownership and Unix symlinks.
Status case decomposition is complete; required platform and product observations
remain necessary before the group can pass.

Presentation cases parse complete executable stdout across the three color modes,
three log formats and both requested log streams. Status, manifest, pending,
assessment, build explanation and migration-plan reports remain JSON; logs remain
observable on stderr. Refused JSON lookups emit diagnostics without a false payload,
and human reports retain their selected log stream. The telemetry suite separately
executes a telemetry product, checks flushed command/outcome spans and rejects test
API-key sentinels in its report, logs and trace. These cases reuse the existing
finalizer and Unix interruption evidence without treating a constructed result as
an actual process transition. `UX-008` remains partial pending its other platform,
interactive and final-package evidence.

Public recovery fixtures use compiler-discovered private crash drivers only to create
interrupted protocol state. The runner binds that inventory by digest in the test
process; the ordinary product receives no fault controls. The tests execute printed
commands through the labelled real shell, including adversarial path spellings, and
check selected roots, allowed actions, refusal, stale selectors and exact recovered
inventories. A real terminal checks decline and Ctrl-C at approval, preserving pending
evidence before a later explicit recovery. These are executable/interactive checks,
not human discoverability, a usable modpack, or packaged consumer qualification.

The Windows native prompt fixture checks console interruption for confirmation,
text input, selection and fuzzy selection. It invokes the real native adapter in
a private test child; the public recovery journey separately checks executable
wiring. Prompt cancellation shares the host request's token. Its scoped monitor
targets the prompt thread's synchronous I/O and retires before that thread can
continue; it injects no answers and does not terminate the thread. Windows CI must
execute these cases; a successful Unix build does not establish their behavior.
The owned console fixture restores normal Ctrl-C processing before spawning ConPTY
children, since the verification runner creates process groups with Ctrl-C suppressed.
The test deliberately starts with suppression and requires notification at the host,
retirement of the prompt read, and a usable follow-up prompt on the same thread.

APFS cleanup retries only the owned tool's OS busy status, with a finite attempt
bound and fresh backing-to-device ownership before each request. Other failures
remain failures. Diagnostics query open files only after confirming the exact
owned mount; they cannot fall back to the host filesystem. The opt-in regression
reassigns devices between busy failure and retry, checks that the other backing
stays attached, and retains permanently busy backing until its reader is closed.
This retries resource cleanup, not a failed product workflow or test scenario.

`mise run coverage` uses [run-coverage.py](../../scripts/run-coverage.py) to obtain the
instrumentation environment, build the product, and capture the instrumented Nextest
inventory before execution. Its digest is supplied to test processes; product children
still receive no fixture controls. Test and report failures remain separate, and a
successful report cannot erase a failed test run. Coverage does not produce release
qualification receipts.

Cancellation and recovery bindings distinguish constructed finalizer results, real
engine ownership, and an ordinary Unix executable chain. The latter launches a
controlled child, changes its ownership record, sends a host interrupt, and checks
actual stderr, status, refusal of another launch, and explicit recovery. It also
checks clean interruption and classified nonzero child status. Those cases remain
partial: actual terminal loss at the executable, Windows cancellation wiring and
consumer Java behavior need their own evidence.

The bound authoring and pending build/import/synchronization groups remain partial.
Synchronization exercises public association keys, partial retention after cache
eviction, restart, exact materialized bytes, stale drafts and explicit cleanup.
Import uses a real unsupported-transport obligation and public diagnostics, rejects
wrong/ambiguous associations, resumes after deleting the original archive, and verifies
exact exported bytes. It preserves the distinction between saved input and a complete
assessment of missing requirements. This offline fixture does not establish a live
provider's restricted-download policy. Instance pending journeys, corrupt/ambiguous
records and interactive import approval remain separate obligations. Required platform
and product-feature declarations do not establish that those variants ran.

## Package context and candidate qualification

A `P` case can reference an `X` executable test, provided the archive gate has bound
that execution to the built and extracted final artifact. Ordinary suite receipts
cannot assert this context themselves. `required_targets` enumerates the exact five
release triples alongside the required OS and feature variants; missing ARM64 or
Intel execution remains `NOT_RUN` independently of other hosts on the same OS.

The candidate gate requires complete case decomposition and coverage of each declared
evidence boundary for every applicable group. All 88 native groups remain mandatory;
from v0.9 the 21 portable groups join them. Pending or partial groups, absent consumer
observations and failed attempts block qualification. The current registry and
workflow evidence are incomplete, so no candidate is qualified by this wiring alone.
See [candidate commands and remaining work](../../docs/testing.md#coverage-and-release-qualification).

Auxiliary candidate builds bind a feature variant to reviewed source, stamped inputs,
pinned native compiler, fixed product-only command and retained executable bytes.
Their shared-runner outcomes join ordinary feature cases; only archive admission can
satisfy package cases. `check-auxiliary-build.py` produces the telemetry bundle and
`qualify-release.py --auxiliary` rechecks it. Synthetic admission fixtures are helper
evidence; actual telemetry execution requires the producer's retained suite results.

Replacement cases join host preparation, public commands and native publication.
Malformed archives, path collisions and invalid initialization options run against a
populated existing project. Preview and terminal decline compare file bytes, empty
directories and native attributes; approved replacement checks its explicit source
scope and preserves templates, prior artifacts and unrelated world sentinels. Native
capture tests retain intervening edits and reject stale read sets or cancellation.
Digest/transfer failures use controlled host transport, not a live provider claim.
Unix link confinement is a separate platform case. These bindings complete the
replacement group's case decomposition; each required host/feature still needs its
own admitted execution evidence.

New-project initialization uses a literal author-document expectation shared by
unattended and PTY/ConPTY checks. It compares the complete document and scaffold
inventory, exact empty runtime lock, reference defaults and template integration
fields without calling the production document decoder. Empty bindings stay absent.
Repeated synchronization preserves authored bytes and identity; a second initialization
explains replacement refusal. Host state is isolated outside the author tree.
The package case repeats these checks on all five final archives; local execution
does not satisfy that case. Generated workflow presence/version checks remain
separate from execution of those workflows under UX-081/UX-082.

Loader-family initialization separates exact declarations, controlled discovery and
fresh official routes. The former ignored explicit NeoForge CLI test made no
catalog request; its assertions now belong to the deterministic exact-pin matrix
in `cli_init`, covering every loader family with an empty executable search path
and an observed proxy. Live CLI cases omit a required version, use fresh isolated
projects and compare authored intent with the resolved lock. Host fixtures cover
stable/prerelease selection, historical coordinates and failure preservation.
Java 8/21 in these cases is an authored requirement, not evidence that Java ran.
Runtime acquisition and consumer startup remain separate obligations.

Generated automation has separate helper, executable and package evidence. Direct
Python tests use mocked commands and real filesystem I/O, classified as H or N
according to the asserted boundary rather than as pure U checks. `cli_automation` runs the script emitted by `init` with the
selected product and an isolated environment. The shared runner supplies its exact
Python interpreter to that test harness; the product receives no test selector.
Validation checks named profiles, exact staged bytes/checksums and stale-output
exclusion on native hosts and, separately, all five extracted archives.

The generated publication workflow targets Ubuntu. Linux/macOS tests run its helper
with the real product and a POSIX GitHub command recorder, checking early refusal,
upload failure and reveal ordering. These tests have no publisher credentials and
cannot contact GitHub. They do not complete UX-082: actual authorized sandbox
publication evidence remains a distinct manual-candidate obligation.

The local mrpack journey (`UX-012`) compares the complete exported index and override
inventory against literal values, then imports the resulting archive into another
project and exports again. It covers common/client/server files, ordered URL fallbacks,
strong and weak original assertions, observed export digests and optional client-only
content. Mrpack cannot store the authoring default for an optional choice: a second
import must ask for it again. Choosing a different default preserves optional
participation in the exported format. Imported dependency coverage remains partial;
removing a root while retaining other imported roots requires explicit acknowledgement.
The host conversion cases and five shipping-archive cases remain separate evidence.

Local JAR ownership (`UX-020`) binds host source-retention/adoption/refusal tests to
one literal public authoring journey. The latter checks source paths with spaces,
changed-source update and adoption, repeated synchronization, exact exported JAR
bytes and removal confinement. The Unix linked-ancestor assertion has a separate
platform case; it does not imply a Windows junction test. All five final archives
must repeat the public journey through package admission. Complete case definitions
are not evidence that those host, feature or package variants have run.
