# Contributing to empack

The [design contracts](docs/design/README.md) define supported behavior. Callable interfaces and compiled examples live in the Rust API documentation. Changes must preserve feature contracts and their negative-case tests.

## Development

`rust-toolchain.toml` selects the compiler, rustfmt and Clippy for local builds and all CI jobs. CI reads this file through the shared toolchain action; it does not install a separate floating stable compiler. Update the pin and test the dependency graph together. Coverage additionally uses `llvm-tools-preview` and cargo-llvm-cov.

Use cargo-nextest, mise and Python 3.11 or later with that toolchain. Strict live tests additionally require Java, provider access and any documented credentials. Never commit credentials or include them in diagnostics.

Native empack embeds the Java bootstrap used by managed exports. Run `mise run build:bootstrap` once before direct Cargo commands; normal mise build tasks include it. The Java project's `bootstrap/mise.toml` pins its maintained build JDK and Maven, while the compiler targets Java 8. Rebuild after changing bootstrap sources or stamping a release version. `python3 scripts/build-bootstrap.py --check` checks an existing artifact without acquiring dependencies. Pack authors using the packaged executable do not need these build tools.

Run `python3 bootstrap/tests/test_bootstrap.py --java /path/to/java` against the packaged JAR. The same JAR is tested on the majors admitted by the [Java policy](docs/design/java.md). These tests require Python, Rust for a small native fixture, OpenSSL and Java keytool; they never use a personal instance. See the [bootstrap contract](docs/design/bootstrap.md) for the boundary and release inputs.

For a Unix test of the complete JAR/native/Java console path, use `bootstrap/tests/test_native_runtime.py --help`. It requires a prepared disposable Prism instance carrying `.empack-bootstrap-fixture`, an explicit engine and Java, and a fresh output directory. Create that marker only in an instance reserved for testing. The smoke checks input, terminal Ctrl-C, cancellation of stopped work, classified statuses and terminal restoration using a synthetic Java console. It does not launch Minecraft or establish Prism GUI acceptance.

```bash
mise run check
mise run clippy
mise run test
mise run smoke
mise run test:live
```

Use a development branch and submit a PR to `main`. Commit coherent, reviewable landings. Record exact commands, outcomes and tested revisions; coverage is supplemental evidence rather than a substitute for contract tests.

Dependency and tool changes follow [Dependency and tool maintenance](docs/dependencies.md), including coupled features, owned patches, generated outputs, and qualification.

## Boundaries

`empack-core` contains pure values and planners with no runtime or I/O dependency. `empack-lib` composes narrow ports, adapters and the operation engine. `empack` owns the host runtime and executable lifecycle. CLI translation and presentation live in the library application adapter. `empack-tests` exercises native executable workflows; library suites verify provider, archive, filesystem and process contracts.

The library's `testing` module is available to its unit tests and callers that explicitly enable `test-utils`. Keep fixture helpers inside that boundary. `empack-tests` opts in; ordinary CLI builds must not enable it. Check the ordinary package separately from the workspace test graph, whose features can be unified.

Do not introduce a broad session facade into the core, success-returning placeholder verifiers, deserializable approval proofs, or a second command-specific publication path. New features must contribute normalized intent, expected effects and verified postconditions. The [feature requirements](docs/design/features.md) describe the useful capabilities the implementation must provide.

## Human-facing output

Lead with the result, then useful changes or problems and the next action when one is needed. Write for a modpack user without knowledge of engine internals: say checking, dependencies, files and settings. Keep technical identifiers when they help locate a version, file or failed operation. Explain which saved work is affected instead of referring to an undifferentiated state.

Default output should answer the command's question. Details explain the decision; structured output retains the complete machine contract. Do not print successful booleans, empty evidence lists, debug enums or a caveat paragraph after every result. Keep incomplete checks, conflicts, partial effects and required recovery visible. An available version is not proof that the whole pack works; explain a relevant limit briefly where it affects the decision. A pinned version is a deliberate choice, not an update failure.

Keep shared facts in their engine/report owner and human rendering in the application adapter. Use `crate::terminal::json_text` for JSON reports sent through the display adapter; it preserves decoded values while escaping terminal controls with valid JSON syntax. Keep canonical and signed document writers separate. Use `application::diagnostics` for completed failures. Error owners supply readable `Display` context; keep stable typed stage/recovery facts and serialization separate from that wording. Anyhow context values are not always visible through standard error-source downcasts; retain the full chain rather than dropping unrecognized frames. Reuse typed causes and shell-safe next actions rather than matching error text. Progress belongs on stderr and must finish before the final report; JSON stdout must remain parseable. Retain Unicode names, escape unsafe terminal controls, and keep plain output usable without color. Cancellation never hides a completed effect or a cleanup failure. Review actual output with representative data, narrow terminals and redirected streams, not only string assertions.

## Source and documentation

Run formatting and Clippy. Document exported contracts with inputs, outcomes and limitations. Prefer comments that explain an invariant over comments that restate code. Use structured, redacted diagnostics and remove temporary debugging output.

Keep normative requirements in `docs/design/` and operational instructions in the user guides linked from `docs/README.md`. Update contracts with behavior changes; keep run-specific evidence in CI and PRs. Keep implementation status in the active PR; do not add historical delivery ledgers. The [verification registry](tests/verification/README.md) owns acceptance requirements and discovery bindings, independently of run results. Design contracts describe the target and must not claim unimplemented commands are available in a released executable.

Write complete, direct sentences. Avoid hype, em dashes and fragment-heavy prose. CLI flags and code retain their literal spelling. The brand reference is `inherent.design/packages/docs/knowledge/process-notes/prose-and-communication-reference.md` in the shared workspace.

## Fixtures and releases

Provider contracts use bounded local HTTP fixtures; opt-in live suites verify current provider behavior. Keep source assertions and environment semantics in fixtures rather than recording credentials or maintaining unused response archives.

Release builds derive their version from the release tag. Git-cliff generates GitHub release notes from commits; release automation does not write historical changelogs back into the source tree.

Contributions use the [Apache 2.0 license](LICENSE).
