# Native release distribution and trust

A native release is a portable, exact installation description. A channel selects
one immutable release without granting arbitrary code execution or tool updates.

## Release payload

| Field | Type | Contract |
| --- | --- | --- |
| `schema` | Integer | Strict release schema version |
| `pack` | `PackId` | Stable pack identity; display names cannot substitute |
| `version` | String | Human-readable label, not ordering or integrity evidence |
| `runtime` | Exact runtime requirements | Minecraft, loader and supported launch requirements |
| `files` | Logical file records | Roles, original assertions, destinations, side and choice requirements |
| `choices` | Stable choice definitions | Defaults, alternatives and constraints |
| `assets` | Immutable asset references | Authored content, templates already rendered for installation |
| `minimum_engine` | Version requirement | Compatibility gate checked before effects |

Host paths, signing keys, credentials, cache entries and operation journals are
excluded. References retain provider-qualified exact identities or approved URLs
with expected digests and sizes. A complete release can use several providers.
Computed export hashes do not replace original source assertions.

`ReleaseId` is SHA-256 of the exact UTF-8 payload bytes. Serialization is deterministic:
map keys have a prescribed order, sets are sorted, integers use canonical decimal
spelling and duplicate JSON keys are rejected. Verifiers hash received payload bytes;
they do not parse and reserialize before checking identity or signatures.

## Signed envelopes

Use Ed25519 signatures over domain-separated exact payload bytes. The signed context
includes the envelope kind and schema so a release signature cannot authenticate a
channel or tool manifest. The envelope carries the encoded payload, algorithm, key
identifier and signatures. Parsing is bounded before cryptographic verification;
validation rejects duplicate fields, unknown algorithms and ambiguous encodings.

A key identifier refers to an explicitly enrolled public key; it is not itself a
trust anchor. Interactive enrollment shows the pack, origin and key fingerprint.
An unattended host must receive trust configuration out of band. Downloading a key
from the same untrusted channel does not establish publisher identity.

Publisher authentication, original source integrity and local write authorization
remain separate proofs. The public API exposes `AuthenticatedRelease` only through
the verifier, never through a deserializable boolean or public unchecked constructor.

## Channels

| Field | Type | Contract |
| --- | --- | --- |
| `pack` | `PackId` | Matches the installed/subscribed pack |
| `channel` | Channel name | Matches the explicit subscription |
| `sequence` | Unsigned integer | Monotonically increasing within the subscribed trust context |
| `expires` | UTC timestamp | Reject expired metadata; unreasonable local clock blocks unattended use |
| `release` | Release ID, URL and byte bound | References one immutable payload |
| `minimum_engine` | Version requirement | No automatic executable replacement |

Persist the highest authenticated sequence in protected local state independently
of the installed release. Reject a lower sequence and a same-sequence different
payload. Explicit managed rollback does not reduce that floor. Failed content
acquisition can retry the same authenticated release without advancing installation.

Trust rotation requires a transition authenticated by an already trusted key and
proof of the new key, or explicit local reenrollment. Removed/revoked keys cannot
authorize new updates. Expiry bounds acceptance of stale channel metadata; offline
revocation cannot be discovered until fresh authenticated trust information arrives.
Signed metadata does not solve a compromised signing key without an independent
trust recovery procedure.

## Hosting and acquisition

Write immutable assets and release envelopes first; verify that they are available
before publishing a channel pointer. A consumer verifies channel identity, signature,
sequence and expiry, then release identity/signature, then acquisition assertions.
Redirects retain HTTPS and origin policy. Auth headers never cross origins implicitly.
Local explicitly selected snapshots use a caller-approved release digest and do not
create a subscription as a side effect.

## Program delivery

Pack updates and tool updates have separate trust roots, version policies and
execution grants. Native empack binaries are distributed for supported platforms.
A thin helper selects and verifies an executable, then delegates instance work to
it. It contains no independent resolver, downloader policy or publication algorithm.
A release's minimum-version requirement reports incompatibility; it cannot authorize
installing or executing an arbitrary binary.

## Wire encoding

Release and channel payloads use independent schema `1` JSON structs. Fields have
one prescribed writer order. Release files sort by logical key, choices by key,
choice alternatives lexically and assertions by algorithm. Download alternatives
retain priority order. Readers reject duplicate or unknown fields; they hash the
received bytes without reserialization. The release payload limit is 16 MiB and
the channel payload limit is 16 KiB.

The envelope fields are `schema`, `kind`, `payload` and `signatures`. Payload bytes,
key fingerprints and signatures use lowercase hexadecimal without whitespace or
prefixes. Each signature has `algorithm: ed25519`, a SHA-256 public-key fingerprint
in `key`, and a 64-byte `signature`. At most sixteen distinct keys may sign an
envelope. A release signature covers `empack.release.v1` followed by a zero byte
and the exact payload; a channel uses `empack.channel.v1` with the same separator.
Verification uses strict Ed25519 verification and rejects weak enrolled keys.

Channel expiry is Unix UTC seconds. A verifier accepts at most 31 days of remaining
validity and refuses clocks earlier than 2020. Release envelope URLs remain within
the explicitly enrolled HTTPS origin. Content-provider downloads have their own
acquisition policy; a publisher origin is not a restriction to one content provider.
An authenticated channel proposes a sequence floor. Durable subscription handling
must save that floor before acquisition and keep it across rollback and retries.

Release file records distinguish SHA-256 selected-byte identity from `assertions`,
which retain original provider digests. Acquisition checks both. Sources are
immutable relative assets, HTTPS alternatives, exact provider selections or manual
instructions. They cannot encode workspace paths. File destinations cannot select
instance control files. World content is restricted to initial-only seeds.

File records carry an explicit `layer`: `common`, `common-override`, `client` or
`server`. Side layers cannot participate on the other side. Selection resolves
choices before applying overlay priority, so disabling a side-specific optional
file exposes its common fallback. Equal-layer duplicate destinations and portable
case/Unicode aliases are errors. Array order never controls replacement.

## Author projection and assets

Native projection consumes the resolved author model and verified content, without
reading a foreign package index. It preserves all environment layers and optional
branches. Each file placement has a stable key derived from logical dependency,
file role, layer and destination; source locations are never serialized as host
paths. Optional boolean groups become `enabled`/`disabled` alternatives, retaining
their stable key, default and description. Conflicting group definitions fail.

`source` records provenance and exact acquisition. An optional `asset` names bundled
bytes without replacing that provenance. Authored local content uses an immutable
asset source. Assets are stored at `assets/<sha256>` and deduplicated by bytes;
different placements retain independent policies and permissions. Provider-owned
world members carry the archive selection, archive digests/size and exact member
path. Archive assertions are never copied into the member's assertion list.

The native archive contains `release.json` and its bundled immutable assets. The
writer checks the complete expected inventory independently after encoding ZIP,
TAR.GZ or 7z. Capture, candidate preparation and publication retain their resource
reservations and exact input read set. A changed author file invalidates a prepared
export. Export does not create a subscription or sign with an implicit key.

## Durable enrollment and observation

`instance subscribe --pack PACK --channel NAME URL --key PUBLIC_KEY` enrolls
explicit Ed25519 public keys for one channel. Public keys use 64 lowercase
hexadecimal characters. The approval preview shows the origin, channel and key
fingerprints. Enrollment does not install content or implicitly trust a downloaded
key. The root-bound schema `1` document at `.empack/subscription.json` holds the
channel URL, enrolled keys, exact observed envelope and authenticated sequence
floor. Retained envelopes must be reverified against current keys and time before
they select release content. Key revocation cannot be bypassed by an old proof.

`instance trust --key PUBLIC_KEY` replaces the enrolled key set after local
approval. `instance trust --revoke-all` disables channel authentication while
retaining its sequence floor. Reenrollment and revocation therefore cannot make
old channel metadata fresh again. Rotation through explicit local reenrollment
does not require a remote key-transition protocol.

`instance observe-channel [ENVELOPE]` fetches the enrolled channel when no local
envelope is selected. Both paths verify bounded signed bytes
against the current enrollment and persists its sequence and exact payload identity.
It does not acquire or install release content. Channel acquisition must complete
this publication before admitting content instructions. A same-sequence identical
payload can be retried; a lower sequence or different payload at the saved sequence
fails. A late trust edit invalidates the prepared operation. These control-document
changes use the same exact grants, native observations and recoverable publication
as other engine operations; they cannot write game files or installation history.

`instance update [ENVELOPE]` selects the exact release named by the retained,
authenticated channel. Selection verifies the envelope signature, payload identity,
size, pack and minimum engine version. The selected proof binds the native instance
root and exact subscription bytes; it cannot survive key rotation or a newer channel
observation. Preparation captures that subscription as part of the publication read
set. Expiry is checked again before publication, after acquisition and verification.
A valid signature from another trust context cannot bypass the enrolled keys.
Explicit snapshots remain separately selected by their expected payload digest.

Remote metadata requests use no provider credentials, cookies or automatic response
decompression. Every redirect remains within the initial HTTPS origin; response
status, declared size, running bytes and total elapsed time are checked. Received
metadata retains its resource reservation through signature verification. Fetching a
release uses the already published channel observation and its exact envelope size
bound. It cannot advance a sequence floor or select a newer release by itself.

Immutable remote asset paths resolve against the authenticated release envelope's
directory. Components are encoded as literal path segments; query credentials from
the envelope URL are not inherited. These locators are transient acquisition inputs.
The saved release retains its signed relative asset identity and original assertions.
Content transfer still requires the installation's explicit network grant and verifies
exact bytes before publication.

## Immutable publisher staging

`release stage SOURCE --key-file KEY` reads an extracted native export, verifies
its selected assets and original assertions, and signs the exact release payload.
The preview identifies the release, pack, public-key fingerprints and output paths.
Signing keys are explicit host files outside both the project and export roots;
the CLI accepts a 32-byte Ed25519 seed encoded as 64 lowercase hexadecimal characters.
Keys never enter project documents, distributions or receipts.

Approved staging publishes `dist/releases/<release-id>/release.json` as the signed
envelope, with its selected assets at their declared relative paths. Repeating the
same publication is harmless. Existing immutable files must match exactly; changing
signatures requires a newly identified release rather than overwriting an envelope
already served to clients. Input changes after preparation invalidate the operation.
Unknown neighboring files are not acquired or deleted.

Staging does not change a channel. The hosting layer serves `dist/` over HTTPS;
channel publication verifies the hosted release before exposing a new pointer.

## Channel publication

`release publish-channel RELEASE_ID --channel NAME --base-url HTTPS_DIRECTORY`
selects an already staged signed release. The caller supplies `--sequence`,
`--expires` (Unix UTC seconds) and one or more external `--key-file` inputs. The
base URL serves the publisher's `dist/` directory and has no query or credentials.

Preparation authenticates the staged envelope, checks local immutable assets and
captures the existing channel pointer. A previous pointer must authenticate under
the supplied signing keys or explicit `--previous-key` public keys. Its sequence
remains relevant after expiry; expired metadata never becomes installation authority.
A new pointer advances the sequence. Repeating identical channel metadata at its
existing sequence is allowed; changing metadata at that sequence is rejected.

After network approval, execution fetches the exact signed envelope and every
unique bundled asset from their HTTPS locations. Shared assets combine their source
assertions for one transfer. Missing, altered or unverifiable hosted bytes prevent
publication. Preparation and execution both enforce the 31-day freshness window.
The complete local read set is checked again before recoverable publication of
`dist/channels/<name>.json`.

For deployment by file transfer, upload immutable release files first, run channel
publication against the live HTTPS directory, then upload the resulting channel
file last. The command prepares local hosting output; it does not upload files or
manage a hosting account. A verification failure leaves the old pointer in place.

## Server entry points

An optional `server_launch` value names either `{kind: jar, path}` or
`{kind: arguments, unix, windows}`. Every named entry point must resolve to exactly
one required managed server file in the release inventory. This value contains no
program, shell command or arbitrary executable-selection policy. Argument-file
launches use the local `user_jvm_args.txt` seed before the verified loader arguments.
The same exact release binds runtime requirements, executable content, launch paths
and game files, so managed update and rollback cannot keep an older entry point.

## Enrollment-bound activation

A release's `require_subscription` flag binds native update policy to the exact
payload identity. Empack-managed native, Prism and server recipes set it. Activation,
repair and launch require a non-revoked, root-bound enrollment for the same pack,
even when a caller omits the CLI enrollment flag. Preparation captures that record;
revocation or replacement before publication invalidates the prepared operation.
An initial snapshot digest still authorizes only the selected bytes. Neither the
release nor an exported archive can enroll a key or pick a trusted origin for its
recipient. Snapshot recipes leave enrollment optional.
