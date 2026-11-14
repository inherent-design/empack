# Publishing native releases

Publish immutable release assets before advancing a signed channel. Any static HTTPS host can serve the output; empack does not upload files or configure hosting.

The author project, publisher workspace and installed instance have different roles. Author commands resolve dependencies and build distributions. `release` commands prepare signed public files in a publisher workspace. Recipient `instance` commands verify and apply those releases. Generated GitHub workflows publish distribution archives and checksums; advancing an HTTPS update channel requires the deployment steps below.

## Release sources

Build named profiles from one author project and lock. Reference snapshots prepare fixed content; `updates: empack` profiles require publisher enrollment and check for future offers. Use `build --explain` to inspect the selected recipe and output paths. Set the profile's update policy explicitly for CI, or override it for a manual build:

```sh
empack --workdir ./pack build --profile launcher --profile dedicated --updates empack
```

Extract the resulting archives into separate directories. Pass the directory containing the native payload and its assets to `release stage`:

| Build output             | Release source directory                     |
| ------------------------ | -------------------------------------------- |
| Managed Prism ZIP        | `./prism-export/.minecraft/.empack-consumer` |
| Managed server archive   | `./server-export/.empack-consumer`           |
| Native `.empack` archive | `./native-export`                            |

Client and server distributions share the pack identity and version but can contain different exact inventories. Separate channels such as `stable-client` and `stable-server` are useful for those variants; channel names do not impose a client/server type. Release payloads support side layers. The current generic native exporter includes content and runtime requirements but does not prepare a managed server entry point; publish the server export's embedded release for complete server runtime updates.

An archive does not enroll its recipient or establish publisher trust. The recipient runs `instance subscribe` once with the intended pack ID, channel URL and independently supplied public key. Subscribed exports then use their generated update integration. Snapshot exports remain fixed even when content is downloaded during installation. See [distribution selection](usage.md#distribution-selection) and [instance enrollment](usage.md#native-release-example).

## Signing keys

A private key file contains a raw 32-byte Ed25519 seed as 64 lowercase hexadecimal characters. A subscriber's `--key` takes the corresponding 32-byte public key in the same hexadecimal encoding. PEM and OpenSSH files are not accepted directly.

Create a publisher workspace and a private key directory outside all projects, exports and hosted output, then initialize a native key without external conversion:

```sh
mkdir -p ./publisher
mkdir -m 700 ~/.config/empack-publisher
empack --workdir ./publisher --dry-run release init --key-file ~/.config/empack-publisher/publisher.key
empack --workdir ./publisher --yes release init --key-file ~/.config/empack-publisher/publisher.key
```

The parent key directory must already exist and be private to the current user (0700 on Unix, protected native ACL on Windows). The key is created exclusively with private permissions; an existing file or symlink is never replaced. Preview creates no key. Cleanup targets only the attempt's own temporary link. If creation succeeds but directory synchronization or link cleanup fails, the diagnostic says that the key was created. No publisher configuration is needed: `--workdir` selects the workspace and `--key-file` remains an explicit input to each signing operation. Keep keys outside every export root, including exports unrelated to this publisher workspace.

The command prints the public key, fingerprint and deployment order, never the private seed. Give the public key to subscribers through an independent trusted route for `instance subscribe --key`; setup does not enroll any subscriber. Back up the private key securely. `instance trust --key` replaces a subscriber's enrolled keys during deliberate rotation.

## Native releases delivered to Prism

Keep the launcher-owned icon policy explicit in the author project's `empack.bindings.yml` when publishing native releases to Prism subscribers:

```yaml
policies:
  icon.png: seed
```

The Prism exporter supplies this default for its own managed distributions; the generic native exporter remains consumer-neutral. A Prism instance rejects a native release that selects a managed game-root icon, including signed updates, repair and rollback. Rebuild and publish the corrected release rather than forcing an ownership conflict. The recipient never changes an authenticated policy.

Updating an old managed icon to seed preserves an existing regular file's current bytes, including Prism's rewrite, without a conflict override. This does not relax ownership of other PNGs or authorize replacing directories/symlinks. Upgrading the executable alone does not alter already published release descriptors.

## Staging and deployment

Select a source directory from the table above, create the publisher directory, and stage the signed immutable files. For an extracted managed server archive:

```sh
mkdir -p ./publisher
empack --workdir ./publisher --yes release stage ./server-export/.empack-consumer --key-file ~/.config/empack-publisher/publisher.key
```

The output is `publisher/dist/releases/<release-id>/release.json` and its assets. Serve `publisher/dist/` through static HTTPS hosting. Staging checks original asset assertions and refuses different bytes at existing release addresses. It does not advance a channel or enroll subscribers. Use `--dry-run` to inspect the release, output paths and signing fingerprints before publication.

Only assets selected by the release descriptor are staged; adjacent source files stay in the export. Re-staging identical output is a no-op. Different bytes at an existing selected asset or envelope are refused. Signing the same release payload with different keys does not authorize replacing its existing immutable envelope.

After the immutable release is available through HTTPS, use `release channel create` with its release ID, channel name, HTTPS base directory, increasing sequence and signing key. The [deployment example](#static-host-deployment) below shows the complete copy, verification and pointer switch.

This verifies the hosted envelope and assets before writing `publisher/dist/channels/stable.json` locally. A successful command creates the signed file; it does not upload it or change the hosted channel. If deployment uses uploads, upload this pointer last. Increase the sequence when changing channel metadata. `--previous-key` accepts an old public key for authenticating the existing pointer after signing-key rotation; it does not sign the replacement or enroll client trust.

Subscribers enroll the hosted `channels/stable.json` URL, then run `instance update` to check the latest offer without applying it. Use `instance update --apply` to save the verified offer and install its release through separate approvals. Existing instances infer their validated side and layout; explicit conflicts fail before release acquisition. First installations default to client/game and display that selection; choose `--side server` for a new server. See [usage](usage.md#native-release-example) for installation and enrollment commands and [release contracts](design/releases.md) for channel identity, key rotation and replay protection.

A preview saves neither the offer nor game content. A new offer remains an offer-only preview until accepted; an already saved offer permits a full installation preview. `--yes` accepts prompts only when application was explicitly requested. Saving the offer retains its anti-replay floor even if subsequent installation fails or is declined. For a channel file supplied separately from its hosted release, `instance update --channel-file FILE --apply` verifies and saves that explicit offer before fetching the exact remote release and assets. It does not replace the enrolled URL. `instance check-updates` performs only the explicit observation save; a local signed release passed to `instance update` uses the already saved offer without a channel request. Channel transport failure is an error, never a claim of being up to date.

Channels have no calendar expiry. An unchanged offer remains usable without renewing metadata or publishing blank updates. Advance the sequence only when changing the offer; republishing the exact same signed pointer is harmless. To offer an older release deliberately, publish a higher sequence selecting it.

## Static host deployment

The public tree follows the staged layout. Each release has its own assets; the channel is the small mutable pointer:

```text
channels/stable.json
releases/<release-id>/release.json
releases/<release-id>/assets/<content-hash>
```

For a host serving `/srv/www/pack/` at `https://packs.example.org/`, the deployment can run beside the web server as follows. Replace `RELEASE_ID` with the ID printed by staging; the deployment account needs write access to that public root.

```sh
release_id="RELEASE_ID"
public_root="/srv/www/pack"
mkdir -p "$public_root/releases/$release_id" "$public_root/channels"

rsync -a --ignore-existing \
  "./publisher/dist/releases/$release_id/" \
  "$public_root/releases/$release_id/"

empack --workdir ./publisher --yes release channel create "$release_id" \
  --channel stable --base-url https://packs.example.org/ \
  --sequence 1 \
  --key-file ~/.config/empack-publisher/publisher.key

install -m 644 ./publisher/dist/channels/stable.json \
  "$public_root/channels/.stable.json.next"
mv "$public_root/channels/.stable.json.next" "$public_root/channels/stable.json"
```

The copy preserves existing immutable destinations, and channel creation verifies their actual hosted bytes. Publish the pointer by a same-directory rename only after that verification succeeds. Remote deployment can transfer the same selected files through SSH or another hosting tool. Serve valid HTTPS; channel files should revalidate promptly, while immutable release files can use long-lived caching. Keep signing keys and publisher state outside the public tree.

For a subsequent pack version, reconcile the author changes, build and stage a new release, deploy its directory, then advance the channel sequence. The version label is not a sequence counter; the caller supplies `--sequence`. Keep the publisher's current channel file and signing identity between deployments.

Historical hosting retains old release directories. Latest-only hosting uses the same protocol: publish the new immutable directory, switch the channel, and retire explicitly selected old directories after allowing for downloads already in progress. Removing old assets can make old direct links and uncached rollback unavailable. Do not overwrite an old release path with new content.
