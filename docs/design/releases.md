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
