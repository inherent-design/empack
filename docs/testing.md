# Verification

The [acceptance contracts](design/verification.md) define the current release target. Tests verify the checked-out implementation; a contract or test count is not evidence that an unwired capability works. Record run-specific results in CI and PRs.

The [verification registry](../tests/verification/README.md) owns case requirements and discovery bindings; retained run and candidate reports own execution results. Structural validation establishes no pass rate.

Use [native and consumer regression coverage](testing-native.md) for terminal, process, encoding and consumer-parser boundaries. [Release verification](releasing.md) covers compiled products, archives and final candidate qualification; [consumer acceptance](verification-acceptance.md) owns actual application checks and their evidence handoff.

## Local checks

```sh
mise run check
mise run clippy
mise run test
mise run smoke
```

`mise run test` runs Python script contracts, the fixed deterministic workspace suite (including executable tests), and doctests. `scripts/verify.py` is the shared local/CI runner. Rust suites build the ordinary product package in a separate target directory before compiling the workspace test graph. Product directories are `verification-product/default` and `verification-product/telemetry` beneath `CARGO_TARGET_DIR`, or the checkout’s `target` when unset. This preserves product feature isolation while allowing a shared build cache. For these runner-built products, Cargo's emitted compilation units establish the enabled features: `empack-tests`, `empack-lib/test-utils`, test targets, and mismatched telemetry features fail before execution. Metadata only associates package identities with workspace names; workspace feature resolution is not the product-build oracle. The retained compiler records and audit are rechecked during receipt admission, including cached compilation units. An externally supplied binary's declared features are not independently established by its digest or version check. The `python` suite executes script contracts independently, without selecting an empack binary. The `doctest` suite runs the pinned rustdoc in both workspace feature graphs and records whether each example ran, only compiled, or established an expected compilation failure. Native CI requires these records on each host; a listing or a successful build does not replace them. Use Nextest 0.9.148 for the executable and library selections. Python evidence also spans the three native operating systems. The PR gate reuses the Linux tooling job and requires separate macOS and Windows receipts. Candidate qualification collects those two native records alongside its Linux run. Admission checks the source, observed outcomes and compiler host; relabelling Linux output cannot satisfy another host. The interpreter architecture remains a separate fact, so a translated interpreter can supervise a native compiler without falsifying it. Local diagnostic selections must identify excluded boundaries and cannot replace the full native suites executed on isolated CI hosts. Also check library tests with minimal features so workspace feature unification cannot hide test-only dependencies. Architecture checks keep `empack-core` free of I/O/runtime dependencies.

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

`python3 scripts/verify.py live-provider-cli` selects the Modrinth and direct-URL executable cases. `python3 scripts/verify.py live-curseforge` also includes the CurseForge executable journey. CurseForge probes use the application’s bundled client key by default; a nonempty exported `EMPACK_KEY_CURSEFORGE` overrides it. The isolated verifier forwards that override only to suites that declare it and treats an empty override as absent. It does not load the invocation directory’s `.env` or `.env.local`; export an override explicitly when needed. Authentication, throttling and transport failures remain nonpassing evidence. These tests use current compatible provider selections; the independently hashed real-pack corpus below retains a separate immutable-input contract.

The live provider CLI command helper and terminal fixture, and live manual assistance, explicitly select `--net-timeout 60`. Their outer command or prompt guards remain 120 seconds for provider CLI and 90 seconds for manual assistance. A stalled initial network operation can report its own deadline before the fixture guard; the outer guard still bounds the entire command or prompt wait. This leaves the product default unchanged. Requests that exceed the selected allowance still fail the attempt. The provider CLI helper includes process status and redacted captured streams in its failure message without formatting the credential-bearing command environment.

The opt-in `publisher-cli` suite uses the selected product against the immutable HTTPS fixture described in the [hosted-publication corpus](../tests/fixtures/hosted-publication/README.md). It verifies publication and a separate player’s authenticated install/update using original synthetic data and public fixture keys. Signed channels are supplied through public local-file observation; release envelopes and the initial seed asset are fetched through actual HTTPS. Repeated update and rollback preserve edited player configuration and world data without lowering the channel sequence floor. It is ignored in offline suites and selected explicitly by the shared runner; its declared host/product variants need their own observations.

```sh
python3 scripts/verify.py publisher-cli --product-profile release
```

The opt-in `native-remount` suite creates disposable APFS images on macOS and requires `hdiutil`, `diskutil`, and `plutil`. It exercises persistent identity, continuations, publication recovery, and publisher/instance workflows across device reassignment. Image browsing and auto-open are disabled. Cleanup rechecks ownership before each of at most five detach requests. Only a typed busy result permits another request, with 200, 400, 800 and 1,600 ms between requests; the native command timeout remains 90 seconds. Confirmed retirement after a retry records the backing identity, request count and elapsed time. Cleanup uses no forced detach and preserves diagnostic state when retirement cannot be confirmed. Run these cases serially on an isolated host that permits the image operations; a metadata simulation is not an actual remount.

```sh
python3 scripts/verify.py native-remount --product-features default --test-features minimal
```

The [pinned import corpus](../tests/fixtures/import-corpus/README.md) owns fixed archive hashes and reviewed expectations. Run `python3 scripts/check-import-corpus.py` or `mise run test:import:corpus` for repository-owned inputs; they also run in the ordinary CLI and deterministic suites. The separate `python3 scripts/verify.py pinned-import` selection acquires the pinned real-pack input. Neither synthetic fixtures nor a successful import establish launcher/game acceptance.

Scheduled compatibility runs on the default branch each Monday. The shared runner selects the synthetic and real-pack pinned corpora, official provider/catalog probes, and current runtime checks. During the first seven UTC dates of each month it substitutes the broader runtime suite, with both Java 8 and 21 installed. Missing credentials or prerequisites remain blocked evidence. `python3 scripts/check-live-compatibility.py --plan-only` writes the dated selection without claiming execution; omit that flag to run it.

A separate job runs `scripts/survey-imports.py --profile weekly`, selecting one of the curated packs per UTC week without backfilling a failed slot. It admits at most 100 MiB per pack archive and 60 seconds per archive transfer, checks the transfer deadline between bounded socket reads, and retains failed observations. Socket timeouts bound a blocked read; they do not make Python reads instantaneously cancellable. Manual fixture downloads have a 256-file/2-GiB total allowance and a 1-GiB per-file limit. Each empack command has a 600-second deadline. The probe and survey jobs have 90- and 60-minute budgets respectively; no whole-command retry masks a failed mutation.

Survey archive caches bind the exact provider/project/file/URL selection and retain an observed digest. Reuse verifies both identity and bytes and is labeled `retained-observation`, not fresh provider execution. A new upstream selection cannot reuse the previous version's bytes. The jobs upload reports even on failure and have read-only repository access; manual dispatch on another branch cannot obtain their credentials. This workflow is scheduled evidence, not a substitute for qualification of the exact release candidate. Broader pinned real-pack coverage remains outstanding.

The executable harness lives in `empack-tests::harness::process`, outside the product dependency graph. Local runs may discover a binary and print the selected path. Qualification requires `EMPACK_QUALIFY=1`, an absolute `EMPACK_E2E_BIN`, `EMPACK_EXPECT_BINARY_SHA256`, and `EMPACK_EXPECT_VERSION`. Wrong bytes or version fail before the scenario. This is executable selection, not full package qualification; target/features and consumer evidence still need the release record. Resolve and verify the selected product before starting an observer or interaction deadline. Hashing a large debug executable is harness setup, not time spent waiting for a running command to reach its prompt.

Compiler-built component drivers may retain the development package version while the selected product is a release. Fixtures that prepare bootstrap invocation files for that product, including live managed-server probes, must bind its expected automation version explicitly. Do not infer that version from the driver's Cargo package or development capability version. Managed probes reject fallback to a downloaded or cached engine so a successful older release cannot stand in for the selected candidate. Full CLI export tests separately check the product's own version selection; fixture setup must not relax the bootstrap's exact-version check.

Children receive a positive environment allowlist and fixture-local home, cache, state and temporary paths. Tests explicitly add any needed fixture tools or product settings. Harness controls and ambient credentials are not forwarded. A failed mutating command is not replayed after matching a timeout diagnostic. Nextest CI retries are disabled so the original failure remains visible. Live runtime probes retain bounded, best-effort launcher output on timeout, with at most 64 KiB per stream. These tails help diagnose the failure; they do not turn a timeout into a pass or replace complete successful output assertions. Managed runtime probes execute the generated installer first (300-second bound), require completed ownership and selected content, then execute the generated start script (90-second bound). Startup still invokes preparation again, exercising convergence before reaching Minecraft. Installation failure cannot be counted as startup evidence. These help/EULA probes are separate from interactive console and gameplay acceptance.

Cleanup read-access fixtures independently establish Unix permission denial or a Windows sharing violation on a selected artifact or cache object. After restoring only the fixture's access, they compare complete project and host snapshots, including file bytes, attributes and empty directories, and require the same approved cleanup to succeed.

Compare the editor schemas with a separately built ordinary default CLI:

```sh
python3 scripts/verify.py schema --output target/verification-schema
```

The schema producer requires `uv` and installs its pinned JSON Schema dependency. It records every selected authoring/binding fixture, validation result, CLI response, and workspace byte/attribute observation, together with stable source/product identity and the ordinary build audit. CI uploads the receipt and observed results from docs/schema-only runs too. Raw output rechecked against its corpus is evidence; a successful job name alone is not.

Formatting and Clippy apply to all targets/features. Native filesystem/process claims require real temporary roots and OS primitives; pure planner tests cannot prove link handling, locks or crash durability.

## Shared suite runner

The fixed `server-cli` selection compiles only its integration test target while selecting both `empack-tests` and `empack-lib` packages to preserve the test feature graph. Full deterministic discovery and suites using private library crash drivers retain workspace compilation. [Registry responsibilities](../tests/verification/README.md#fixture-responsibilities) distinguish exhaustive syntax, preparer wiring, actual archives and consumer execution.

```sh
python3 scripts/verify.py deterministic
python3 scripts/verify.py cli
python3 scripts/verify.py telemetry
python3 scripts/verify.py live-catalog live-provider-cli live-modrinth server-cli --product-profile release
```

Fixed selections live in `scripts/verification/suites.py`. `mise run test:cli` runs the offline executable suite. `mise run test:live` explicitly selects live CLI routes and the rolling import survey as well. Telemetry checks are ignored by default and fail if an explicitly selected product cannot produce a trace; absence of the feature is not a successful test.

`mise run test:filter NAME` is a local exploratory Nextest substring filter, not a qualification suite. `mise run survey:imports:pr` and `mise run survey:imports:curated` survey the selected latest upstream packs; the pinned corpus remains a separate check under `mise run test:import:corpus`.

`live-provider` includes both providers and uses the same bundled CurseForge key or explicit override as `live-curseforge`. The separate `live-modrinth` and `live-curseforge` selections name exact discovered provider tests. `runtime-current` selects the managed vanilla and NeoForge launcher checks plus the Fabric classpath check. `live-runtime` retains the full historical matrix and additionally requires `EMPACK_TEST_JAVA8_HOME`. Missing named tests or prerequisites cannot silently reduce one of these selections. CI live probes and `test:live` build an independent release-profile product; normal deterministic and CLI checks retain their development-profile builds. The test driver remains separate from that product. Use `--product-profile release` for comparable local live evidence. The receipt records the selected profile, and the CI gate rejects a development artifact as a substitute for the live release-profile run. This does not change installer/startup deadlines or retry a failed operation.

Each invocation creates a fresh directory under `target/verification/` containing compiler discovery, Nextest JUnit, bounded redacted logs, a source-file digest manifest, and `receipt.json`. The Rust inventory must match the reviewed platform and test-feature membership, including ignore policy, before execution. `--test-features minimal` uses the workspace with `--no-default-features`; `--test-features all` uses `--all-features`. The harness still requests its explicit `test-utils` dependency in the minimal graph. These flags describe the test graph, not the separately built product. Receipts retain both discovery and execution commands; the gate checks their exact selection and graph rather than trusting a feature label. Every selected test must produce a passing outcome. Empty required binaries, missing results, skipped selected tests, retries, failed commands and changed source/binary bytes cannot yield a suite pass. The final CI gate also checks the classification digest and the compiler inventory's actual host target. Missing tools or prerequisites produce `BLOCKED`; the Unix-only desktop fixture on Windows and the APFS remount fixture outside macOS produce explicit `NOT_APPLICABLE` results.

Nextest suite, instrumented coverage and Rustdoc execution receive application home, configuration, state and cache paths under their own worker-home directories. Ambient empack settings and fault controls are removed; only declared suite prerequisites and runner-selected product/inventory controls are supplied. Cargo and Rustup homes remain attached to the compiler coordinator’s existing storage, including their configured paths, so this isolation does not copy caches or toolchains. Automatic toolchain installation is disabled for that execution. Product subprocesses retain their separate environment allowlist and owned paths.

The runner does not retry an attempt. Re-running it creates another record and retains the earlier failure. These are suite records, not final release qualification or proof that every requirement has a bound case. A definition marked `complete` must bind every declared evidence boundary; validation rejects an omitted boundary before joining results. Finish case definitions before freezing a candidate: `complete` describes decomposition, while actual execution remains a separate requirement. The CI report joins validated Rust results to the concrete cases and required platform/product-feature variants in the registry. Missing variants remain `NOT_RUN`; a partial or pending decomposition cannot qualify its requirement even when its existing cases pass.

Run Python tooling through the same entry point:

```sh
python3 scripts/verify.py python --output target/verification-python
```

This fixed suite uses unittest discovery, an isolated worker environment and a 15-minute deadline. It records each test's lifecycle alongside discovery, source, classification and interpreter identities. Admission replays those events: subtest failures, expected failures, skipped assertions, setup errors and incomplete runs cannot become passing cases. Missing or repeated identities are rejected. PR CI requires this record even for documentation-only changes; a green tooling job alone is insufficient. Python cases use the `independent` feature variant and cannot substitute for empack, packaged artifacts or actual consumers. Raw `unittest` remains useful for focused debugging but does not produce an admissible receipt.

Python bindings may declare a narrower host scope with an explicit reason. The Unix tar-workflow and privileged symlink fixtures apply to Linux/macOS; Windows ZIP and reparse-point behavior remain separate obligations. An excluded test must produce an observed skip and remains `NOT_RUN` in the receipt. It cannot become a passed case. The suite succeeds only when every applicable test passes; unexpected skips, missing work, fixture failures and failures outside the declared host scope still prevent success. Admission replays this policy from the bound classification.

The worker uses the existing Rustup installation and selects the repository's pinned compiler with automatic toolchain installation disabled. Cargo home and application home/state/cache remain isolated; host Cargo credentials, configuration and compiler wrappers are not inherited. The receipt identifies the compiler and Cargo used by the small offline build fixtures. On Windows, compiler workers retain the installed MSVC discovery roots and configured toolset/library paths so Rust can find its linker. Ordinary product invocations do not inherit those compiler-only settings. The shared CI toolchain action initializes an x64 MSVC developer environment. Shared compiler selection prefers its validated native `cl.exe`/`link.exe` directory so Git Bash's unrelated `link.exe` cannot shadow it. Ordinary product and release builds, Nextest discovery/execution, rustdoc and compiled registry discovery use that same selection. Initializing the toolchain alone is insufficient when a later shell prepends Git tools. Selection changes only the command's copied environment; it does not remove host tools or change the global environment. An external product's version probe still uses runtime isolation. Local Windows compiler checks should run from an initialized x64 developer environment. Command logs preserve UTF-8 bytes and line endings without host newline translation. A missing pinned installation blocks the suite before tests run; Cargo's `--offline` alone does not prevent Rustup from downloading a toolchain.

Hashed Python producer sources, bootstrap Java/resources/build definitions, the toolchain file, Cargo lockfile and verification JSON use LF checkout attributes on every host. A native Git fixture enables `autocrlf`, checks the hashed Python/rustdoc/bootstrap inputs, and retains controls for ordinary text conversion and exact signed-fixture bytes. Cross-host admission compares exact bytes; it does not normalize received evidence to accept another source snapshot. Disposable auxiliary builds stage the producer's same JAR and identity record into their version-stamped source checkout, then apply normal source/version/digest admission; they do not rebuild Java or relabel its evidence.

The shared bootstrap workflow owns the packaged JVM and host checks before native compilation: the admitted JVM majors run on Linux, with Windows Java 8/25 and macOS Java 25 covering host-specific acquisition, permissions and process behavior. PR and candidate workflows wait for that gate and retain their separate native/consumer checks. They do not repeat the packaged fixture suite after building native products. `python3 bootstrap/tests/probe_encoding.py --java /path/to/java --output encoding.json` records synthetic native/JVM argument handoffs, Unicode executable/JAR paths and selected encoding properties; Windows records system and console code pages separately. The shared workflow retains these observations with the host results. This nonshipping diagnostic does not change the host locale, qualify a Prism session or turn observed argument loss into a passing consumer check; forced `C` variants affect only its child processes.

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

The Verification workflow starts for every PR, including documentation and stacked branches, and for pushes to `main` or `dev`. Pushes compare the exact before/after trees; a new branch selects every tracked path. A missing comparison revision or checkout mismatch fails planning. There is no path filter that can suppress the terminal gate. `scripts/ci-verification.py` compares immutable base/head revisions with renames disabled, so moving code into a documentation path cannot hide its removal. Markdown and editor-schema changes run Python, registry structure, Markdown and executable schema checks. Other changes also select the deterministic native matrix, feature checks, ARM64 compilation and one deterministic coverage run. Each event gets fresh evidence for its checked-out revision. No PR-result reuse is implemented; even an equivalent-looking merge is not treated as previously tested. Lockfile, toolchain and verification changes therefore select fresh checks. ARM64 cross-build remains compilation evidence only.

Each native target runs the deterministic suite against the ordinary default product, under the minimal workspace graph, then repeats the deterministic suite plus telemetry contracts under the all-features graph against a separately built telemetry product. The gate requires both product/graph pairs; a telemetry build or an all-features inventory cannot stand in for the default product and minimal test graph.

Provider, runtime and managed-client paths select additional relevant probes. Shared dependency/toolchain or verification changes select representative routes across those boundaries. Changes to the CurseForge adapter or its mixed-provider probe require the credentialed suite as well. This is change-based PR/branch evidence; it does not replace the complete provider/loader matrix required for a candidate.

Live and consumer jobs run only on repository branches or a maintainer's manual dispatch. Forks use GitHub-hosted runners for secretless checks, receive no repository or organization provider overrides or coverage credentials, and cannot satisfy a required live job by skipping it. The final gate reports the missing trusted execution. A maintainer can review and test the change on a repository branch; the workflow does not use `pull_request_target` to execute fork code with secrets. A nonempty `EMPACK_KEY_CURSEFORGE` repository or organization secret overrides the bundled client key for the selected CurseForge suites, including `pinned-import`; its absence does not skip those tests or establish that the bundled key is accepted by the service.

Manual dispatch also runs the rolling import survey and the existing import-smoke driver. Broad surveys are excluded from ordinary PRs and instrumented coverage. Pinned real-pack inputs and scheduled rotations use separate selections; an upstream survey is not a stable regression oracle. Import/export changes select the fixed `pinned-import` suite, while docs-only changes do not acquire packs.

Use `verification-gate` as the required PR status. It requires every selected job to succeed, including the complete native matrix, and checks each uploaded suite receipt against its compiler inventory and JUnit: source revision, target, product features, selected identities, first-attempt results and evidence digests must agree. Missing, cancelled, skipped, blocked or inconsistent evidence fails the gate. Use the plan's actual checkout revision when inspecting CI receipts. A pull-request checkout can be a synthetic merge commit with the PR head as a parent; the API's head SHA alone is not the receipt's source revision. This does not permit substituting an older source tree or treating a later merge as already qualified. For admitted Rust receipts, its `acceptance` section retains each bound case's exact tests, required host/product variants, artifact digests and supporting receipt digests. It rejects substituted evidence boundaries, mismatched revisions and contradictory records claiming the same receipt identity. All requirement groups remain in the report, including undecomposed native work and staged portable work. A passing selected CI suite does not make those groups complete. The report and source registry digests travel together.

The gate writes `gate.json` and a complete `gate.md` together; CI uploads both. Its job summary uses a compact view of gate status, source identity and every requirement group, with a pointer to those complete artifacts. `--summary PATH` selects this view without replacing or truncating the full reports; long admission errors are shortened only in that summary, with an explicit reference to the full diagnostic. Per-test/attempt details remain in the artifact so the summary fits GitHub’s 1 MiB step-summary limit. The complete report lists requirement groups, their binding status, exact execution variants, and source/executable/receipt hashes. Failed evidence admission produces a diagnostic summary without a passing acceptance table. Report generation time is not an execution timestamp. The renderer does not revalidate standalone JSON, apply waivers, or calculate coverage from test counts; the JSON and original job artifacts remain the evidence to inspect.

Doctest cases retain their required mode (`run`, `compile`, or `compile_fail`) and workspace test graph independently of product features. The release candidate gate admits both graphs from each native host using the same observed-rustdoc verifier. Discovery and execution normalize native source-path separators to the registry's repository paths; item names, line numbers, modes and ignore policy remain exact. Duplicate identities are rejected after normalization, including mixed spellings. Compiler-only examples cannot replace executed examples, and failed attempts remain visible even when a later run passes.

The resulting CI record has `qualification: false`: the remaining case decomposition, script, consumer and device evidence and final-package checks are not yet a complete release qualification record. Selecting this required status in repository branch protection is an administrative step; changing workflow YAML does not set it.

To inspect failed runs alongside later attempts, set `RELEASE_VERSION` to the version recorded by those runs and supply each receipt explicitly:

```sh
python3 scripts/report-verification.py --revision FULL_COMMIT_SHA --version "$RELEASE_VERSION" --receipt /path/to/first/receipt.json --receipt /path/to/second/receipt.json --output /path/to/new-report
```

This command writes `attempts.json` and `attempts.md` after rechecking the selected receipts against their retained inventories, JUnit and the checkout's classification. The requirement registry must also match the input captured by the run. Changing an assertion or case requires new evidence; an unchanged test name cannot carry an old passing result into the new requirements. Rust, Python and rustdoc admission all enforce this binding. Keep each attempt directory intact. Use the source revision and product version actually recorded by the selected runs; this command does not execute tests or qualify a different candidate. Select all relevant attempts explicitly: it cannot discover omitted history. Identical receipt files are counted once. Distinct attempts retain their dates, suite names, test graph and executable hashes. A failed test followed by a pass stays failed in the combined view, including when two suites execute that test within one receipt. Missing tests remain unexecuted.

A blocked preflight can explain the missing prerequisite without supplying an executable or passing tests. Unreadable, malformed, mismatched or changed evidence is listed as rejected and contributes no acceptance results. Failure of the runner's final source or product check invalidates its earlier test results. Reports preserve failed and blocked outcomes ahead of unstarted suites; they never turn an incomplete run into success. Exit status is `0` for passing selected attempts, `2` for a blocked selection, and `1` otherwise. Even `0` does not establish release qualification. Strict CI and archive gates still require one complete passing attempt per selected configuration. The report does not grant waivers or replace those gates.

## Strict external checks

```sh
mise run test:live
mise run smoke:runtime
mise run smoke:providers
```

Strict execution fails when prerequisites are missing. Runtime checks need public Mojang, loader and Maven access plus Java 21 through `JAVA_HOME` or `PATH`. Historical Forge cases use `EMPACK_TEST_JAVA8_HOME`; Java 17 runtime cases use `EMPACK_TEST_JAVA17_HOME`. CurseForge checks use the bundled application key unless a nonempty `EMPACK_KEY_CURSEFORGE` override is exported. Other explicitly declared prerequisites remain required. Never record credentials, signed locators or downloaded third-party bytes in committed reports.

The runtime task builds the CLI before running. Managed-server probes use `EMPACK_E2E_BIN` when supplied, otherwise the workspace debug executable. They extract a reference distribution, run its generated installer and launcher, require a completed native instance record, and reach Minecraft help through local Java. Host state and caches remain in temporary directories.

The runtime matrix covers vanilla, Fabric launcher layouts, Quilt, historical and modern Forge, and early/current NeoForge families. Validate official installer profiles, expected libraries and launch arguments. Help output or EULA refusal proves launcher execution, not gameplay; tests never accept the EULA.

## Documentation checks

The repository-relative validator checks inline local Markdown links and anchors, fenced blocks and selected prose rules. It does not check external URLs or claim to parse every Markdown extension. File checks work without Git metadata:

```sh
python3 scripts/check-docs.py docs/testing.md docs/design/builds.md
```

Git-dependent checks are explicit. For a documentation-only change, set `DOCS_REVISION` and `BASE_REVISION` to the reviewed commit references, fetch them if needed, and validate the snapshot without switching the working tree:

```sh
python3 scripts/check-docs.py --ref "$DOCS_REVISION" --base "$BASE_REVISION" --docs-only --strict-prose docs
```

This checks parent ancestry, changed-file scope and diff whitespace in addition to the selected snapshot's documents. Ref names are inputs; no developer-local branch list, fixed main revision or `.git/` helper is required. Documentation checks do not run the native or browser/device acceptance matrix.
