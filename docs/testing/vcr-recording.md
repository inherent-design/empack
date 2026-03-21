# VCR recording guide

For the default verification matrix, see [`README.md`](README.md).

## Current role of VCR fixtures

The repository contains recorded HTTP fixtures under `crates/empack-tests/fixtures/cassettes/` and cassette loader helpers in `crates/empack-tests/src/fixtures.rs`.

These fixtures are useful for VCR-backed maintenance work such as:

- maintaining recorded API examples
- targeted cassette loader checks
- future higher-fidelity provider tests

They are not the default release gate. The trusted default matrix is documented in the [testing matrix](README.md).

## Prerequisites

The recording script expects:

- `curl`
- `jq`
- `.env.local` with `EMPACK_KEY_CURSEFORGE`

If you need a starting point, copy `.env.local.template` to `.env.local` and fill in the required key.

## Script entry points

From the repository root:

```bash
./scripts/record-vcr-cassettes.sh --help
./scripts/record-vcr-cassettes.sh --dry-run
./scripts/record-vcr-cassettes.sh --only modrinth/search_sodium
./scripts/record-vcr-cassettes.sh
```

The script records the current cassette set, sanitizes the CurseForge API key, and validates JSON output.

## Verify cassette changes

After updating fixtures, run the smallest relevant follow-up checks:

```bash
jq empty crates/empack-tests/fixtures/cassettes/modrinth/search_sodium.json
cargo test -p empack-tests fixtures::tests::test_load_vcr_cassette -- --exact
cargo test -p empack-tests fixtures::tests::test_load_vcr_body_string -- --exact
```

These checks confirm that the recorded file is valid JSON and that the current cassette loader helpers can still parse the recorded response body.

## Current boundaries

- VCR-backed work is separate from the trusted nextest-based release-gate matrix.
- Recording touches live network services and can fail due to rate limits or API drift.
- Keep live-recording claims out of the default workflow guidance unless that path is promoted into the trusted matrix.

## Current cassette set

The script currently manages fixtures across four buckets:

- `modrinth/`
- `curseforge/`
- `loaders/`
- `minecraft/`

Inspect the current fixture tree with:

```bash
find crates/empack-tests/fixtures/cassettes -maxdepth 2 -type f | sort
```

## References

- [Testing matrix](README.md)
- [`docs/reference/MODRINTH.md`](../reference/MODRINTH.md)
- [`docs/reference/CURSEFORGE.md`](../reference/CURSEFORGE.md)
