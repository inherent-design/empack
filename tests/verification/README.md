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
python3 scripts/check-test-map.py
```

Nextest supplies compiled Rust identities, including ignored tests. The external
`cli-inventory` example uses the compiled Clap tree; it adds no product command or
test control. Global options have one root definition. Leaf commands, local
arguments, option values, aliases and groups remain separately visible. Python
discovery uses `unittest.TestLoader` and fails on import errors or an empty suite.
Each required platform and feature selection needs its own inventory.

## Binding format

A Rust identity is `BINARY_ID::TEST_NAME`, using both values exactly as Nextest
reports them. Python identities use `python::MODULE.CLASS.METHOD`. Classification
files contain a `tests` array with `id` and `boundary`. The boundary vocabulary is:

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
changed option values as well as newly added commands. Binding checks reject
unknown and vanished identities; a renamed test needs an explicit registry edit.

```bash
python3 scripts/check-test-map.py --inventory /tmp/empack-tests-all.json --bindings path/to/rust-bindings.json
python3 scripts/check-test-map.py --python-inventory /tmp/empack-python-tests.json --python-bindings path/to/python-bindings.json
python3 scripts/check-test-map.py --cli-inventory /tmp/empack-cli.json --cli-bindings path/to/cli-bindings.json
```

These checks validate structure and classification only. The complete bindings,
requirement/case/variant result join, doctest discovery and final qualification gate
are still being implemented. `scripts/verify.py` already joins selected compiler IDs
to actual JUnit results and retains one receipt per attempt. A successful structural check does not qualify the release.

`test-bindings.json` records the compiled external test crate on macOS arm64,
including fixture checks, controlled hosts, executable journeys, interactive cases
and live probes. Other platform-only identities and the remaining workspace still
need classification. The registry marks UX-047 partial and lists its remaining
evidence. Comparing the complete workspace inventory with this partial file must
fail on unclassified tests. Cases declare required platforms and product feature
variants; those declarations do not establish that any variant ran.
