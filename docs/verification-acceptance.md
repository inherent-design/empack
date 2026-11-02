# Consumer and human acceptance records

The candidate gate accepts operator-recorded tasks separately from native suites and the headless Prism parser probe. Reading a record does not run the task or independently establish that its description is true. The maintainer reviews the operator's observations and retained files before selecting the record for a gate.

The internal checklist is [acceptance-procedures.json](../tests/verification/acceptance-procedures.json). It covers reference and bundled Prism imports, mrpack and CurseForge imports, managed and standalone server consoles and GUIs, authorized startup, pending-work recovery, documentation tasks and sandbox publication. These are verification procedures, not additional product commands or a general workflow language.

Consumer records are required on Linux, macOS and Windows for default and telemetry products. The initial selected host for human tasks is macOS. This does not turn a human task on one host into evidence for another. Actual-consumer and human results remain unperformed until their records exist; helper tests cannot supply them. The current consumer group remains partially decomposed rather than claiming that these initial checklists settle every consumer/profile combination.

## Server GUI tasks

The separate `acceptance::server::managed-gui` and `acceptance::server::bundled-gui` procedures qualify the existing GUI entry points. Run them in an isolated desktop session with a selected Minecraft runtime that supports its server window and a GUI-capable Java installation. Record the actual desktop environment, server/runtime versions and generated distribution. A missing desktop or unsupported runtime remains blocked; console observations and fake tools cannot establish GUI behavior.

Use the extracted candidate's unmodified `start_gui.sh` or `start_gui.bat` as described in [Server distributions](servers.md). For managed delivery, retain preparation-before-Java evidence and use the admitted empack executable. For standalone bundled delivery, keep recipient empack unavailable. Compare the installed inventory independently, confirm the actual server window opens and `nogui` is absent, issue a command through that window, then stop the server through its GUI. Record process retirement, the resulting status and whether the invoking terminal remains usable. Use a disposable world; observe the existing EULA procedure without having empack accept it automatically.

Retain the relevant terminal log and GUI observations, such as a redacted screenshot and the server's response to the command. The GUI records are separate from the console records, which still exercise terminal input, interruption and shutdown. Both procedures require their declared Linux/macOS/Windows and default/telemetry variants. Writing these checklists or admitting synthetic records does not perform them.

## Record identity

Store each record and its supporting files in a separate evidence directory outside the tracked source. Use schema `1` and kind `operator-acceptance`.

| Field                                               | Required value                                                                                                                                            |
| --------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `source_revision`, `version`                        | The exact candidate source and version.                                                                                                                   |
| `archive_evidence_sha256`                           | Digest of the archive gate's `archive-evidence.json`.                                                                                                     |
| `target`, `platform`, `features`, `artifact_sha256` | An admitted native target, its OS family, `default` or `telemetry`, and the executable actually used.                                                     |
| `procedure`, `procedures_sha256`                    | An exact checklist ID and the digest of the checklist file at that source.                                                                                |
| `started_at`, `completed_at`                        | Ordered ISO dates with explicit timezones.                                                                                                                |
| `operator`                                          | Nonempty `id` and the checklist's permitted `role`: `maintainer` or `fresh-user`.                                                                         |
| `environment`                                       | Actual `os_version`, `architecture`, `tool`, `tool_version` and `runtime`.                                                                                |
| `subject`                                           | The pack's `project_revision`, exported `distribution_sha256` and independently captured `expected_inventory_sha256`. Passing tasks require both digests. |
| `observations`                                      | Every checklist step, in order, with `step`, `status`, `observed` and `evidence`.                                                                         |
| `files`                                             | Supporting files, each with a relative `path`, `sha256` and integer `bytes`.                                                                              |
| `status`                                            | The aggregate observed status, preserving failure, blocked or unperformed steps.                                                                          |

The product identity comes from the already checked archive or auxiliary telemetry build. A record cannot choose an unrelated executable. The pack distribution and expected inventory identify what the operator inspected; they are reported subject identities, not an automated re-verification of a potentially large pack archive. Keep the independent inventory and relevant consumer observations in the evidence.

Use the actual supported consumer and dependency configuration. Record its versions and relevant settings rather than calling a fixture or parser probe a launcher run. For licensed startup, use an authorized account and a disposable profile. Record the reached checkpoint without storing account credentials or claiming gameplay certification. Sandbox publication must identify its disposable repository and scoped permissions; it must not use a production publishing target.

For pre-release sandbox publication, provision the admitted candidate executable in the disposable workflow instead of invoking the generated installer's published-tag lookup. Keep the generated helper and its build, staging, tag checks and publication steps unchanged. Retain the original workflow and the provisioning diff, and record the runner's executable digest alongside the initializing executable's identity when they use different hosts. This checks the candidate's generated build/publication path; availability of its future release-download endpoint remains unverified.

## Observations and supporting files

Each step uses `PASS`, `FAIL`, `BLOCKED` or `NOT_RUN`. Its `observed` text states what happened, including limitations. Passing and failing steps must reference at least one retained supporting file. Unperformed steps may have no files, but must remain `NOT_RUN`; writing down an expected result does not establish it.

Supporting paths are relative to the record directory. Absolute paths, traversal, noncanonical spellings, symbolic links and duplicate files are rejected. The reader checks each file's current length and digest. Records are limited to 1 MiB, with at most eight supporting files of at most 16 MiB each. Redact secrets before capturing and hashing logs; do not attach authentication tokens, private keys or account data.

All attempts remain evidence. A later passing record does not erase a failed one. Missing hosts, feature graphs, steps or actual task records stay unqualified.

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

Retain the other required suite, parser, tooling, rustdoc and candidate-execution arguments when invoking the complete gate. Repeat `--acceptance` for each record. This command admits existing observations; it does not launch a consumer. Missing or mismatched supporting files fail admission, and an incomplete candidate remains blocked even when one operator record passes.

The JSON and Markdown output identify the operator-reported scope. No consumer or human pass is provided by the repository's synthetic admission fixtures.

## Hosted handoff and gate resumption

Use the candidate archives and auxiliary telemetry products retained by one release workflow run. Download its `checked-native-archives` artifact and obtain `archive_evidence_sha256` from the SHA-256 of its `archive-evidence.json`. The source revision, version, executable identities and distribution observations in the records must describe those exact inputs. A new build or workflow run requires checking those identities again; a successful manual-dispatch rehearsal does not qualify a later tag's rebuilt payload.

After performing and reviewing the selected procedures, stage their existing JSON records and supporting files:

```sh
python3 scripts/acceptance-assets.py stage \
  --revision "$RELEASE_REVISION" --version "${RELEASE_VERSION#v}" \
  --archive-evidence-sha256 "$ARCHIVE_EVIDENCE_SHA256" \
  --record .git/acceptance/prism-reference/acceptance.json \
  --output .git/acceptance-upload-1
```

Repeat `--record` for the other reviewed records and attempts. Staging preserves their bytes and checks supporting files and candidate coordinates; the gate still establishes the product, procedure and outcome bindings. The output directory must be new. Its `staging.json` lists the files to upload. Record asset names contain the archive-evidence digest and the record's own digest; supporting assets use their content digest. Different attempts have different names, and shared supporting bytes need only one asset.

The container is a dedicated draft release named `acceptance-v<version>`, such as `acceptance-v0.8.4-beta`. Create it once, with maintainer authorization, or reuse the existing draft. It contains acceptance data and remains separate from the shipping release. For example, using Bash:

```bash
EVIDENCE_TAG="acceptance-${RELEASE_VERSION}"
gh release create "$EVIDENCE_TAG" --draft --target "$RELEASE_REVISION" \
  --title "Acceptance evidence for $RELEASE_VERSION" \
  --notes "Reviewed operator records and supporting files for candidate qualification."
shopt -s nullglob
assets=(.git/acceptance-upload-1/acceptance-*.json .git/acceptance-upload-1/observation-*)
gh release upload "$EVIDENCE_TAG" "${assets[@]}" --clobber
```

Use the intended repository's checkout or pass `--repo owner/repository` to the GitHub CLI commands. Reuploading staged content under the same digest name replaces identical bytes; preserve differently named earlier attempts. Do not remove a failed record to obtain a passing candidate.

The qualification job reads this draft through `scripts/acceptance-assets.py collect`. It selects every record for the exact archive-evidence digest, checks content addresses and supporting bytes, reconstructs the original relative paths, and forwards each record through `--acceptance` to the existing gate. Collection does not execute a task or grant a product identity. Missing accessible records remain missing obligations; access errors, changed bytes or incomplete assets fail collection and prevent qualification. Retrieved files and `intake.json` are retained with the gate report.

GitHub limits draft-release listings to callers with repository push access. The trusted qualification job therefore has `contents: write` permission, while the collector issues only GET requests. Publication stays in the separate tag-only job; manual workflow dispatch does not publish. See [GitHub's release API permissions](https://docs.github.com/en/rest/releases/releases#list-releases).

When native, parser and candidate-execution phases have succeeded and the gate is blocked only by missing operator tasks, complete those tasks, upload the reviewed records and rerun the failed gate in the same workflow run. Its successful build artifacts are reused. Gate reports have attempt-specific artifact names, and publication consumes the artifact selected by the successful gate output, including when only publication is retried. Earlier blocked reports remain available. Actual failed observations still fail the candidate; collection and a gate retry cannot erase them. Hosted execution of this handoff remains a separate check from the repository's mocked GitHub and admission tests.
