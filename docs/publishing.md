# Publishing native releases

Publish immutable release assets before advancing a signed channel. Any static HTTPS
host can serve the output; empack does not upload files or configure hosting.

## Signing keys

A private key file contains a raw 32-byte Ed25519 seed as 64 lowercase hexadecimal
characters. A subscriber's `--key` takes the corresponding 32-byte public key in the
same hexadecimal encoding. PEM and OpenSSH files are not accepted directly.

Create a publisher workspace and a private key directory outside all projects,
exports and hosted output, then initialize a native key without external conversion:

```sh
mkdir -p ./publisher
mkdir -m 700 ~/.config/empack-publisher
empack --workdir ./publisher --dry-run release init --key-file ~/.config/empack-publisher/publisher.key
empack --workdir ./publisher --yes release init --key-file ~/.config/empack-publisher/publisher.key
```

The parent key directory must already exist and be private to the current user
(0700 on Unix, protected native ACL on Windows). The key is created exclusively
with private permissions; an existing file or symlink is never replaced. Preview
creates no key. A failed write retires only its own temporary file. No publisher
configuration is needed: `--workdir` selects the workspace and `--key-file` remains
an explicit input to each signing operation. Keep keys outside every export root,
including exports unrelated to this publisher workspace.

The command prints the public key, fingerprint and deployment order, never the
private seed. Give the public key to subscribers through an independent trusted
route for `instance subscribe --key`; setup does not enroll any subscriber.
Back up the private key securely. `instance trust --key` replaces a subscriber's
enrolled keys during deliberate rotation.

## Native releases delivered to Prism

Keep the launcher-owned icon policy explicit in the author project's
`empack.bindings.yml` when publishing native releases to Prism subscribers:

```yaml
policies:
  icon.png: seed
```

The Prism exporter supplies this default for its own managed distributions; the
generic native exporter remains consumer-neutral. A Prism instance rejects a
native release that selects a managed game-root icon, including signed updates,
repair and rollback. Rebuild and publish the corrected release rather than forcing
an ownership conflict. The recipient never changes an authenticated policy.

Updating an old managed icon to seed preserves an existing regular file's current
bytes, including Prism's rewrite, without a conflict override. This does not relax
ownership of other PNGs or authorize replacing directories/symlinks. Upgrading the
executable alone does not alter already published release descriptors.

## Staging and deployment

Extract a native export into `./export`, create the publisher directory, and stage
the signed immutable files:

```sh
mkdir -p ./publisher
empack --workdir ./publisher --yes release stage ./export --key-file ~/.config/empack-publisher/publisher.key
```

The output is `publisher/dist/releases/<release-id>/release.json` and its assets.
Serve `publisher/dist/` through static HTTPS hosting. Staging checks original asset
assertions and refuses different bytes at existing release addresses. It does not
advance a channel or enroll subscribers. Use `--dry-run` to inspect the release,
output paths and signing fingerprints before publication.

Only assets selected by the release descriptor are staged; adjacent source files
stay in the export. Re-staging identical output is a no-op. Different bytes at an
existing selected asset or envelope are refused. Signing the same release payload
with different keys does not authorize replacing its existing immutable envelope.

After the immutable release is available through HTTPS, prepare its channel pointer:

```sh
empack --workdir ./publisher --yes release publish-channel RELEASE_ID \
  --channel stable --base-url https://packs.example.org/ \
  --sequence 1 --expires UNIX_UTC_SECONDS \
  --key-file ~/.config/empack-publisher/publisher.key
```

Choose a future expiry within 31 days. This verifies the hosted envelope and assets
before writing `publisher/dist/channels/stable.json`. If deployment uses uploads,
upload this pointer last. Increase the sequence when changing channel metadata.
`--previous-key` accepts an old public key for authenticating the existing pointer
after signing-key rotation; it does not sign the replacement or enroll client trust.

Subscribers enroll the hosted `channels/stable.json` URL, run `instance check-updates`,
then `instance update`. Existing instances infer their validated side and layout;
explicit conflicts fail before release acquisition. First installations default to
client/game and display that selection; choose `--side server` for a new server.
`instance update --check` composes a fresh check and installation with separate
approvals. A declined or previewed check never proceeds to installation. See [usage](usage.md#native-release-example)
for installation and enrollment commands and [release contracts](design/releases.md)
for expiry, key rotation and replay protection.

Checking updates authenticates the signed channel and, after approval, saves its
anti-replay floor without changing game content. Preview saves nothing and grants
no later installation approval. Update uses the saved exact release; failed
installation and rollback retain the floor. Output identifies the offered release,
sequence and expiry, and update shows the installed and offered human versions with
exact IDs. Channel transport failure is an error, never a claim of being up to date.
`observe-channel` remains a compatible alias for `check-updates`.

Renewing expiry requires another `release publish-channel` with a higher sequence
and future expiry, followed by uploading the signed channel file last. No daemon
renews expiry and no client expiry check is relaxed.
