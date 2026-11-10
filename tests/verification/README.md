# Verification registry

[requirements.json](requirements.json) owns the stable UX and planned portable-authoring obligations, concrete cases, and required execution variants. Requirements are not execution evidence. `case_bindings_status` distinguishes pending, partial, and complete decomposition; empty or incomplete cases cannot contribute passing coverage. Portable requirements remain assigned to their implementation checkpoints. Use the registry for per-workflow assertions, the [testing guide](../../docs/testing.md) for commands and prerequisites, and retained reports for actual results.

## Discovery

Run from the repository root. Save outputs outside the tracked source tree:

```bash
cargo nextest list --workspace --all-features --message-format json > /tmp/empack-tests-all.json
cargo nextest list --workspace --no-default-features --message-format json > /tmp/empack-tests-minimal.json
cargo run --quiet -p empack-tests --example cli-inventory > /tmp/empack-cli.json
python3 scripts/discover-python-tests.py > /tmp/empack-python-tests.json
python3 scripts/check-test-map.py --discover-tooling target/verification-inventory
```

Nextest supplies compiled Rust identities, including ignored tests. The external `cli-inventory` example uses the compiled Clap tree; it adds no product command or test control. Global options have one root definition. Leaf commands, local arguments, option values, aliases and groups remain separately visible. Python discovery uses `unittest.TestLoader` and fails on import errors or an empty suite. Each required platform and feature selection needs its own inventory. The registry command validates native membership for the selected host and feature graph; cases for other platforms remain bound without being claimed as executed. Python, rustdoc, editor-schema and consumer-parser identities use their own producers rather than being searched for in Nextest's executable-test inventory. `verify.py schema` builds an ordinary default product and runs the fixed editor schema/CLI corpus through public `manifest check --json`. Its receipt retains raw validation, response and whole-workspace observations, plus product/source hashes and Cargo feature evidence. The CI gate requires exactly one passing Linux attempt even for docs/schema-only edits. Admission rejects altered inputs, incomplete fixtures, ambiguous product identity and false success summaries. This single conformance case cannot replace the other authoring observations. The Linux candidate driver invokes this producer with the admitted extracted default binary. Its receipt must match the archive's digest, version, target and release profile before entering qualification. Missing, duplicate, failed or unrelated attempts remain incomplete; ordinary CI product evidence cannot replace that candidate receipt.

`mise run test:map` runs the same discovery and classification check as the PR tooling job and `mise run test`. It lists Python tests, compiles the CLI inventory, and asks rustdoc for both the complete and ignored workspace doctest listings. Bounded logs and inventories are saved under `target/verification-inventory`. A failed discovery cannot fall back to a prior inventory. Compilation and listing do not execute the acceptance cases.

## Binding format

A Rust identity is `BINARY_ID::TEST_NAME`, using both values exactly as Nextest reports them. Python identities use `python::MODULE.CLASS.METHOD`. Classification files contain a `tests` array with exact identities and evidence boundaries. The boundary vocabulary is:

| Code | Evidence boundary                                |
| ---- | ------------------------------------------------ |
| U    | Pure value, parser or planning check             |
| H    | In-process host or engine integration            |
| N    | Native filesystem, process, terminal or recovery |
| X    | Spawned product executable                       |
| I    | Interactive terminal workflow                    |
| L    | Live external service or runtime dependency      |
| C    | Actual consumer parser, import or startup        |
| P    | Extracted distribution package                   |
| W    | Actual browser host                              |
| M    | Recorded manual acceptance                       |

CLI bindings contain `surfaces`, each with the exact discovered `id`, its `definition`, and nonempty owning `requirements`. Definition comparison catches changed option values, aliases, environment-variable names, arity, positional indices, actions and parsing flags, as well as newly added commands. Environment values are never included. Generated help routes belong to the help obligation; classifying those routes does not qualify the operations they describe.

`python-bindings.json` assigns each unittest identity an evidence boundary and requirement owner. `scripts/verify.py python` executes that fixed inventory and retains lifecycle callbacks, including subtest and setup failures. Admission checks exact source/input/classification identities, interpreter version/digest and complete outcomes. Python cases use `independent`, with no empack executable identity. Pure parser checks, controlled helper calls and actual script processes remain distinct from executing empack or a consumer. CI rejects absent or failed tooling records; reports retain failed attempts alongside later passes.

`doctest-bindings.json` records rustdoc identities, execution modes (`run`, `compile`, or `compile_fail`), feature graphs, and reviewed ignore policy. Source-line moves require explicit binding updates; unknown, missing, duplicate, filtered, or unfamiliar discovery/output fails closed. Compile-only examples and expected constructor-access failures do not establish runtime behavior. Run `python3 scripts/verify.py doctest --test-features minimal` and `python3 scripts/verify.py doctest --test-features all` for actual observations. Records bind source, compiler, requirement registry, workspace test graph, bounded logs, and per-example results. Each required host/graph must have its own evidence; an all-features pass cannot fill a missing minimal run. A failed attempt remains a failure even if no individual result rows were produced. Candidate admission rechecks records through `--doctest`. These observations supplement executable and package cases; they do not establish consumer startup or fresh-user acceptance.

```bash
python3 scripts/check-test-map.py --inventory /tmp/empack-tests-all.json --bindings tests/verification/test-bindings.json --test-features all
python3 scripts/check-test-map.py --inventory /tmp/empack-tests-minimal.json --bindings tests/verification/test-bindings.json --test-features minimal
python3 scripts/check-test-map.py --python-inventory /tmp/empack-python-tests.json --python-bindings tests/verification/python-bindings.json
python3 scripts/check-test-map.py --cli-inventory /tmp/empack-cli.json --cli-bindings tests/verification/cli-bindings.json
```

`test-bindings.json` uses schema 3 to classify the minimal and all-features Rust test graphs across macOS arm64, Linux x86_64 and Windows x86_64. Each exact Nextest identity has its binary ID/name, boundary, owning API responsibility, required platforms, explicit `test_features` membership and reviewed ignore policy. Each selected graph is compared independently; matching test names across graphs do not substitute for executing both. The minimal workspace graph disables default features but retains features requested by dependencies, including the test harness’s `test-utils`. Product feature selection remains separate. The registry contains expectations, not execution results. Its platform union comes from compiler discovery; new platform-neutral tests explicitly require every listed host before those variants have run. The `api` field identifies implementation responsibility, not measured call coverage or completion of every requirement routed through that API.

The runner checks this classification before executing every fixed Rust suite. Full workspace discovery must include every expected host test and binary. Narrow suites may omit only binaries outside their explicit selection; test filters still require complete discovery within the selected binaries. Unknown targets, tests, ignore-policy changes and vanished tests fail. CI rechecks the inventory and classification digest alongside the JUnit join. Cross compilation cannot substitute for host discovery. The two additional release execution targets (Intel macOS and ARM64 Linux) require the corresponding OS membership too. These are expectations awaiting discovery and execution on those hosts, not inferred passing evidence. Both minimal and all-features membership must be checked against their own compiler inventories.

These checks validate structure and classification. After revalidating each selected receipt against its discovery and observed results, the CI gate joins exact test outcomes to cases and required host/product variants. Each variant retains its artifact and receipt digests. Missing variants remain `NOT_RUN`; conflicting results cannot erase a failed attempt. Partial or pending decomposition cannot become a passing requirement, and evidence at another boundary cannot substitute for a declared case. This report is separate from final release qualification.

The fixed `consumer::prism::managed-config-grammar` identity belongs to the Qt probe, not Rust discovery. Its `independent` variant identifies parser evidence without an empack product execution. Admission requires every fixture observation, a supported consumer dependency/build environment, immutable input identity, and the probe digest. Public build preparation, server entry points, and actual launcher startup have separate cases. Synthetic admission fixtures cannot supply consumer observations. See [Managed consumer interpretation](../../docs/testing.md#managed-consumer-interpretation) for the probe procedure.

`mise run coverage` uses [run-coverage.py](../../scripts/run-coverage.py) to obtain the instrumentation environment, build the product, and capture the instrumented Nextest inventory before execution. Its digest is supplied to test processes; product children still receive no fixture controls. Test and report failures remain separate, and a successful report cannot erase a failed test run. Coverage does not produce release qualification receipts.

## Package context and candidate qualification

A `P` case can reference an `X` executable test, provided the archive gate has bound that execution to the built and extracted final artifact. Ordinary suite receipts cannot assert this context themselves. `required_targets` enumerates the exact five release triples alongside the required OS and feature variants; missing ARM64 or Intel execution remains `NOT_RUN` independently of other hosts on the same OS.

Candidate qualification requires complete decomposition and every declared evidence boundary for each applicable group. All automated native obligations remain mandatory. For v0.8.4-beta only, the registry names 30 operator-only cases deferred by the maintainer because their human/consumer environments are unavailable. UX-084 contains only those deferred cases; the other 87 native groups retain blocking automated obligations. From v0.9, the full declared native scope and the 21 portable groups apply. Pending or partial required groups, missing required observations, and failed attempts block qualification. Reports preserve observed `status` separately from `release_status`; a deferred unrun case remains `NOT_RUN`, never `PASS`. Environment deferrals cannot exclude automated cases and do not suppress supplied failed receipts. Inspect the candidate report for actual results; the presence of these rules establishes no pass. See [Coverage and release qualification](../../docs/testing.md#coverage-and-release-qualification) and [Consumer and human acceptance records](../../docs/verification-acceptance.md).

Auxiliary candidate builds bind a feature variant to reviewed source, stamped inputs, pinned native compiler, fixed product-only command and retained executable bytes. Their shared-runner outcomes join ordinary feature cases; only archive admission can satisfy package cases. `check-auxiliary-build.py` produces the telemetry bundle and `qualify-release.py --auxiliary` rechecks it. Synthetic admission fixtures are helper evidence; actual telemetry execution requires the producer's retained suite results.
