# Verification

The [acceptance contracts](design/verification.md) define the current release target. Tests verify the checked-out implementation; a contract or test count is not evidence that an unwired capability works. Record run-specific results in CI and PRs.

The [verification registry](../tests/verification/README.md) owns case requirements and discovery bindings; retained run and candidate reports own execution results. Structural validation establishes no pass rate.

## Local checks

```sh
mise run check
mise run clippy
mise run test
mise run smoke
```

`mise run test` runs Python script contracts, the fixed deterministic workspace suite (including executable tests), and doctests. `scripts/verify.py` is the shared local/CI runner. Rust suites build the ordinary product package in a separate target directory before compiling the workspace test graph. For these runner-built products, Cargo's emitted compilation units establish the enabled features: `empack-tests`, `empack-lib/test-utils`, test targets, and mismatched telemetry features fail before execution. Metadata only associates package identities with workspace names; workspace feature resolution is not the product-build oracle. The retained compiler records and audit are rechecked during receipt admission, including cached compilation units. An externally supplied binary's declared features are not independently established by its digest or version check. The `python` suite executes script contracts independently, without selecting an empack binary. The `doctest` suite runs the pinned rustdoc in both workspace feature graphs and records whether each example ran, only compiled, or established an expected compilation failure. Native CI requires these records on each host; a listing or a successful build does not replace them. Use Nextest 0.9.148 for the executable and library selections. Python evidence also spans the three native operating systems. The PR gate reuses the Linux tooling job and requires separate macOS and Windows receipts. Candidate qualification collects those two native records alongside its Linux run. Admission checks the source, observed outcomes and compiler host; relabelling Linux output cannot satisfy another host. The interpreter architecture remains a separate fact, so a translated interpreter can supervise a native compiler without falsifying it. Local diagnostic selections must identify excluded boundaries and cannot replace the full native suites executed on isolated CI hosts. Also check library tests with minimal features so workspace feature unification cannot hide test-only dependencies. Architecture checks keep `empack-core` free of I/O/runtime dependencies.

```sh
cargo nextest run -p empack-lib --lib --no-default-features
```

The execution variants cover different implementation and distribution risks:

| Variant                                                                              | Why it is separate                                                                                                                                                                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Linux, macOS and Windows native suites                                               | Filesystem identity, locking, permissions, process ownership and console behavior use different platform implementations. APFS remount cases are macOS-only; Windows console/job cases cannot be established by Unix PTYs.                  |
| Default product with minimal test features; telemetry product with all test features | Both product configurations need their own execution. The separate test graphs also expose dependencies or fixture authority hidden by workspace feature unification.                                                                       |
| Five shipped archives                                                                | Linux x86_64/ARM64, macOS x86_64/ARM64 and Windows x86_64 each have their own executable image, build provenance and native loading behavior. Another architecture's successful execution or a cross-build cannot qualify an archive.       |
| Live routes and actual consumers                                                     | Fixture servers, headless parsers and controlled runtimes isolate faults but do not establish upstream access, launcher interpretation during import/startup, or a real Java console. Each required route or task retains its own evidence. |

The [registry](../tests/verification/requirements.json) declares the applicable variants for each case. These distinctions explain the repetitions; they do not reduce the required matrix.

The separated executable suites have explicit boundaries:

| Test binary                                     | Evidence                                                                                                               |
| ----------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `cli_arguments`, `cli_dependencies`, `cli_init` | Offline parsing, refusal and initialization                                                                            |
| `cli_build`                                     | Offline artifact inventory, profile selection, native snapshot installation and cleanup                                |
| `journey_import`                                | Pinned Modrinth/CurseForge archives, complete exported inventory and traversal refusal                                 |
| `journey_authoring`                             | Literal layered authoring, local JAR update/adoption, rebuild and removal                                              |
| `journey_pending`                               | Public build/sync inspection, verified manual association, retained partial inputs, restart, stale refusal and discard |
| `journey_pending_import`                        | Public import selectors, retained-archive restart, exact exported bytes and changed-destination refusal                |
| `journey_pending_instance`                      | Public instance keys, partial retention, executable restart, stale installation and exact discard                      |
| `journey_publisher`                             | Local signed-channel enrollment, update conflicts, repair, rollback and trust changes                                  |
| `cli_offline`                                   | Focused offline CLI contracts, with connected existing-source workflows in its `journeys` module                       |
| `journey_local_exports`                         | Offline resourcepack export and reference Prism preparation                                                            |
| `live_cli_init`                                 | Official loader catalog resolution                                                                                     |
| `live_provider_cli`                             | Provider identity, pinned direct-URL acquisition and convergence                                                       |
| `live_server_cli`                               | Server acquisition/export and instance preparation; no game startup                                                    |

The library integration targets use the same boundary names: `live_provider_contract`, `live_runtime_catalog_contract`, `live_server_distribution_startup`, `external_archive_contract` and `live_import_content_contract`. Renaming them does not run their ignored probes or change their prerequisites. Local fixed-target tasks are checked against classified package/target pairs; compiler discovery still establishes actual membership.

Live binaries contain ignored tests so a default run cannot silently claim their coverage. Select them explicitly with strict prerequisites:

```sh
cargo nextest run -p empack-tests --test cli_arguments --test cli_dependencies --test cli_init --test cli_build --test journey_authoring --test journey_local_exports
EMPACK_E2E_STRICT=1 cargo nextest run -p empack-tests --test live_cli_init --test live_provider_cli --test live_server_cli --run-ignored ignored-only --test-threads 2
```

Per-workflow assertions, test identities, and required variants live in the [verification registry](../tests/verification/requirements.json). That registry covers authoring, imports and pending work, provider selection, builds, publisher/instance composition, recovery, and runtime ownership. Use its case definitions and the selected run report to assess coverage instead of treating this guide as a second case inventory or progress ledger.

`python3 scripts/verify.py live-provider-cli` selects the Modrinth and direct-URL executable cases. `python3 scripts/verify.py live-curseforge` also includes the CurseForge executable journey and requires explicit `EMPACK_KEY_CURSEFORGE` before discovery/execution. An absent credential is blocked evidence, not an empty passing selection. These tests use current compatible provider selections; the independently hashed real-pack corpus below retains a separate immutable-input contract.

The opt-in `publisher-cli` suite uses the selected product against the immutable HTTPS fixture described in the [hosted-publication corpus](../tests/fixtures/hosted-publication/README.md). It verifies host-to-CLI publication behavior using original synthetic data and public fixture keys. It is ignored in offline suites and selected explicitly by the shared runner; its declared host/product variants need their own observations.

```sh
python3 scripts/verify.py publisher-cli --product-profile release
```

The opt-in `native-remount` suite creates disposable APFS images on macOS and requires `hdiutil`, `diskutil`, and `plutil`. It exercises persistent identity, continuations, publication recovery, and publisher/instance workflows across device reassignment. Image browsing and auto-open are disabled. Cleanup rechecks ownership before detach, uses no forced detach, and preserves diagnostic state when retirement cannot be confirmed. Run these cases serially on an isolated host that permits the image operations; a metadata simulation is not an actual remount.

```sh
python3 scripts/verify.py native-remount --product-features default --test-features minimal
```

The [pinned import corpus](../tests/fixtures/import-corpus/README.md) owns fixed archive hashes and reviewed expectations. Run `python3 scripts/check-import-corpus.py` or `mise run test:import:corpus` for repository-owned inputs; they also run in the ordinary CLI and deterministic suites. The separate `python3 scripts/verify.py pinned-import` selection acquires the pinned real-pack input. Neither synthetic fixtures nor a successful import establish launcher/game acceptance.

Scheduled compatibility runs on the default branch each Monday. The shared runner selects the synthetic and real-pack pinned corpora, official provider/catalog probes, and current runtime checks. During the first seven UTC dates of each month it substitutes the broader runtime suite, with both Java 8 and 21 installed. Missing credentials or prerequisites remain blocked evidence. `python3 scripts/check-live-compatibility.py --plan-only` writes the dated selection without claiming execution; omit that flag to run it.

A separate job runs `scripts/survey-imports.py --profile weekly`, selecting one of the curated packs per UTC week without backfilling a failed slot. It admits at most 100 MiB per pack archive and 60 seconds per archive transfer, checks the transfer deadline between bounded socket reads, and retains failed observations. Socket timeouts bound a blocked read; they do not make Python reads instantaneously cancellable. Manual fixture downloads have a 256-file/2-GiB total allowance and a 1-GiB per-file limit. Each empack command has a 600-second deadline. The probe and survey jobs have 90- and 60-minute budgets respectively; no whole-command retry masks a failed mutation.

Survey archive caches bind the exact provider/project/file/URL selection and retain an observed digest. Reuse verifies both identity and bytes and is labeled `retained-observation`, not fresh provider execution. A new upstream selection cannot reuse the previous version's bytes. The jobs upload reports even on failure and have read-only repository access; manual dispatch on another branch cannot obtain their credentials. This workflow is scheduled evidence, not a substitute for qualification of the exact release candidate. Broader pinned real-pack coverage remains outstanding.

The executable harness lives in `empack-tests::harness::process`, outside the product dependency graph. Local runs may discover a binary and print the selected path. Qualification requires `EMPACK_QUALIFY=1`, an absolute `EMPACK_E2E_BIN`, `EMPACK_EXPECT_BINARY_SHA256`, and `EMPACK_EXPECT_VERSION`. Wrong bytes or version fail before the scenario. This is executable selection, not full package qualification; target/features and consumer evidence still need the release record. Resolve and verify the selected product before starting an observer or interaction deadline. Hashing a large debug executable is harness setup, not time spent waiting for a running command to reach its prompt.

Children receive a positive environment allowlist and fixture-local home, cache, state and temporary paths. Tests explicitly add any needed fixture tools or product settings. Harness controls and ambient credentials are not forwarded. A failed mutating command is not replayed after matching a timeout diagnostic. Nextest CI retries are disabled so the original failure remains visible. Live runtime probes retain bounded, best-effort launcher output on timeout, with at most 64 KiB per stream. These tails help diagnose the failure; they do not turn a timeout into a pass or replace complete successful output assertions. Managed runtime probes execute the generated installer first (300-second bound), require completed ownership and selected content, then execute the generated start script (90-second bound). Startup still invokes preparation again, exercising convergence before reaching Minecraft. Installation failure cannot be counted as startup evidence. These help/EULA probes are separate from interactive console and gameplay acceptance.

Cleanup read-access fixtures independently establish Unix permission denial or a Windows sharing violation on a selected artifact or cache object. After restoring only the fixture's access, they compare complete project and host snapshots, including file bytes, attributes and empty directories, and require the same approved cleanup to succeed.

Compare the editor schemas with a separately built ordinary default CLI:

```sh
python3 scripts/verify.py schema --output target/verification-schema
```

The schema producer requires `uv` and installs its pinned JSON Schema dependency. It records every selected authoring/binding fixture, validation result, CLI response, and workspace byte/attribute observation, together with stable source/product identity and the ordinary build audit. CI uploads the receipt and observed results from docs/schema-only runs too. Raw output rechecked against its corpus is evidence; a successful job name alone is not.

Formatting and Clippy apply to all targets/features. Native filesystem/process claims require real temporary roots and OS primitives; pure planner tests cannot prove link handling, locks or crash durability.

### Inherited terminal regression

On macOS and Linux, the normal native suite includes a real controlling-PTY test:

```sh
cargo test -p empack-lib --lib inherited_runtime_owns_terminal_and_restores_it -- --nocapture
```

It supervises an isolated runtime process, checks foreground ownership before any console reads, sends input and terminal Ctrl-C, and verifies normal shutdown, cancellation, nonzero exit, failed exec, disabled terminal suspend keys, external stop/resume, cancellation while stopped, and restoration of terminal ownership and settings. The fixture changes terminal settings to make restoration observable. It proves host process/terminal behavior; it is not Minecraft gameplay acceptance. The Unix executable test `cli_pty_instance_launch_restores_console_after_input_interrupt_and_failure` connects public export/install to the selected product's `instance launch` through a controlling PTY. A fixture runtime reads console input and changes echo settings; normal shutdown, graceful terminal Ctrl-C, nonzero exit and host cancellation must restore the invoking shell's settings and allow a follow-up read. The test checks the final diagnostic/status, retirement of the owned group, preserved player bytes and a subsequent public launch. It uses the compiler-bound native test runtime with an explicit signal handler, not Java or generated server entry points; those and Windows console behavior require separate evidence.

Unix confirmation, text, selection and fuzzy selection share a cancellation-aware byte reader and captured terminal-mode owner. Text and selection fixtures use real PTYs to check Unicode editing, defaults, original option indices, paging, optional Escape, token cancellation without input, and Ctrl-C during partial UTF-8 or escape sequences. They compare restored flags, control characters and speeds and check cursor visibility. The `native_unix_prompt_adapters_retire_input_and_host_interrupts_before_followup` case exercises `LiveInteractiveProvider` through a controlling PTY, interrupts each adapter through both keyboard input and a host signal, then obtains a fresh rejection on the same thread. These native adapter checks complement the public CLI approval journeys; they do not establish Java, launcher or GUI acceptance. Run them through the verification runner on the required native hosts and product feature sets.

The Windows ConPTY case connects public export/install/launch to the executable's Ctrl-C diagnostic, status 130 and confirmed child retirement. Its compiler-bound private runtime registers a [console control handler](https://learn.microsoft.com/en-us/windows/console/setconsolectrlhandler) before announcing readiness and stays alive through Ctrl-C, so cancellation cannot race a completed child. The fixture owner verifies kernel creation time before retaining cleanup authority through a process handle. A follow-up launch must succeed, and game/player bytes and attributes remain unchanged. ConPTY merges stdout/stderr, so this does not prove separate stream routing, actual Java behavior or Unix terminal restoration. The ignored private runtime's ordinary invocation is not acceptance evidence; the product has no corresponding test control.

Managed Prism tests separately verify the root icon's seed policy and rejection of an explicit managed policy before publication, while keeping other PNGs managed. Engine launch fault injection separately establishes that failed terminal cleanup after confirmed retirement reports the error without retaining runtime ownership; uncertain-retirement tests still require recovery. Signed-channel tests use the native release producer and a persisted Prism layout, reject managed icon updates, and preserve launcher bytes through corrected seed updates and launch preparation. Historical managed-icon fixtures verify migration to seed without conflict overrides.

The Windows termination-denial fixture keeps a real owned job alive while checking that failed termination cannot establish retirement. Dropping the owner explicitly retries job termination before closing its kill-on-close handle; the fixture must then reap the child within the existing bound. This cleanup attempt does not change the already reported uncertainty into a retirement proof or clear recovery state.

Executable finalization is tested separately from process supervision:

```sh
cargo test -p empack --bin empack
cargo test -p empack-lib --lib launch_failure_summary
```

These checks retain cleanup, recovery, uncertainty and partial-effect diagnostics when cancellation is set, retain success after a late interrupt, and verify that launch failure headlines do not imply that the runtime never ran. The executable fixtures test result presentation; the engine and PTY fixtures establish ownership cleanup and actual process retirement.

The `managed_runtime_exit_uses_empack_classification` host test runs a fixture child with exit statuses 0 and 7, then verifies managed CLI classification and the retained runtime-status diagnostic. It does not launch Java. The Unix `cli_interrupt_exits_130` test sends SIGINT to the actual empack executable during a bounded local HTTP request and checks both exit 130 and stderr. Run it against a freshly built binary to test executable wiring:

```sh
cargo build -p empack --all-features
cargo test -p empack-tests --all-features --test cli_exit_codes cli_interrupt_exits_130
```

Resource-lifetime checks distinguish cancellation from worker retirement. The runtime's dropped-handle test writes and synchronizes a real four-byte file, then holds its owner behind a channel. Dropping the caller must cancel the worker token without releasing reservations, evicting the active operation or publishing a terminal result. Releasing the owner permits a typed cancelled result and zero remaining reservations. The bounded registry still requires explicit result release before admitting another operation. Readiness, retirement and release use channels and observed phases rather than elapsed sleeps as evidence.

A controlled-clock unit case checks the shared transfer budget without network or filesystem activity. Elapsed time and consumed bytes carry into later requests; narrower limits and failed charges cannot reset that allowance. Exact expiry also rejects zero-byte work. Tokio clock controls are a library development dependency; they are not a product configuration or a substitute for actual I/O retirement.

Captured Unix processes have a separate tiny-limit check: each stream may use its full allowance, while overflow with an already-full progress queue must report the limit failure after the owned group is gone. Existing tests retain closed-pipe deadlines, descendant retirement and disconnected progress observers. These checks have a Windows job counterpart using literal `cmd.exe` scripts: exact per-stream limits, overflow behind a full progress queue and complete output after the observer disconnects. Overflow must return its specific limit error through confirmed retirement, rather than timing out or losing job ownership. These checks bind UX-080 alongside acquisition and API receipt retention; they do not complete its remaining resource dimensions and public-workflow variants. A macOS zombie fixture holds an exited child unreaped, observes the real group-signal failure, then reaps it and confirms absence. A signal error cannot override subsequent proof of retirement. A separate real live group with an injected signal-denial result must still fail retirement and retain the recovery diagnostic. Chunked HTTP fixtures check the specific byte-limit and deadline errors, including socket retirement, rather than accepting any failure. Payload acquisition checks also retain reservations through the last reader, fit two seven-byte inputs into a sixteen-byte scratch allowance, and retire stalled verifier/response ownership when the caller cancels or drops its future. These reuse existing component tests. Reservations are admission estimates, not measured OS memory use or enforced filesystem quotas. Inherited interactive consoles do not have captured-output limits.

The executable's local proxy fixture explicitly selects blocking reads after accepting a connection, then applies its bounded read timeout. It cannot rely on platform-specific inheritance of the listener's nonblocking mode. Proxy-thread panics retain their original cause. Native image cleanup reports the freshly observed owned backing's entities and exact-file open handles when its expected mount is already absent. It never selects the host filesystem in that state or grants new detach authority. A busy-detach failure remains a failure; these diagnostics do not establish its cause or extend cleanup limits.

## Shared suite runner

```sh
python3 scripts/verify.py deterministic
python3 scripts/verify.py cli
python3 scripts/verify.py telemetry
python3 scripts/verify.py live-catalog live-provider-cli live-modrinth server-cli --product-profile release
```

Fixed selections live in `scripts/verification/suites.py`. `mise run test:cli` runs the offline executable suite. `mise run test:live` explicitly selects live CLI routes and the rolling import survey as well. Telemetry checks are ignored by default and fail if an explicitly selected product cannot produce a trace; absence of the feature is not a successful test.

`mise run test:filter NAME` is a local exploratory Nextest substring filter, not a qualification suite. `mise run survey:imports:pr` and `mise run survey:imports:curated` survey the selected latest upstream packs; the pinned corpus remains a separate check under `mise run test:import:corpus`.

`live-provider` includes both providers and requires `EMPACK_KEY_CURSEFORGE`. The separate `live-modrinth` and `live-curseforge` selections name exact discovered provider tests. `runtime-current` selects the managed vanilla and NeoForge launcher checks plus the Fabric classpath check. `live-runtime` retains the full historical matrix and additionally requires `EMPACK_TEST_JAVA8_HOME`. Missing named tests or prerequisites cannot silently reduce one of these selections. CI live probes and `test:live` build an independent release-profile product; normal deterministic and CLI checks retain their development-profile builds. The test driver remains separate from that product. Use `--product-profile release` for comparable local live evidence. The receipt records the selected profile, and the CI gate rejects a development artifact as a substitute for the live release-profile run. This does not change installer/startup deadlines or retry a failed operation.

Each invocation creates a fresh directory under `target/verification/` containing compiler discovery, Nextest JUnit, bounded redacted logs, a source-file digest manifest, and `receipt.json`. The Rust inventory must match the reviewed platform and test-feature membership, including ignore policy, before execution. `--test-features minimal` uses the workspace with `--no-default-features`; `--test-features all` uses `--all-features`. The harness still requests its explicit `test-utils` dependency in the minimal graph. These flags describe the test graph, not the separately built product. Receipts retain both discovery and execution commands; the gate checks their exact selection and graph rather than trusting a feature label. Every selected test must produce a passing outcome. Empty required binaries, missing results, skipped selected tests, retries, failed commands and changed source/binary bytes cannot yield a suite pass. The final CI gate also checks the classification digest and the compiler inventory's actual host target. Missing tools or prerequisites produce `BLOCKED`; the Unix-only desktop fixture on Windows and the APFS remount fixture outside macOS produce explicit `NOT_APPLICABLE` results.

The runner does not retry an attempt. Re-running it creates another record and retains the earlier failure. These are suite records, not final release qualification or proof that every requirement has a bound case. The CI report joins validated Rust results to the concrete cases and required platform/product-feature variants in the registry. Missing variants remain `NOT_RUN`; a partial or pending decomposition cannot qualify its requirement even when its existing cases pass.

Run Python tooling through the same entry point:

```sh
python3 scripts/verify.py python --output target/verification-python
```

This fixed suite uses unittest discovery, an isolated worker environment and a 15-minute deadline. It records each test's lifecycle alongside discovery, source, classification and interpreter identities. Admission replays those events: subtest failures, expected failures, skipped assertions, setup errors and incomplete runs cannot become passing cases. Missing or repeated identities are rejected. PR CI requires this record even for documentation-only changes; a green tooling job alone is insufficient. Python cases use the `independent` feature variant and cannot substitute for empack, packaged artifacts or actual consumers. Raw `unittest` remains useful for focused debugging but does not produce an admissible receipt.

Python bindings may declare a narrower host scope with an explicit reason. The Unix tar-workflow and privileged symlink fixtures apply to Linux/macOS; Windows ZIP and reparse-point behavior remain separate obligations. An excluded test must produce an observed skip and remains `NOT_RUN` in the receipt. It cannot become a passed case. The suite succeeds only when every applicable test passes; unexpected skips, missing work, fixture failures and failures outside the declared host scope still prevent success. Admission replays this policy from the bound classification.

The worker uses the existing Rustup installation and selects the repository's pinned compiler with automatic toolchain installation disabled. Cargo home and application home/state/cache remain isolated; host Cargo credentials, configuration and compiler wrappers are not inherited. The receipt identifies the compiler and Cargo used by the small offline build fixtures. On Windows, compiler workers retain the installed MSVC discovery roots and configured toolset/library paths so Rust can find its linker. Ordinary product invocations do not inherit those compiler-only settings. The shared CI toolchain action initializes an x64 MSVC developer environment. Shared compiler selection prefers its validated native `cl.exe`/`link.exe` directory so Git Bash's unrelated `link.exe` cannot shadow it. Ordinary product and release builds, Nextest discovery/execution, rustdoc and compiled registry discovery use that same selection. Initializing the toolchain alone is insufficient when a later shell prepends Git tools. Selection changes only the command's copied environment; it does not remove host tools or change the global environment. An external product's version probe still uses runtime isolation. Local Windows compiler checks should run from an initialized x64 developer environment. Command logs preserve UTF-8 bytes and line endings without host newline translation. A missing pinned installation blocks the suite before tests run; Cargo's `--offline` alone does not prevent Rustup from downloading a toolchain.

Hashed Python producer sources, the toolchain file, Cargo lockfile and verification JSON use LF checkout attributes on every host. A native Git fixture enables `autocrlf`, checks the hashed Python/rustdoc inputs, and retains controls for ordinary text conversion and exact signed-fixture bytes. Cross-host admission compares exact bytes; it does not normalize received evidence to accept another source snapshot.

Doctests are discovered and executed separately and their receipts are joined to their own acceptance cases. Source-edited or interrupted tooling attempts remain nonqualifying. `report-verification.py --receipt` accepts both native and Python records and preserves earlier failed attempts.

To select an independently built executable, supply all identity fields explicitly:

```sh
python3 scripts/verify.py cli --binary /absolute/path/empack --sha256 EXPECTED_SHA256 --version EXPECTED_VERSION --target TARGET_TRIPLE --product-features default --product-profile release
```

The runner verifies digest and version. For an external executable, target, features and build profile are caller-supplied provenance; final release qualification must bind them to the build record. A locally built product records the runner's ordinary-package build selection. Neither mode establishes actual consumer startup, platform coverage elsewhere, or final archive installation.

## Evidence storage and cleanup

Keep active verification output separate from author projects and player instances. The shared runner creates uniquely named attempts under `target/verification/` by default; explicit `--output` paths may place owned run artifacts elsewhere. `mise run clean` invokes `cargo clean`; it does not manage a verification archive or determine whether retained evidence is still needed.

| Artifact                                   | Retention and cleanup rule                                                                                                                                                                                                                                                            |
| ------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Active attempts and recovery state         | Retain while a producer or its children may still use them. A timeout or old timestamp alone does not establish retirement.                                                                                                                                                           |
| Selected candidate and acceptance evidence | Retain the exact archives/products, receipts, inventories, observations, and supporting files required to revalidate the selected result. Preserve failed or blocked observations that explain that candidate.                                                                        |
| Completed diagnostic runs                  | Keep the canonical evidence needed for an unresolved defect or relevant regression. After recording its disposition, remove redundant copies and regenerable files that are not required by the retained evidence.                                                                    |
| Compiler homes and build caches            | Reclaim only proven disposable run-owned copies after retirement and a reference check. Never remove the installed toolchain or shared user state as though it were a run artifact. The current Python runner reuses installed pinned Rust tools and disables automatic installation. |
| Coverage and experimental outputs          | Retain the selected report, inputs, and meaningful failure evidence; remove stale duplicate outputs after their purpose ends. Coverage cannot replace ordinary-product or final-package evidence.                                                                                     |

Before pruning, inventory owned paths, measure allocated storage, check active users, and identify the files referenced by retained records. Preserve their bytes and hashes, record the deleted categories and measurements, and recheck retained evidence afterward. Receipts, logs, or source snapshots alone may be insufficient when admission also requires compiled-unit records, a product backing, or consumer attachments. Never leave a receipt presented as revalidatable after deleting its required supporting files.

Use the lifecycle above rather than one age cutoff for every artifact. CI upload expiry is configured per workflow and may differ from the selected release-evidence retention requirement; transfer required evidence before it expires. Record local cleanup in the existing task checkpoint with a concise manifest, not another historical backlog. A change to the candidate or evidence inputs still requires the relevant requalification even when old run files remain available.

## Change selection and final gate

The Verification workflow starts for every PR, including documentation and stacked branches, and for pushes to `main` or `dev`. The former branch and post-merge workflows are consolidated into this path. Pushes compare the exact before/after trees; a new branch selects every tracked path. A missing comparison revision or checkout mismatch fails planning. There is no path filter that can suppress the terminal gate. `scripts/ci-verification.py` compares immutable base/head revisions with renames disabled, so moving code into a documentation path cannot hide its removal. Markdown and editor-schema changes run Python, registry structure, Markdown and executable schema checks. Other changes also select the deterministic native matrix, feature checks, ARM64 compilation and one deterministic coverage run. Each event gets fresh evidence for its checked-out revision. No PR-result reuse is implemented; even an equivalent-looking merge is not treated as previously tested. Lockfile, toolchain and verification changes therefore select fresh checks. ARM64 cross-build remains compilation evidence only.

Each native target runs the deterministic suite against the ordinary default product, under the minimal workspace graph, then repeats the deterministic suite plus telemetry contracts under the all-features graph against a separately built telemetry product. The gate requires both product/graph pairs; a telemetry build or an all-features inventory cannot stand in for the default product and minimal test graph.

Provider, runtime and managed-client paths select additional relevant probes. Shared dependency/toolchain or verification changes select representative routes across those boundaries. Changes to the CurseForge adapter or its mixed-provider probe require the credentialed suite as well. This is change-based PR/branch evidence; it does not replace the complete provider/loader matrix required for a candidate.

Live and consumer jobs run only on repository branches or a maintainer's manual dispatch. Forks use GitHub-hosted runners for secretless checks, receive no provider or coverage credentials, and cannot satisfy a required live job by skipping it. The final gate reports the missing trusted execution. A maintainer can review and test the change on a repository branch; the workflow does not use `pull_request_target` to execute fork code with secrets. The selected CurseForge job expects the `EMPACK_KEY_CURSEFORGE` repository or organization secret and records `BLOCKED` if unavailable.

Manual dispatch also runs the rolling import survey and the existing import-smoke driver. Broad surveys are excluded from ordinary PRs and instrumented coverage. Pinned real-pack inputs and scheduled rotations use separate selections; an upstream survey is not a stable regression oracle. Import/export changes select the fixed `pinned-import` suite, while docs-only changes do not acquire packs.

Use `verification-gate` as the required PR status. It requires every selected job to succeed, including the complete native matrix, and checks each uploaded suite receipt against its compiler inventory and JUnit: source revision, target, product features, selected identities, first-attempt results and evidence digests must agree. Missing, cancelled, skipped, blocked or inconsistent evidence fails the gate. Use the plan's actual checkout revision when inspecting CI receipts. A pull-request checkout can be a synthetic merge commit with the PR head as a parent; the API's head SHA alone is not the receipt's source revision. This does not permit substituting an older source tree or treating a later merge as already qualified. For admitted Rust receipts, its `acceptance` section retains each bound case's exact tests, required host/product variants, artifact digests and supporting receipt digests. It rejects substituted evidence boundaries, mismatched revisions and contradictory records claiming the same receipt identity. All 109 requirement groups remain in the report, including undecomposed native work and staged portable work. A passing selected CI suite does not make those groups complete. The report and source registry digests travel together.

The gate writes `gate.json` and a readable `gate.md` together; CI uploads both and adds the Markdown to its job summary. The view lists requirement groups, their binding status, exact execution variants, and source/executable/receipt hashes. Failed evidence admission produces a diagnostic summary without a passing acceptance table. Report generation time is not an execution timestamp. The renderer does not revalidate standalone JSON, apply waivers, or calculate coverage from test counts; the JSON and original job artifacts remain the evidence to inspect.

Doctest cases retain their required mode (`run`, `compile`, or `compile_fail`) and workspace test graph independently of product features. The release candidate gate admits both graphs from each native host using the same observed-rustdoc verifier. Discovery and execution normalize native source-path separators to the registry's repository paths; item names, line numbers, modes and ignore policy remain exact. Duplicate identities are rejected after normalization, including mixed spellings. Compiler-only examples cannot replace executed examples, and failed attempts remain visible even when a later run passes.

The resulting CI record has `qualification: false`: the remaining case decomposition, script, consumer and device evidence and final-package checks are not yet a complete release qualification record. Selecting this required status in repository branch protection is an administrative step; changing workflow YAML does not set it.

To inspect failed runs alongside later attempts, supply each receipt explicitly:

```sh
python3 scripts/report-verification.py --revision FULL_COMMIT_SHA --version 0.8.4-beta --receipt /path/to/first/receipt.json --receipt /path/to/second/receipt.json --output /path/to/new-report
```

This command writes `attempts.json` and `attempts.md` after rechecking the selected receipts against their retained inventories, JUnit and the checkout's classification. The requirement registry must also match the input captured by the run. Changing an assertion or case requires new evidence; an unchanged test name cannot carry an old passing result into the new requirements. Rust, Python and rustdoc admission all enforce this binding. Keep each attempt directory intact. Use the source revision and product version actually recorded by the selected runs; this command does not execute tests or qualify a different candidate. Select all relevant attempts explicitly: it cannot discover omitted history. Identical receipt files are counted once. Distinct attempts retain their dates, suite names, test graph and executable hashes. A failed test followed by a pass stays failed in the combined view, including when two suites execute that test within one receipt. Missing tests remain unexecuted.

A blocked preflight can explain the missing prerequisite without supplying an executable or passing tests. Unreadable, malformed, mismatched or changed evidence is listed as rejected and contributes no acceptance results. Failure of the runner's final source or product check invalidates its earlier test results. Reports preserve failed and blocked outcomes ahead of unstarted suites; they never turn an incomplete run into success. Exit status is `0` for passing selected attempts, `2` for a blocked selection, and `1` otherwise. Even `0` does not establish release qualification. Strict CI and archive gates still require one complete passing attempt per selected configuration. The report does not grant waivers or replace those gates.

## Strict external checks

```sh
mise run test:live
mise run smoke:runtime
mise run smoke:providers
```

Strict execution fails when prerequisites are missing. Runtime checks need public Mojang, loader and Maven access plus Java 21 through `JAVA_HOME` or `PATH`. Historical Forge cases use `EMPACK_TEST_JAVA8_HOME`. Provider checks use declared credentials; `EMPACK_KEY_CURSEFORGE` supplies the explicit live CurseForge suite. Never record credentials, signed locators or downloaded third-party bytes in committed reports.

The runtime task builds the CLI before running. Managed-server probes use `EMPACK_E2E_BIN` when supplied, otherwise the workspace debug executable. They extract a reference distribution, run its generated installer and launcher, require a completed native instance record, and reach Minecraft help through local Java. Host state and caches remain in temporary directories.

The runtime matrix covers vanilla, Fabric launcher layouts, Quilt, historical and modern Forge, and early/current NeoForge families. Validate official installer profiles, expected libraries and launch arguments. Help output or EULA refusal proves launcher execution, not gameplay; tests never accept the EULA.

## Consumer and release evidence

Inspect exported manifests and archives independently, then test the actual supported consumer. Prism ZIP imports, Modrinth and CurseForge format acceptance, server startup and native instance updates are separate gates. Do not assume a TAR/7z tree is importable wherever a ZIP is accepted.

Use a separate Prism profile through `--dir` and import each generated archive through `--import`. Authenticate through the launcher when needed; never copy account files into test fixtures or publish launcher logs containing credentials. Check the imported runtime components and selected file bytes. Prism may use `minecraft/` for platform imports and `.minecraft/` for native instance archives.

For a referenced Prism export, launch the imported instance and verify both native preparation and the `Prism → empack → Java` process chain. Change the test release descriptor without updating its expected hash, then launch again: empack must fail preparation, Prism must propagate the nonzero result, and Java must not start. Restore the descriptor afterward. Modrinth and CurseForge snapshot imports must launch without empack hooks. Record the consumer version and distinguish successful import, game startup and gameplay; these are different assertions.

Native release acceptance includes A-to-B updates, choices, user-edited configuration, manual content, offline policy, trust failures, runtime changes and rollback. Crash tests terminate a real subprocess at durable boundaries and recover in a new process. Use a played-world sentinel to prove update/rollback confinement.

Curated large-pack workflows run through the production CLI and resource profile. Inspect inventory and selected bytes, not only artifact size or exit status. Compare repeated sync and rebuild behavior with unchanged version labels.

## Extracted archive checks

Run the fixed offline CLI suite against an existing distribution archive on its native execution host:

```sh
python3 scripts/check-release-archive.py --archive /absolute/path/empack-0.8.4-beta-aarch64-apple-darwin.tar.gz --sha256 EXPECTED_ARCHIVE_SHA256 --version 0.8.4-beta --target aarch64-apple-darwin
```

The checker captures and hashes the archive before extraction, admits only the expected regular executable, checks Unix executable attributes, and extracts outside the checkout into a path containing spaces. It invokes the shared `cli` suite with the extracted executable's exact digest and version, without discovery or rebuilding the product. Each public command still uses the harness's isolated home, state and cache. Missing native execution support is `BLOCKED`; cross compilation is not a substitute. The test harness itself is compiled from the current test checkout.

`target/verification-packages/` retains one package record and the underlying suite receipt, compiler inventory, JUnit and bounded logs per attempt. Archive and executable digests are rechecked afterward. A changed, failed or unexecuted archive cannot pass. The record distinguishes the test-source revision from the product's caller-supplied identity; it does not establish the product's build provenance by itself. The release workflow joins this evidence to the corresponding build record. It executes each of the five archives on its native target, including ARM64 Linux; cross-compilation alone does not satisfy execution. The gate rechecks source, version, profile, features, archive/executable digests and the underlying compiler inventory and JUnit results. Unix packaging excludes host filesystem metadata, including macOS AppleDouble entries. Only those tested archives enter the publication payload. Publication verifies its checksums and the gate's evidence digest again, without rebuilding.

`workflow_dispatch` accepts the reviewed version and exercises this pipeline without publishing. Tag publication now requires both `archive-gate` and `candidate-gate`. The latter retains all 88 native obligations and makes the 21 staged portable obligations mandatory from v0.9. Incomplete case decomposition, absent required evidence boundaries and missing observations block qualification, even when every archive test passes. Missing platform execution remains a failure, not a cross-build pass. A configured workflow is not proof that its five-host execution has succeeded.

Incomplete native/consumer case bindings and missing observations still block candidate qualification. The workflow defines five-target archive execution, three-host default/telemetry native and live attempts, and the supported Prism parser probe. Those producers do not establish a qualified candidate merely by being configured: their required records and the remaining consumer/manual evidence must be supplied and admitted. No approved low-risk waiver policy is installed, and this gate accepts no waivers. Critical or actual-consumer omissions cannot be excused by a caller-supplied result label.

Release builds now run `scripts/prepare-release.py` in a clean disposable checkout. The requested version must match the reviewed workspace release target. Stamping changes only the workspace version and its local package entries in `Cargo.lock`; external dependencies and their checksums remain unchanged. The input record retains the source commit/tree, original and stamped Cargo-file digests and toolchain digest. Build metadata uses the source commit's date, and both Cargo and cross build with `--locked`. A new dependency or an ambiguous lockfile shape requires a source change, not implicit dependency resolution during publication. `Cross.toml` forwards only the public `GIT_HASH` and `BUILD_DATE` stamping variables into the cross-build container.

The fixed `scripts/release-artifacts.py build` producer compiles only the ordinary product and retains Cargo's JSON compilation units, metadata, tool version and a replayable feature audit in `compile.json` and its evidence files. Compilation bundles also retain `build.stdout` and `metadata.stdout` after credential redaction. Admission replays the narrow, pinned Rustup-banner handling for Cross and compares the result with the recorded compiler JSON; unknown launcher output is refused. Registry package IDs retain their crate/version fragment while URL credentials and query values remain redacted. Native and cross builds use the same verifier. Complete Cargo metadata names every emitted dependency unit; an unknown unit is refused rather than retained anonymously. The audit rejects Tokio's development clock feature in ordinary products as well as the test-support package/features. The internal audit uses `package` for dependency names, including non-workspace crates. Historical records remain bound to their original source and verifier. Cross 0.2.5 runs metadata inside the selected build container, so package identities and output paths use the same namespace as its compiler records; admission never guesses host/container path substitutions. PR cross-build checks retain these records as compilation evidence, with execution still required on the Linux ARM64 target. Reusing a compilation output directory is refused; choose a fresh attempt so an unsuccessful build cannot overwrite or inherit a previous successful record.

Archive recording requires that compilation bundle, matches it to the reviewed source/stamped inputs and exact executable, and copies its evidence without another build. Archive admission recomputes the compiled feature graph, including cached units, before accepting package execution. A payload missing that binding is refused. The native runner, archive producer and telemetry producer share the retained-audit writer and reader. Synthetic admission fixtures and small Cargo workspaces verify the tooling; they are not final-package acceptance.

Build artifacts retain `build.json` and the compilation bundle; execution artifacts retain the package and suite receipts. `scripts/release-artifacts.py gate` validates their complete five-target set and writes `archive-evidence.json` plus `SHA256SUMS` alongside the unchanged archives. The evidence explicitly records that full candidate qualification remains separate. The workflow also uploads each compilation-attempt directory as `product-compilation-TARGET`, including bounded logs when compilation fails before an archive exists. That diagnostic upload uses the attempt path directly, rather than an environment value assigned only after a successful build. A failed or incomplete bundle remains diagnostic evidence and cannot authorize packaging or publication.

Long runner operations emit fixed phase labels and write `<phase>.progress.json` in their attempt directory before entering the operation. Archive capture, native-image inspection and the CLI suite have separate markers; the suite also marks compiler discovery and execution. Records use `entered`, `returned`, `raised` or `interrupted` and measured elapsed time. They contain no command arguments, environment, child output or exception details. Diagnostic storage or console failure preserves the underlying command outcome.

A returned phase can still have a nonzero or timed-out command. An entry without a terminal marker establishes only that the call was entered; it does not establish process retirement or the cause of an outer job cancellation. A hard job stop may also prevent artifact upload. These markers are best-effort diagnostics with `qualification: false`; receipt admission rejects them as execution evidence. Complete source/product, inventory and outcome receipts remain required.

To evaluate an already checked payload without publishing:

```sh
python3 scripts/qualify-release.py gate --payload /path/to/checked-native-archives --consumer /path/to/prism.json --archive-evidence-sha256 EXPECTED_GATE_DIGEST --revision FULL_COMMIT_SHA --version 0.8.4-beta --output /path/to/candidate.json
```

Run from the exact clean candidate checkout. This rechecks the archive payload's identity and evaluates the source registry, then writes JSON and Markdown with explicit outstanding obligations. Supplemental `--receipt` inputs are rechecked against inventory/JUnit, the same source/version and the actual shipped binary for that target. Bare suite receipts cannot admit arbitrary feature builds.

For a required telemetry variant, run the fixed auxiliary producer from the clean candidate checkout:

```sh
python3 scripts/check-auxiliary-build.py --version 0.8.4-beta --output target/verification-auxiliary
```

It stamps a disposable source clone, builds only the ordinary `empack` package with `--features telemetry --release --locked`, and runs the shared CLI and telemetry suites against the retained executable. The build uses an isolated Cargo home, a separate target directory, the pinned native compiler and the candidate's build date. The original source checkout and default archives remain unchanged. Add each emitted `build.json` to candidate evaluation with repeated `--auxiliary` options; keep its executable and `suite/` evidence alongside it. Admission checks the prepared inputs, compiler, fixed command, actual compiled feature graph, executable bytes and independently replayed suite results. The same retained compilation bundle is required; a declared telemetry feature or a matching version string cannot replace it. The upload selection includes `compile.json`, all three compiler-proof files and both captured stdout files; the admission tests stage those exact globs and recheck the downloaded bundle. These are trusted workflow producer records, not third-party signed attestations.

Auxiliary builds can satisfy their explicit feature/host cases, but never package cases or replace any of the five shipped archives. Failures remain failures even if a later attempt passes. The candidate workflow produces telemetry evidence on Linux, macOS and Windows; it does not treat cross-compilation as native execution.

After archive admission, run the fixed native/live producer once on each of the three primary native hosts:

```sh
python3 scripts/check-candidate-execution.py --payload /path/to/checked-native-archives --archive-evidence-sha256 EXPECTED_GATE_DIGEST --auxiliary /path/to/telemetry/build.json --revision FULL_COMMIT_SHA --version 0.8.4-beta --output target/verification-candidate
```

It captures and extracts that host's admitted default archive outside the checkout and reuses the retained telemetry executable. It does not rebuild either product. Telemetry bytes are captured into an owned temporary copy; Unix execution modes are established there because downloaded workflow artifacts need not retain them. The downloaded build proof and executable remain unchanged. Four independent attempts cover native and live selections with default/minimal and telemetry/all-feature test graphs. `candidate-native` excludes the ordinary CLI binaries already exercised by the archive and telemetry producers; ignored APFS remount tests run separately on macOS. Discovery accepts only those declared binary exclusions and rejects unexplained omissions. The live phase selects the full runtime suite, including historical Java 8 loaders, plus the required catalogs, providers, pinned import, hosted publication, server and Unix desktop-assistance routes. CI supplies Zulu 8 and Temurin 21 and records the actual Java observations.

A blocked live prerequisite does not stop the other attempts or erase their results. The final execution status remains blocked or failed as observed. Add each emitted `execution.json` with repeated `--execution` options, retaining its neighboring feature/phase directories. Admission replays every receipt, compiler, source, payload and binary identity; missing, duplicate or altered attempts cannot select a passing winner. These supplemental observations do not confer package or actual-consumer proof. The candidate gate collects all three host records and still reports blocked or failed execution when the corresponding job is unsuccessful.

Repeated `--consumer` inputs admit the fixed Prism parser producer for the same clean source. Admission checks the supported Qt/compiler environment, probe and fixture identities, and every recorded setting. A claimed PASS with broken required commands is rejected; an accurately recorded failure remains failed. This evidence contributes only to the parser case, independently of product features. It cannot qualify actual Prism import/startup or any missing native case. The release workflow runs the same parser action as PR CI and requires its record before candidate evaluation. Repeated `--tooling` inputs admit the fixed Python producer for the same clean source. The candidate workflow runs it before evaluation and retains its discovery, lifecycle, log and receipt files. It contributes only to explicitly bound tooling cases; it does not supply product-feature, archive or consumer proof. The command returns `0` only for full qualification, `2` for missing obligations and `1` for failed or rejected evidence.

Package cases reference ordinary executable test IDs, but require additional proof from the archive gate. Running the same test through ordinary CLI CI does not satisfy its package case. Each package variant identifies an exact target triple; Intel and ARM64 evidence cannot substitute for each other merely because they share an OS. The `UX-085` package case checks local content export and instance preparation on all five targets. Its remaining cases cover header admission, observation capture and installed native inspectors. These complete the group's case definitions; acceptance still requires execution of every applicable variant. Packaged identity, help and usage errors belong to `UX-001`; generated helper execution belongs to `UX-081`. Their package cases also require all five exact target triples.

Before the archive's CLI suite runs, the package producer reads a bounded native header and records architecture plus native linker/load reports. It uses GNU `readelf --dynamic --program-headers --version-info --wide` on Linux, Apple `otool -L -l` on macOS, and MSVC `dumpbin /HEADERS /DEPENDENTS` on Windows. The record retains the executable digest and length, actual inspector path, digest and version, exact arguments, host and timestamp, and bounded report bytes with their digests. A missing inspector blocks this package check; failed inspection or changed inputs fail it. No inspector invocation executes the analyzed binary.

The archive gate independently extracts the checked archive and reads its header again before admitting the observation. It accepts only the five shipping thin, little-endian 64-bit profiles. Package and archive-gate records use schema 2; older records must be regenerated, not migrated. This establishes the recorded architecture and preserves dependency/load metadata; it is not a complete image validator or a claim of compatibility with untested OS versions. Actual execution on each native target remains a separate obligation. The controlled Python fixtures contain inert ABI headers and fabricated reports. The native inspector test reads a copy of the installed native compiler without executing the copy; neither fixture kind supplies evidence for an actual empack distribution.

Header rules follow the [Linux ELF definitions](https://github.com/torvalds/linux/blob/master/include/uapi/linux/elf.h), [Apple Mach-O definitions](https://github.com/apple-oss-distributions/xnu/blob/main/EXTERNAL_HEADERS/mach-o/loader.h) and [Microsoft PE format](https://learn.microsoft.com/en-us/windows/win32/debug/pe-format).

The release job rechecks both the archive-evidence digest and the qualification digest from its prerequisite jobs, including source, version and payload association, before publishing unchanged bytes. Candidate diagnostics stay in a separate workflow artifact; they do not become extra distribution files. A blocked candidate cannot reach publication through a passing archive job.

## Coverage and release qualification

The PR coverage job retains `verification-coverage` diagnostics after failure: runner logs, compiler inventory, phase markers and any generated `lcov.info`. Failed commands also print a tail of their already-redacted log, limited to 40 lines and 8,192 characters. A successful LCOV report does not erase a failed test run. These diagnostic uploads and line-coverage figures do not supply native or final-package qualification.

Measure production-source coverage separately from inline test bodies where tooling permits. Coverage supplements behavioral evidence and must use the candidate revision. Do not combine unrelated historical runs into a same-head acceptance claim. Native platform, provider, runtime and consumer results identify their tested revision, tool versions and explicit exclusions. Release publication follows successful acceptance of one immutable candidate and dependency graph.

Generated project automation is covered by `scripts/tests/test_project_automation.py` and the executable `smoke_generated_workflow` contract. The latter initializes a pack with spaces in its name, builds Modrinth and Prism profiles through the generated helper, rejects a mismatched tag and excludes stale output. Python tests verify download digests, staging names, checksum contents and draft publication ordering. Windows CI executes generated batch scripts with native `cmd.exe`; Java fixtures check argument selection and exit status without claiming Minecraft gameplay.

Server archive tests check both console and GUI launchers in ZIP, TAR and 7z outputs. Native shell and Windows command fixtures verify the default `nogui` argument, its absence from GUI launchers, argument boundaries, preparation ordering and exit codes. Live runtime probes exercise console launchers; they do not certify desktop GUI rendering.

`cli_server` uses a Cargo-built executable in the external test package to observe Java probe and runtime invocations. It checks explicit/PATH program selection, approval, minimum versions, malformed probe output, exact arguments and game cwd. Failed probes and unsuccessful runtime exits must allow a later launch without stale runtime ownership. Player data, `eula=false`, completed installation records and input/tool bytes remain unchanged. The fixture records its digest and is absent from the ordinary product package's dependency graph.

On Unix, a separate public launch checks non-UTF-8 arguments. Linux also uses a non-UTF-8 executable name; the macOS fixture uses a Unicode name because its volume rejects the invalid filename bytes with `EILSEQ` before invocation. The observer records native bytes independently of its readable JSON strings, which are null when UTF-8 cannot represent the value. Program and working-directory observations must resolve to the selected filesystem locations; the executable filename and arguments retain their exact native bytes. This permits the OS to report either `/var` or `/private/var` without normalizing arguments. Empty arguments, Unicode, newlines and shell syntax must arrive literally in the installed game directory. A bare non-UTF-8 program name must refuse PATH lookup with guidance to use an absolute path. Refusal preserves the installation; successful retirement permits another public launch. This checks native argument transport, not Java or packaged output.

A compiler-discovered private driver supplies the actual generated server scripts as component setup. The test executes those files unchanged through the selected product and local fixture, using Bash on Unix and `cmd.exe` on Windows. Console/GUI arguments, `JAVA_HOME`/PATH selection, preparation-before-probe, failure refusal and managed versus direct-runtime exit status have separate assertions. These checks do not start a JVM or a graphical application; official loader help/EULA probes and actual interactive consumer acceptance remain separate obligations. The setup driver's empty ordinary invocation is not counted as acceptance evidence.

## Managed consumer interpretation

Managed Prism configuration tests include absent, unsupported, duplicated and misplaced version markers, command shadowing, multiline quoting, and hash-prefixed settings with open quotes or trailing continuations under both supported versions. Include ordinary space/tab controls and Unicode whitespace at key, section, comment and value boundaries. Accept LF/CRLF and unterminated final lines; reject bare CR and unsupported controls. Unicode display-value fixtures remain accepted without changing author bytes. Semicolon controls preserve the effective lifecycle settings. Compare the same fixtures with Prism 11.1.1's actual `INIFile::loadFile` implementation, using a clean upstream checkout and Qt 6.4 or newer within major version 6, as required by that Prism revision. Qt is opt-in test tooling, not an empack runtime dependency. The probe rejects unsupported Qt versions before compilation. Earlier Qt 5 runs are supplemental diagnostics, not qualification of Prism 11.1.1. Consumer-parser execution is separate from launching Minecraft. Its receipt records application and empack revisions, dirty state, fixture SHA-256, platform/architecture, compile/runtime Qt versions, compiler identity, build configuration, probe executable digest, observation time and source-input digests. The schema-2 record retains every fixture observation; settings failures return a nonzero status after writing the observations. The gate recomputes the outcome from those settings rather than trusting the status field. Dirty-source results remain diagnostic only. The producer currently runs on Linux and macOS; the required parser case selects Linux, matching the CI runner. The same observations support UX-053's Prism integration and UX-083's headless configuration contract without multiplying execution counts. Actual import and startup remain UX-084 obligations. This does not qualify other consumer operations or hosts. Actual consumer and fresh-user observations use the separate [operator acceptance records](verification-acceptance.md), bound to the candidate products and retained supporting files. Their reader tests do not perform those tasks. The public CLI build case selects ten named fixtures from that same corpus across three managed recipe variants. It validates the generated baseline against literal content before checking exact custom configuration bytes and unchanged remaining members. Rejection preserves author files, attributes and prior archives. A separate standalone bundled control retains author-owned settings without managed hooks. This executable boundary supplements the full engine/parser corpus; it does not run Prism, exercise server templates or qualify a final archive.

The same managed-integration group separately binds server batch preparation and generated-script execution. Changed custom entrypoints are refused for reference snapshots and subscribed recipes; byte-identical copies receive the required output attributes in ZIP, TAR.GZ and 7z without changing source permissions. Standalone bundled scripts retain their separate customization policy. The executable fixture runs unmodified generated scripts through the selected product and fixture Java, checking preparation ordering, refusal and status handling. It does not establish a complete server export or JVM startup. Native results without the supported Prism parser cannot complete this group; actual consumer import/startup remains the separate, mandatory UX-084 gate.

The probe compiles upstream `INIFile.cpp` and `Json.cpp` without vendoring or changing their parsing code:

```sh
python3 scripts/check-prism-config.py --prism-source /path/to/PrismLauncher-11.1.1 --qmake /path/to/qmake > /path/to/prism.json
```

Use a clean checkout at `7d2d3c1ec6fcf8255ebacd02d9f792688575dfb4` and supported Qt 6 development tools plus `make`. The shared fixtures also run through empack's public build preparation. The probe verifies accepted settings and reproduces the legacy shadow and hash-prefixed quote/continuation counterexamples; it does not certify every rejected representation.

Managed server tests check archive attributes in ZIP, TAR.GZ and 7z for identical scripts captured with and without executable source permissions. On Unix, extract and directly execute the shell entry points with controlled fake external tools. Windows tests execute batch entry points with `EMPACK_NO_PAUSE=1`, checking full diagnostic text, empty syntax-error output and preserved exit status. Source permissions and authored bytes remain unchanged after candidate preparation.

## Documentation checks

The repository-relative validator checks inline local Markdown links and anchors, fenced blocks and selected prose rules. It does not check external URLs or claim to parse every Markdown extension. File checks work without Git metadata:

```sh
python3 scripts/check-docs.py docs/testing.md docs/design/builds.md
```

Git-dependent checks are explicit. After fetching the relevant branches, validate a stacked contract snapshot without switching the working tree:

```sh
python3 scripts/check-docs.py --ref origin/dev-v0.9-verified-export --base origin/dev-v0.9-browser-store --docs-only --strict-prose docs/development/v0.9.0-beta
```

This checks parent ancestry, changed-file scope and diff whitespace in addition to the selected snapshot's documents. Ref names are inputs; no developer-local branch list, fixed main revision or `.git/` helper is required. Documentation checks do not run the native or browser/device acceptance matrix.
