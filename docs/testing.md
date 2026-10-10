# Verification

The [acceptance contracts](design/verification.md) define v0.6.0-beta behavior.
Tests verify the checked-out implementation; a contract or test count is not evidence
that an unwired capability works. Record run-specific results in CI and PRs.

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

Formatting and Clippy apply to all targets/features. Native filesystem/process
claims require real temporary roots and OS primitives; pure planner tests cannot
prove link handling, locks or crash durability.

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

The runtime matrix covers vanilla, Fabric launcher layouts, Quilt, historical and
modern Forge, and early/current NeoForge families. Validate official installer
profiles, expected libraries and launch arguments. Help output or EULA refusal
proves launcher execution, not gameplay; tests never accept the EULA.

## Consumer and release evidence

Inspect exported manifests and archives independently, then test the actual supported
consumer. Prism ZIP imports, Modrinth and CurseForge format acceptance, server startup
and native instance updates are separate gates. Do not assume a TAR/7z tree is
importable wherever a ZIP is accepted.

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
