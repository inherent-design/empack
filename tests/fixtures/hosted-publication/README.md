# Hosted publication fixtures

These original synthetic files are licensed under Apache-2.0, like empack. They
contain a 45-byte configuration file and two native release documents. No game,
mod, launcher or third-party asset is included. Minecraft and Java fields are
declarations; the publication tests do not start either program.

`manifest.json` records exact input, envelope and asset identities. Its two signing
seeds are public test data. They are used only to exercise publisher verification
and key rotation. Each release is signed with its corresponding fixture key.

The unsigned documents were written as literal JSON. The recorded empack executable
then staged each through `release stage`, using the fixture key outside the input
and publisher directories. The staged envelope was frozen here. Reproduction uses
the ordinary command:

```sh
empack --workdir PUBLISHER --yes release stage INPUT --key-file FIXTURE_KEY
```

`INPUT` is a copy of `inputs/a` or `inputs/b`; `FIXTURE_KEY` contains that manifest
entry's hexadecimal seed and must be outside `INPUT` and `PUBLISHER`. Compare the
resulting `dist/releases/RELEASE_ID/` with `good/releases/RELEASE_ID/`. Expected
payload and file hashes are computed independently from the literal bytes, rather
than from a candidate publication plan. The executable test also verifies channel
signatures directly with Ed25519 and checks the complete decoded pointer fields.

The other directories deliberately change the hosted result:

| Directory | Hosted result |
| --- | --- |
| `good` | Exact staged envelopes and configuration bytes. |
| `wrong-envelope` | Different bytes at the selected release-envelope address. |
| `wrong-asset` | Exact envelope, but corrupt configuration bytes. |
| `missing-asset` | Exact envelope, with the configuration absent. |

The live executable test pins the fixture commit and reads these files through
GitHub's ordinary HTTPS raw-content route. It publishes a pointer only in its
temporary local workspace. The repository contains no deployed channel pointer.
Successful local publication does not establish network deployment or consumer
startup. The live test remains distinct from controlled-host tests and is excluded
from ordinary offline runs.

Git attributes preserve these files byte-for-byte on every checkout platform.
Changing the fixture requires a new immutable source pin and reviewed manifest
hashes; a moving branch URL cannot substitute for that pin.
