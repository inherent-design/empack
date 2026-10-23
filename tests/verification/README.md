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
