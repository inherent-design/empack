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
requirement owner. Pure parser checks, controlled helper calls and actual script
processes remain distinct from executing empack or a consumer. `doctest-bindings.json`
records rustdoc identities and reviewed ignore policy. Compile-only examples and
expected constructor-access failures do not establish runtime behavior. Rustdoc
source-line moves require an explicit binding update; they cannot silently remove
a case. Binding checks reject unknown and vanished identities.

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
