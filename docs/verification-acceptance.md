# Consumer and human acceptance records

The candidate gate accepts operator-recorded tasks separately from native suites
and the headless Prism parser probe. Reading a record does not run the task or
independently establish that its description is true. The maintainer reviews the
operator's observations and retained files before selecting the record for a gate.

The internal checklist is
[acceptance-procedures.json](../tests/verification/acceptance-procedures.json).
It covers reference and bundled Prism imports, mrpack and CurseForge imports,
managed and standalone server consoles, authorized startup, pending-work recovery,
documentation tasks and sandbox publication. These are verification procedures,
not additional product commands or a general workflow language.

Consumer records are required on Linux, macOS and Windows for default and telemetry
products. The initial selected host for human tasks is macOS. This does not turn a
human task on one host into evidence for another. Actual-consumer and human results
remain unperformed until their records exist; helper tests cannot supply them.
The current consumer group remains partially decomposed rather than claiming that
these initial checklists settle every consumer/profile combination.

## Record identity

Store each record and its supporting files in a separate evidence directory outside
the tracked source. Use schema `1` and kind `operator-acceptance`.

| Field | Required value |
| --- | --- |
| `source_revision`, `version` | The exact candidate source and version. |
| `archive_evidence_sha256` | Digest of the archive gate's `archive-evidence.json`. |
| `target`, `platform`, `features`, `artifact_sha256` | An admitted native target, its OS family, `default` or `telemetry`, and the executable actually used. |
| `procedure`, `procedures_sha256` | An exact checklist ID and the digest of the checklist file at that source. |
| `started_at`, `completed_at` | Ordered ISO dates with explicit timezones. |
| `operator` | Nonempty `id` and the checklist's permitted `role`: `maintainer` or `fresh-user`. |
| `environment` | Actual `os_version`, `architecture`, `tool`, `tool_version` and `runtime`. |
| `subject` | The pack's `project_revision`, exported `distribution_sha256` and independently captured `expected_inventory_sha256`. Passing tasks require both digests. |
| `observations` | Every checklist step, in order, with `step`, `status`, `observed` and `evidence`. |
| `files` | Supporting files, each with a relative `path`, `sha256` and integer `bytes`. |
| `status` | The aggregate observed status, preserving failure, blocked or unperformed steps. |

The product identity comes from the already checked archive or auxiliary telemetry
build. A record cannot choose an unrelated executable. The pack distribution and
expected inventory identify what the operator inspected; they are reported subject
identities, not an automated re-verification of a potentially large pack archive.
Keep the independent inventory and relevant consumer observations in the evidence.

Use the actual supported consumer and dependency configuration. Record its versions
and relevant settings rather than calling a fixture or parser probe a launcher run.
For licensed startup, use an authorized account and a disposable profile. Record
the reached checkpoint without storing account credentials or claiming gameplay
certification. Sandbox publication must identify its disposable repository and
scoped permissions; it must not use a production publishing target.

## Observations and supporting files

Each step uses `PASS`, `FAIL`, `BLOCKED` or `NOT_RUN`. Its `observed` text states what
happened, including limitations. Passing and failing steps must reference at least
one retained supporting file. Unperformed steps may have no files, but must remain
`NOT_RUN`; writing down an expected result does not establish it.

Supporting paths are relative to the record directory. Absolute paths, traversal,
noncanonical spellings, symbolic links and duplicate files are rejected. The reader
checks each file's current length and digest. Records are limited to 1 MiB, with at
most eight supporting files of at most 16 MiB each. Redact secrets before capturing
and hashing logs; do not attach authentication tokens, private keys or account data.

All attempts remain evidence. A later passing record does not erase a failed one.
Missing hosts, feature graphs, steps or actual task records stay unqualified.

## Candidate admission

Add each reviewed record to the existing candidate-gate command:

```sh
python3 scripts/qualify-release.py gate \
  --payload .git/candidate-payload \
  --archive-evidence-sha256 "$ARCHIVE_EVIDENCE_SHA256" \
  --revision "$RELEASE_REVISION" --version "${RELEASE_VERSION#v}" \
  --acceptance .git/acceptance/prism-reference/acceptance.json \
  --output .git/candidate.json
```

Retain the other required suite, parser, tooling, rustdoc and candidate-execution
arguments when invoking the complete gate. Repeat `--acceptance` for each record.
This command admits existing observations; it does not launch a consumer. Missing
or mismatched supporting files fail admission, and an incomplete candidate remains
blocked even when one operator record passes.

The JSON and Markdown output identify the operator-reported scope. The hosted
release workflow does not yet collect these operator bundles automatically; that
collection and the actual tasks remain release work. No consumer or human pass is
provided by the repository's synthetic admission fixtures.
