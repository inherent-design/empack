# Contributing to empack

The [design contracts](docs/design/README.md) define supported behavior. Callable
interfaces and compiled examples live in the Rust API documentation. Changes must
preserve feature contracts and their negative-case tests.

## Development

Use the pinned Rust toolchain, cargo-nextest and mise. Strict live tests additionally
require Java, provider access and any documented credentials. The packwiz executable
is not required. Never commit
credentials or include them in diagnostics.

```bash
mise run check
mise run clippy
mise run test
mise run smoke
mise run e2e:strict
```

Use a development branch and submit a PR to `main`. Commit coherent, reviewable
landings. Record exact commands, outcomes and tested revisions; coverage is
supplemental evidence rather than a substitute for contract tests.

## Boundaries

`empack-core` contains pure values and planners with no runtime or I/O dependency.
`empack-lib` composes narrow ports, adapters and the operation engine. `empack`
owns the host runtime and executable lifecycle. CLI translation and presentation
live in the library application adapter. `empack-tests` exercises native executable
workflows; library suites verify provider, archive, filesystem and process contracts.

Do not introduce a broad session facade into the core, success-returning placeholder
verifiers, deserializable approval proofs, or a second command-specific publication
path. New features must contribute normalized intent, expected effects and verified
postconditions. The [feature requirements](docs/design/features.md) describe the useful capabilities
the implementation must provide.

## Source and documentation

Run formatting and Clippy. Document exported contracts with inputs, outcomes and
limitations. Prefer comments that explain an invariant over comments that restate
code. Use structured, redacted diagnostics and remove temporary debugging output.

Keep normative requirements in `docs/design/` and commands in `docs/usage.md`.
Update contracts with behavior changes; keep run-specific evidence in CI and PRs.
Do not add delivery ledgers, historical status reports or duplicate API sketches.

Write complete, direct sentences. Avoid hype, em dashes and fragment-heavy prose.
CLI flags and code retain their literal spelling. The brand reference is
`inherent.design/packages/docs/knowledge/process-notes/prose-and-communication-reference.md`
in the shared workspace.

## Fixtures and releases

Provider contracts use bounded local HTTP fixtures; opt-in live suites verify
current provider behavior. Keep source assertions and environment semantics in
fixtures rather than recording credentials or maintaining unused response archives.

Release builds derive their version from the release tag. Git-cliff generates
GitHub release notes from commits; release automation does not write historical
changelogs back into the source tree.

Contributions use the [Apache 2.0 license](LICENSE).
