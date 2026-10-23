# Verification

The [acceptance contracts](design/verification.md) define the current release target.
Tests verify the checked-out implementation; a contract or test count is not evidence
that an unwired capability works. Record run-specific results in CI and PRs.

The v0.8.4 overhaul tracks the complete requirement denominator and compiled
discovery in the [verification registry](../tests/verification/README.md). Its
unbound requirements remain incomplete; structural validation is not a pass rate.

## Local checks

```sh
mise run check
mise run clippy
mise run test
mise run smoke
```

`mise run test` builds the executable, runs Python script contracts, runs the
workspace Nextest selection excluding E2E, and runs doctests. Also check library
tests with minimal features so workspace feature unification cannot hide test-only
dependencies. Architecture checks keep `empack-core` free of I/O/runtime dependencies.

```sh
cargo nextest run -p empack-lib --lib --no-default-features
```

The separated executable suites have explicit boundaries:

| Test binary | Evidence |
| --- | --- |
| `cli_arguments`, `cli_dependencies`, `cli_init` | Offline parsing, refusal and initialization |
| `cli_build` | Offline artifact creation, metadata and cleanup |
| `journey_authoring` | Literal layered authoring, rebuild and removal |
| `journey_local_exports` | Offline resourcepack export and reference Prism preparation |
| `live_cli_init` | Official loader catalog resolution |
| `live_provider_cli` | Modrinth identity, acquisition and convergence |
| `live_server_cli` | Server acquisition/export and instance preparation; no game startup |

Live binaries contain ignored tests so a default run cannot silently claim their
coverage. Select them explicitly with strict prerequisites:

```sh
cargo nextest run -p empack-tests --test cli_arguments --test cli_dependencies --test cli_init --test cli_build --test journey_authoring --test journey_local_exports
EMPACK_E2E_STRICT=1 cargo nextest run -p empack-tests --test live_cli_init --test live_provider_cli --test live_server_cli --run-ignored ignored-only --test-threads 2
```

The resourcepack scenario shares its assertions between offline client exports and
live server preparation. Missing Java cannot skip the offline client case. Component
fixtures created through codecs are distinct from the literal authoring journey;
neither proves that Minecraft or Prism consumed the resulting pack. Remaining
legacy `e2e_` suites still await separation and shared-runner integration.

The executable harness lives in `empack-tests::harness::process`, outside the
product dependency graph. Local runs may discover a binary and print the selected
path. Qualification requires `EMPACK_QUALIFY=1`, an absolute `EMPACK_E2E_BIN`,
`EMPACK_EXPECT_BINARY_SHA256`, and `EMPACK_EXPECT_VERSION`. Wrong bytes or version
fail before the scenario. This is executable selection, not full package
qualification; target/features and consumer evidence still need the release record.

Children receive a positive environment allowlist and fixture-local home, cache,
state and temporary paths. Tests explicitly add any needed fixture tools or product
settings. Harness controls and ambient credentials are not forwarded. A failed
mutating command is not replayed after matching a timeout diagnostic. Nextest CI
retries are disabled so the original failure remains visible.

After building the executable, compare the editor schemas with the actual CLI:

```sh
uv run scripts/check-authoring-schema.py target/debug/empack
```

This exercises accepted and rejected authoring forms without creating host state or
cache storage. JSON Schema checks complement composed domain validation; they do not
replace it.

Formatting and Clippy apply to all targets/features. Native filesystem/process
claims require real temporary roots and OS primitives; pure planner tests cannot
prove link handling, locks or crash durability.

### Inherited terminal regression

On macOS and Linux, the normal native suite includes a real controlling-PTY test:

```sh
cargo test -p empack-lib --lib inherited_runtime_owns_terminal_and_restores_it -- --nocapture
```

It supervises an isolated runtime process, checks foreground ownership before any
console reads, sends input and terminal Ctrl-C, and verifies normal shutdown,
cancellation, nonzero exit, failed exec, disabled terminal suspend keys, external
stop/resume, cancellation while stopped, and restoration of terminal ownership and
settings. The fixture changes terminal settings to make restoration observable.
It proves host process/terminal behavior; it is not Minecraft gameplay acceptance.
Managed Prism tests separately verify the root icon's seed policy and rejection of
an explicit managed policy before publication, while keeping other PNGs managed.
Engine launch fault injection separately establishes that failed terminal cleanup
after confirmed retirement reports the error without retaining runtime ownership;
uncertain-retirement tests still require recovery. Signed-channel tests use the
native release producer and a persisted Prism layout, reject managed icon updates,
and preserve launcher bytes through corrected seed updates and launch preparation.
Historical managed-icon fixtures verify migration to seed without conflict overrides.

Executable finalization is tested separately from process supervision:

```sh
cargo test -p empack --bin empack
cargo test -p empack-lib --lib launch_failure_summary
```

These checks retain cleanup, recovery, uncertainty and partial-effect diagnostics
when cancellation is set, retain success after a late interrupt, and verify that
launch failure headlines do not imply that the runtime never ran. The executable
fixtures test result presentation; the engine and PTY fixtures establish ownership
cleanup and actual process retirement.

The `managed_runtime_exit_uses_empack_classification` host test runs a fixture child
with exit statuses 0 and 7, then verifies managed CLI classification and the retained
runtime-status diagnostic. It does not launch Java. The Unix
`e2e_interrupt_exits_130` test sends SIGINT to the actual empack executable during
a bounded local HTTP request and checks both exit 130 and stderr. Run it against a
freshly built binary to test executable wiring:

```sh
cargo build -p empack --all-features
cargo test -p empack-tests --all-features --test e2e_exit_codes e2e_interrupt_exits_130
```

## Strict external checks

```sh
mise run e2e:strict
mise run smoke:runtime
mise run smoke:providers
```

Strict execution fails when prerequisites are missing. Runtime checks need public
Mojang, loader and Maven access plus Java 21 through `JAVA_HOME` or `PATH`. Historical
Forge cases use `EMPACK_TEST_JAVA8_HOME`. Provider checks use declared credentials;
`EMPACK_KEY_CURSEFORGE` supplies the explicit live CurseForge suite. Never record
credentials, signed locators or downloaded third-party bytes in committed reports.

The runtime task builds the CLI before running. Managed-server probes use
`EMPACK_E2E_BIN` when supplied, otherwise the workspace debug executable. They
extract a reference distribution, run its generated installer and launcher, require
a completed native instance record, and reach Minecraft help through local Java.
Host state and caches remain in temporary directories.

The runtime matrix covers vanilla, Fabric launcher layouts, Quilt, historical and
modern Forge, and early/current NeoForge families. Validate official installer
profiles, expected libraries and launch arguments. Help output or EULA refusal
proves launcher execution, not gameplay; tests never accept the EULA.

## Consumer and release evidence

Inspect exported manifests and archives independently, then test the actual supported
consumer. Prism ZIP imports, Modrinth and CurseForge format acceptance, server startup
and native instance updates are separate gates. Do not assume a TAR/7z tree is
importable wherever a ZIP is accepted.

Use a separate Prism profile through `--dir` and import each generated archive
through `--import`. Authenticate through the launcher when needed; never copy
account files into test fixtures or publish launcher logs containing credentials.
Check the imported runtime components and selected file bytes. Prism may use
`minecraft/` for platform imports and `.minecraft/` for native instance archives.

For a referenced Prism export, launch the imported instance and verify both native
preparation and the `Prism → empack → Java` process chain. Change the test release
descriptor without updating its expected hash, then launch again: empack must fail
preparation, Prism must propagate the nonzero result, and Java must not start.
Restore the descriptor afterward. Modrinth and CurseForge snapshot imports must
launch without empack hooks. Record the consumer version and distinguish successful
import, game startup and gameplay; these are different assertions.

Native release acceptance includes A-to-B updates, choices, user-edited configuration,
manual content, offline policy, trust failures, runtime changes and rollback. Crash
tests terminate a real subprocess at durable boundaries and recover in a new process.
Use a played-world sentinel to prove update/rollback confinement.

Curated large-pack workflows run through the production CLI and resource profile.
Inspect inventory and selected bytes, not only artifact size or exit status. Compare
repeated sync and rebuild behavior with unchanged version labels.

## Coverage and release qualification

Measure production-source coverage separately from inline test bodies where tooling
permits. Coverage supplements behavioral evidence and must use the candidate revision.
Do not combine unrelated historical runs into a same-head acceptance claim. Native
platform, provider, runtime and consumer results identify their tested revision,
tool versions and explicit exclusions. Release publication follows successful
acceptance of one immutable candidate and dependency graph.

Generated project automation is covered by `scripts/tests/test_project_automation.py`
and the executable `smoke_generated_workflow` contract. The latter initializes a pack
with spaces in its name, builds Modrinth and Prism profiles through the generated
helper, rejects a mismatched tag and excludes stale output. Python tests verify
download digests, staging names, checksum contents and draft publication ordering.
Windows CI executes generated batch scripts with native `cmd.exe`; Java fixtures
check argument selection and exit status without claiming Minecraft gameplay.

Server archive tests check both console and GUI launchers in ZIP, TAR and 7z outputs.
Native shell and Windows command fixtures verify the default `nogui` argument, its
absence from GUI launchers, argument boundaries, preparation ordering and exit codes.
Live runtime probes exercise console launchers; they do not certify desktop GUI rendering.

## Managed consumer interpretation

Managed Prism configuration tests include absent, unsupported, duplicated and
misplaced version markers, command shadowing, multiline quoting, and hash-prefixed
settings with open quotes or trailing continuations under both supported versions.
Include ordinary space/tab controls and Unicode whitespace at key, section, comment
and value boundaries. Accept LF/CRLF and unterminated final lines; reject bare CR and
unsupported controls. Unicode display-value fixtures remain accepted without changing
author bytes. Semicolon controls preserve the effective lifecycle settings. Compare the
same fixtures with Prism 11.1.1's actual `INIFile::loadFile` implementation, using
a clean upstream checkout and Qt 6.4 or newer within major version 6, as required
by that Prism revision. Qt is opt-in test tooling, not an empack runtime dependency.
The probe rejects unsupported Qt versions before compilation. Earlier Qt 5 runs are
supplemental diagnostics, not qualification of Prism 11.1.1. Consumer-parser execution
is separate from launching Minecraft. Its receipt records application and empack
revisions, dirty state, fixture SHA-256, platform/architecture, compile/runtime Qt
versions, compiler identity and build configuration. The probe compiles upstream `INIFile.cpp` and `Json.cpp`
without vendoring or changing their parsing code:

```sh
python3 scripts/check-prism-config.py --prism-source /path/to/PrismLauncher-11.1.1 --qmake /path/to/qmake
```

Use a clean checkout at `7d2d3c1ec6fcf8255ebacd02d9f792688575dfb4` and supported Qt 6
development tools plus `make`. The shared fixtures also run through empack's public build
preparation. The probe verifies accepted settings and reproduces the legacy
shadow and hash-prefixed quote/continuation counterexamples; it does not certify
every rejected representation.

Managed server tests check archive attributes in ZIP, TAR.GZ and 7z for identical
scripts captured with and without executable source permissions. On Unix, extract
and directly execute the shell entry points with controlled fake external tools.
Windows tests execute batch entry points with `EMPACK_NO_PAUSE=1`, checking full
diagnostic text, empty syntax-error output and preserved exit status. Source
permissions and authored bytes remain unchanged after candidate preparation.

## Documentation checks

The repository-relative validator checks inline local Markdown links and anchors,
fenced blocks and selected prose rules. It does not check external URLs or claim to
parse every Markdown extension. File checks work without Git metadata:

```sh
python3 scripts/check-docs.py docs/testing.md docs/design/builds.md
```

Git-dependent checks are explicit. After fetching the relevant branches, validate a
stacked contract snapshot without switching the working tree:

```sh
python3 scripts/check-docs.py --ref origin/dev-v0.9-verified-export --base origin/dev-v0.9-browser-store --docs-only --strict-prose docs/development/v0.9.0-beta
```

This checks parent ancestry, changed-file scope and diff whitespace in addition to
the selected snapshot's documents. Ref names are inputs; no developer-local branch
list, fixed main revision or `.git/` helper is required. Documentation checks do not
run the native or browser/device acceptance matrix.
