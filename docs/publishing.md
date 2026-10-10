# Publishing native releases

Publish immutable release assets before advancing a signed channel. Any static HTTPS
host can serve the output; empack does not upload files or configure hosting.

## Signing keys

A private key file contains a raw 32-byte Ed25519 seed as 64 lowercase hexadecimal
characters. A subscriber's `--key` takes the corresponding 32-byte public key in the
same hexadecimal encoding. PEM and OpenSSH files are not accepted directly.

The following example requires Python 3 and OpenSSL with Ed25519 support. It creates
new files with private permissions and refuses to overwrite existing keys. Keep this
directory outside authoring projects, export roots and hosted output.

```sh
python3 - <<'PY'
import os
from pathlib import Path
import subprocess

key_dir = Path.home() / '.config/empack'
key_dir.mkdir(parents=True, exist_ok=True, mode=0o700)
private = subprocess.run(
    ['openssl', 'genpkey', '-algorithm', 'ED25519', '-outform', 'DER'],
    check=True, capture_output=True).stdout
public = subprocess.run(
    ['openssl', 'pkey', '-inform', 'DER', '-pubout', '-outform', 'DER'],
    input=private, check=True, capture_output=True).stdout
assert len(private) == 48 and private.startswith(bytes.fromhex('302e020100300506032b657004220420'))
assert len(public) == 44 and public.startswith(bytes.fromhex('302a300506032b6570032100'))
for name, raw in [('publisher.key', private[-32:]), ('publisher.pub', public[-32:])]:
    fd = os.open(key_dir / name, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, 'w') as output:
        output.write(raw.hex() + '\n')
print('Created publisher.key and publisher.pub in', key_dir)
PY
```

Keep `publisher.key` private. Share only `publisher.pub` through a trusted route.
Give its contents to subscribers for `instance subscribe --key`; a key fetched from
the same untrusted channel is not independent proof of publisher identity. Back up
the private key securely. `instance trust --key` replaces a subscriber's enrolled
keys during deliberate rotation.

## Staging and deployment

Extract a native export into `./export`, create the publisher directory, and stage
the signed immutable files:

```sh
mkdir -p ./publisher
empack --workdir ./publisher --yes release stage ./export --key-file ~/.config/empack/publisher.key
```

The output is `publisher/dist/releases/<release-id>/release.json` and its assets.
Serve `publisher/dist/` through static HTTPS hosting. Staging checks original asset
assertions and refuses different bytes at existing release addresses. It does not
advance a channel or enroll subscribers. Use `--dry-run` to inspect the release,
output paths and signing fingerprints before publication.

After the immutable release is available through HTTPS, prepare its channel pointer:

```sh
empack --workdir ./publisher --yes release publish-channel RELEASE_ID \
  --channel stable --base-url https://packs.example.org/ \
  --sequence 1 --expires UNIX_UTC_SECONDS \
  --key-file ~/.config/empack/publisher.key
```

Choose a future expiry within 31 days. This verifies the hosted envelope and assets
before writing `publisher/dist/channels/stable.json`. If deployment uses uploads,
upload this pointer last. Increase the sequence when changing channel metadata.
`--previous-key` accepts an old public key for authenticating the existing pointer
after signing-key rotation; it does not sign the replacement or enroll client trust.

Subscribers enroll the hosted `channels/stable.json` URL, run `instance observe-channel`,
then `instance update` (`--side server` for servers). See [usage](usage.md#native-release-example)
for installation and enrollment commands and [release contracts](design/releases.md)
for expiry, key rotation and replay protection.
