# Contributing to empack

The [v0.5 design](docs/design/README.md) is the implementation target. Check the
[ledger](docs/design/implementation.md) before assuming a proposed API exists.
Keep useful behavioral fixtures and remove tests that require obsolete behavior.

## Development

Use the pinned Rust toolchain, cargo-nextest and mise. Strict live tests additionally
require packwiz, Java, provider access and any documented credentials. Never commit
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
owns CLI translation, presentation and the host runtime. Existing `empack-tests`
fixtures remain during migration toward reusable adapter contract suites.

Do not introduce a broad session facade into the core, success-returning placeholder
verifiers, deserializable approval proofs, or a second command-specific publication
path. New features must contribute normalized intent, expected effects and verified
postconditions. The [feature requirements](docs/design/parity.md) describe the useful capabilities
the implementation must provide.

## Source and documentation

Run formatting and Clippy. Document exported contracts with inputs, outcomes and
limitations. Prefer comments that explain an invariant over comments that restate
code. Use structured, redacted diagnostics and remove temporary debugging output.

Keep normative requirements in `docs/design/`. Old specifications are removed. Git history retains historical behavior; it must
not compete with the target. Convert public API sketches into compiled
examples as those APIs land. Update the implementation ledger with each landing.

Write complete, direct sentences. Avoid hype, em dashes and fragment-heavy prose.
CLI flags and code retain their literal spelling. The brand reference is
`inherent.design/packages/docs/knowledge/process-notes/prose-and-communication-reference.md`
in the shared workspace.

## Fixture maintenance

For recorded provider cassettes, preview with
`./scripts/record-vcr-cassettes.sh --dry-run`, then record explicitly and run
`cargo test -p empack-tests fixtures::tests::test_load_vcr_cassette -- --exact`.
Live recording uses local credentials and requires `curl` and `jq`.

Release builds derive their version from the release tag. `CHANGELOG.md` is
historical, generated release evidence; do not rewrite it as target design.

Contributions use the [Apache 2.0 license](LICENSE).
