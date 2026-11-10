# Consumer and human acceptance records

The candidate gate accepts operator-recorded tasks separately from native suites and the headless Prism parser probe. Reading a record does not run the task or independently establish that its description is true. The maintainer reviews the operator's observations and retained files before selecting the record for a gate.

For v0.8.4-beta, the maintainer has deferred the named operator-only cases in the [release scope registry](../tests/verification/requirements.json). They remain unrun unless actual records establish otherwise, but they do not block this beta. The procedures below remain available for voluntary and later qualification; this scope decision does not grant a pass or alter v0.9 requirements.

The internal checklist is [acceptance-procedures.json](../tests/verification/acceptance-procedures.json). It covers reference and bundled Prism imports, mrpack and CurseForge imports, managed and standalone server consoles and GUIs, authorized startup, pending-work recovery, documentation tasks and sandbox publication. These are verification procedures, not additional product commands or a general workflow language.

When required by the selected release scope, Prism, platform-import and server consumer records cover Linux, macOS, and Windows for default and telemetry products. The registered-browser handoff procedure is Windows-specific. The selected host for human tasks is macOS. A result on one host does not supply evidence for another. Actual-consumer and human tasks remain unperformed until their records exist; helper tests cannot supply them.

The qualification profile selects the following existing workflows. Complete case definitions identify the required observations; they do not certify arbitrary custom configurations, every possible recipe or gameplay.

| Procedure                                              | Selected workflow                                                                                                                                        |
| ------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Prism reference                                        | Default reference snapshot import/preparation/startup, then a separately enrolled subscribed fixture for a signed runtime update and relaunch.           |
| Prism bundled                                          | Standalone bundled snapshot; recipient empack is unavailable.                                                                                            |
| Modrinth / CurseForge import                           | Platform snapshot imported through the supported consumer, with independent content/runtime verification and recipient empack unavailable during launch. |
| Managed server console / GUI                           | Generated reference-snapshot entry points, preparation before Java and comparison with the independent inventory. Console and GUI tasks stay separate.   |
| Bundled server console / GUI                           | Standalone bundled snapshot with recipient empack unavailable; direct Java status and retirement are observed.                                           |
| Windows desktop handoff                                | The registered browser opens the permitted page; only verified content can complete the pending operation.                                               |
| Human startup / recovery / documentation / publication | The separately attributed tasks in the checklist, including authorized accounts and a disposable publication repository.                                 |

For platform imports, keep empack unavailable in the consumer's launch environment rather than relying on it being unused. For managed server consoles, compare the installed inventory before testing input and shutdown. Record the actual application, dependency and runtime versions; missing prerequisites remain blocked. These selected consumer tasks complement all required native, executable, package and parser cases.

## Prism runtime update

The reference-instance procedure's signed update must change a supported Minecraft or loader requirement as well as selected content. Capture the old and proposed runtime/profile identities and independent inventories. Retain an unrelated user component, launcher-written icon and played-world sentinel so preservation is observable. A content-only update cannot establish the runtime-transition case.

Use the default reference snapshot for the initial import, preparation and startup checks. For the runtime-update steps, prepare a separate reference fixture with `updates: empack` and complete its explicit local enrollment before initial preparation, following the [publishing procedure](publishing.md). Retain both distribution identities and their independent inventories. This exercises the snapshot default and the deliberately subscribed update workflow.

Keep Prism running with the subscribed fixture's old profile loaded when the signed update becomes available. Launch through its generated wrapper: `instance launch --check-updates` must refuse the runtime change before starting the game or replacing the completed release, game inventory or component profile. Record the refusal and compare those inputs afterward. An authenticated channel observation and its anti-replay floor may advance; that is distinct from activating the new release.

Then stop Prism, apply the same selected update with `instance update`, and restart the launcher so it reloads the component metadata. Check the new runtime/profile and content together, preservation of the unrelated component, launcher-written icon and player data, and actual startup through the selected runtime. Retain both runtime identities, the before/after profiles, inventories and redacted launcher/process observations. If the consumer, runtime or authorized startup is unavailable, record the missing step as blocked rather than substituting a fixture. See [Prism runtime transitions](design/instances.md#prism-runtime-transitions).

## Server GUI tasks

The separate `acceptance::server::managed-gui` and `acceptance::server::bundled-gui` procedures qualify the existing GUI entry points. Run them in an isolated desktop session with a selected Minecraft runtime that supports its server window and a GUI-capable Java installation. Record the actual desktop environment, server/runtime versions and generated distribution. A missing desktop or unsupported runtime remains blocked; console observations and fake tools cannot establish GUI behavior.

Use the extracted candidate's unmodified `start_gui.sh` or `start_gui.bat` as described in [Server distributions](servers.md). For managed delivery, retain preparation-before-Java evidence and use the admitted empack executable. For standalone bundled delivery, keep recipient empack unavailable. Compare the installed inventory independently, confirm the actual server window opens and `nogui` is absent, issue a command through that window, then stop the server through its GUI. Record process retirement, the resulting status and whether the invoking terminal remains usable. Use a disposable world; observe the existing EULA procedure without having empack accept it automatically.

Retain the relevant terminal log and GUI observations, such as a redacted screenshot and the server's response to the command. The GUI records are separate from the console records, which still exercise terminal input, interruption and shutdown. Both procedures require their declared Linux/macOS/Windows and default/telemetry variants. Writing these checklists or admitting synthetic records does not perform them.

## Windows registered-browser handoff

Run `acceptance::desktop::windows-download-handoff` in an isolated Windows desktop using the exact candidate and the ordinary registered browser. Record the browser name/version, desktop and URL association. A fixture executable replacing the opener cannot supply this record.

Start with genuinely pending restricted content and independently retained provider identity, assertions and expected inventory. Use public `pending list` and `pending show` output to identify the operation and its file key. Resume with the documented `--open-downloads` and `--downloads-dir` assistance, following [pending-work usage](usage.md#shared-pending-inputs). Observe the exact permitted provider page in the browser. Before supplying bytes, confirm that the operation remains pending: accepting an open request does not establish a completed download or installation.

Attempt association with wrong bytes and retain the refusal without changing the pending operation, author data or player sentinels. Download the actual required content through the permitted page, rename it, and explicitly associate it with the public file key. Resume the same operation and compare the completed inventory independently. Retain redacted public diagnostics and browser/download observations. A provider restriction, unavailable desktop, failed download or incomplete operation stays blocked or failed; neither a checklist nor a synthetic admission result substitutes for this task. Both declared product variants require their own observations.

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
