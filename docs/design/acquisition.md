# Resolution and acquisition ports

Contract for v0.5.0-alpha.1. Callable types and signatures are defined in the
[provider catalog](../../crates/empack-lib/src/engine/providers.rs), [HTTP acquisition](../../crates/empack-lib/src/engine/acquisition.rs), [imports](../../crates/empack-lib/src/engine/import.rs) and [content storage](../../crates/empack-lib/src/engine/content.rs). This page specifies their behavior and ownership.

## Resolution and import ports

### Object-safe async conventions

Use static dispatch for pure helpers. Use object-safe traits only at infrastructure seams that benefit from replacement in tests or alternative implementations.

An infrastructure trait intended for `Arc<dyn Trait>` must use a dyn-compatible
return type, such as a boxed `Send` future. Do not show native `async fn` methods in such a trait and assume they can be used as trait objects. This follows Rust's dyn-compatibility restrictions; concrete `Engine` methods can simply be `async fn`. [R12](https://doc.rust-lang.org/reference/items/traits.html#dyn-compatibility)

### Provider catalog

`ResolveQuery` includes game/loader compatibility, content kind, explicitly permitted alternative game versions, and provider preference. Search ranking is selection assistance, not canonical identity. Direct IDs bypass search ranking, not validation or ownership checks.

`ProviderResolution` returns canonical identity, exact files, requirements and dependency evidence, source metadata, and explicit capability limitations. Authentication failure, throttling, provider failure, and genuine not-found are separate errors. None silently becomes an unidentified local file unless the user's policy explicitly permits that fallback and the diagnostic retains why it happened.

The compiled search adapter uses [Modrinth search facets](https://docs.modrinth.com/api/operations/searchprojects/)
and [CurseForge's class/game search](https://docs.curseforge.com/rest-api/#search-mods).
Its retained windows expose pagination and incomplete coverage; they are not resolved
selections. See the [provider search API](api.md).

The concrete catalog is `engine::providers::ProviderCatalog`; see its [API contracts](api.md).
Its exact-selection adapters follow the [Modrinth version contract](https://docs.modrinth.com/api/operations/getversion/)
and [CurseForge file contract](https://docs.curseforge.com/rest-api/).
They retain all declared files and relations before any placement or optionality
conversion. A provider's environment metadata describes support; it does not
override the user's required/optional intent.

The compiled catalog supports content identification through
[Modrinth's hash endpoint](https://docs.modrinth.com/api/operations/versionfromhash/)
and [CurseForge's game-specific fingerprint lookup](https://docs.curseforge.com/rest-api/#get-fingerprints-matches-by-game-id).
A fingerprint result still requires declared digest and size checks. See the
[content identification API](api.md#compiled-content-identification-api) for the concrete
API and its current boundaries.

### Transport is not exposed to command handlers

Provider implementations receive a transport policy object that owns credential scope, redirects, retry classification, timeouts, response limits, and rate reservations. Request builders supply endpoint-specific data; they do not each create an unconstrained HTTP client.

Retries apply only to operations classified as safe to retry. A rate-limit cooldown and a capacity reservation are different concepts. Relative
cooldowns use monotonic deadlines with their fractional-second precision preserved;
wall-clock rounding must not let a request resume before the declared interval. Retrying acquisition must not reset the cumulative byte or deadline budget indefinitely.

Only send a credential to its intended provider origin. Strip sensitive headers on cross-origin redirects unless an explicit credential rule allows them. Log redacted locators and stable provider/file IDs, not bearer tokens or signed query strings.

CurseForge CDN acquisition needs a separate, fixed credential rule for HTTPS
`edge.forgecdn.net` downloads. Its [June 2026 announcement](https://blog.curseforge.com/introducing-api-key-authentication-for-curseforge-file-downloads/)
sets July 16 as the start of API-key enforcement. Supply `x-api-key` through the
configured host credential, never a persisted URL or project document. Re-evaluate
the rule for every redirect; arbitrary mirrors and alternate origins do not inherit
the key. A rejected or missing key remains an authentication failure, not evidence
that a file is absent or that its declared digest may be changed.

The compiled `ProviderCatalog::configure_acquisition` applies this rule to an
`HttpAcquisition` without exposing the credential. `Engine::with_provider_catalog`
configures both catalog and acquisition together. Only HTTPS on port 443 at the
exact CDN hostname receives the header; alternate ports, subdomains and other
origins do not. Replacing the catalog also replaces or clears that credential.
Standalone import/acquisition hosts apply the same configuration explicitly.

### Import adapters produce data, not project changes

The compiled inspection boundary is documented in [the import inspection API](api.md#compiled-import-inspection-api).
Wire semantics follow the [mrpack specification](https://support.modrinth.com/en/articles/8802351-modrinth-modpack-format-mrpack)
and the [CurseForge export structure](https://support.curseforge.com/support/solutions/articles/9000198500-exporting-a-modpack-for-curseforge-project-submission).
The native import host
connects explicitly selected archives and provider modpack pages to verified interpretation
and approved publication. The CLI import host supplies those selections and decisions.

Run synchronous archive parsing in a bounded blocking worker over a file-backed quarantined source. Parsing receives no live project root, backend runner, or publisher.

Preserve format record locations in diagnostics: file path, manifest JSON pointer or line, and imported destination. Datapack-folder inference is a pure helper returning evidence and a proposed layout choice; it does not immediately modify backend options.

Import supports mrpack and CurseForge ZIP. It can recognize packwiz metadata
without supporting packwiz-directory import; detection never authorizes execution.

`prepare_import` resolves provider records, checks representability, validates all destinations/layers, verifies required external or embedded bytes, and builds candidate intent before replacement is authorized. Required unresolved/restricted content returns `NeedsInput` rather than deleting the previous project.

## Acquisition, content storage, and archive handling

### Acquisition API

The acquisition layer streams to an owned quarantine file, checks status and origin policy, applies `Content-Length` as an early rejection only, and counts actual received bytes. It hashes incrementally and verifies expected size/digests before returning `AcquiredContent`.

Exact provider refresh prefers current download origins and retains distinct saved origins
as fallbacks. A returned URL is not evidence that its server will deliver the file. Every
alternative must satisfy the original content assertions; refresh cannot replace those
assertions or silently update the locked selection.

An HTML error page with a successful status is not accepted as a JAR solely because of its filename. Content identification and archive parsing remain explicit validation steps. Do not overwrite declared source hashes with hashes of the response.

`CacheAccess` in a request is an upper-bound preference, not authority. `AcquisitionContext` privately contains either `Arc<dyn ContentLookup>`, `Arc<dyn ContentStore>`, or no cache capability, selected by the engine at the public entry point. Helpers cannot upgrade that context. `VerifiedAcquisition` itself stores no global cache writer and receives only a scratch-file factory, never a live project root factory. A write request under read-only authority returns `CapabilityDenied`.

Cache insertion is permitted only after verification and only with `ReadWrite` authority. A preview can use retained cache bytes through `ReadOnly` but cannot update access timestamps, write an index, evict objects, or create a durable continuation record.

### Content leases and cache ownership

A retained lease keeps bytes readable for an active operation even if an index entry is evicted. Cache lookup and native-handle retention or pin acquisition are one coordinated operation; do not find a path and reopen it later after another process can delete it. Preview retention uses read handles or in-memory coordination, not persistent pin/LRU writes. Implementations may prevent eviction or retain an open object through unlink, but must satisfy the same readable-lease contract on each platform.

The cache is not user intent and not the operation journal. A lost disposable cache may cause reacquisition; it must not destroy the only recovery preimage. Recovery data has an independent retention policy.

A content store can begin as verified files plus a small index. It does not require a database. Metadata index transactions, if introduced, still do not make file operations outside that index transactional.

The native implementation uses SHA-256-addressed files in a host-private directory.
Its configured entry limit counts canonical objects. Traversal permits up to 100,000
additional unknown/control neighbors plus the coordination file, and checks
cancellation during enumeration. Cleanup counts without collecting metadata while
holding shared coordination, then admits the observed selection size before capture.
An empty store therefore does not reserve memory for its configured maximum size.
`FileContentStore` can insert verified leases; its separate `FileContentLookup` view
cannot write. `CachedFileRequest` carries the current source assertions and policy.
Lookup verifies those assertions and the content address while copying into owned
private storage under a shared OS lock. The copy remains readable after cache
eviction and retains its resource reservation until its last reader closes. This
uses additional temporary disk space in exchange for a consistent lease contract on
all supported platforms. An address is never promoted to independent source evidence.

Insertion takes exclusive coordination, verifies streamed bytes again, enforces
object/count/total limits and publishes a synchronized temporary file. Existing
objects are checked before reuse. Corrupt objects, links and special files are errors;
unrecognized neighboring files are preserved. Lookup does not create directories,
coordination files, indexes, pin records or application access timestamps. Native
filesystem access-time behavior remains controlled by the host filesystem. This
store is disposable and must never hold the only publication recovery preimage.

### Archive interface and limits

The adapter first validates metadata, entry count, path syntax, duplicate/case collisions, entry kinds, and declared sizes. Extraction still enforces actual per-entry and total expanded-byte limits. Metadata can lie. Use checked arithmetic and count partial output even if an entry eventually fails CRC or digest validation.

Reject archive-provided symlinks, hardlinks, device entries, and unsupported special files unless a narrowly defined future feature explicitly represents them. Existing user templates containing links are a separate compatibility decision, not permission to accept arbitrary archive link entries.

Open an archive once where possible and retain its reader. Avoid rereading the whole compressed file for every entry. Nested archive processing has a separately bounded policy; do not recursively expand arbitrary nested files merely because they look like ZIPs.

A transferred archive's digest and the digests of its content entries are separate. Successful ZIP parsing does not establish that its file semantics are supported.

### Transfer reservations

Known file sizes reserve their validated byte length rather than the entire configured
per-file ceiling. Unknown sizes use the smaller of that ceiling and currently
available scratch capacity; admission then reserves it atomically, and the same cap
applies to the receive stream and quarantine writer. Verified leases retain only
actual bytes and their open file. Retained content therefore cannot block a later
small file solely because its configured maximum is larger than the remaining budget.

### Shared private content backing

Build inputs and downloaded runtime libraries consolidate verified bytes into an
append-only private pool. Logical leases retain independent readers and original
source evidence. Sharing a SHA-256 content address never upgrades an observed file
or an MD5 declaration to stronger source assurance.

Each pool owns one file and its private directory handle. Member readers are bounded
to their own ranges, and publication checks the content address again. The last
reader keeps the backing alive. Async acquisition reserves scratch before each new
range and retains that charge on the backing, including a failed partial append.
After an append failure, existing ranges remain readable but the pool accepts no
further writes. No compaction or early range reclamation is implied.

Moving a file into the pool briefly needs both its quarantine and destination bytes.
If that overlap cannot be admitted, acquisition retains the already charged original
lease. Its native handle remains charged too; pooling does not become an extra
requirement for an otherwise valid download. Cancelled/closed admission and failed
pools still reject work. Uncharged inputs cannot use this fallback.
An existing clone of the original lease retains its original reservation until it
retires. Synchronous build assembly covers pools with its enclosing worker budget;
async acquisition owns explicit handle, metadata and byte reservations. This is
operation-private storage, not the persistent content cache.

The compiled build acquisition path validates the entire HTTP request inventory
before transfer and uses one cumulative byte/deadline budget, including alternatives.
It returns no acquired subset after a failed request. Manual and embedded obligations
remain explicit pending input.

Synchronization materialization uses the same cache-obligation verifier as builds.
After explicit acquisition approval, it checks original hashes, size and observations
before refreshing provider locators. Hits can restore missing placements without a
provider request. Corrupt or unavailable cache entries remain misses; they cannot
change pins or authorize different bytes. A complete verified acquisition batch may
populate disposable storage before the separate project publication decision. Preview
performs neither acquisition nor cache publication.

The host also gives add/import acquisition a read-only cache capability. Approved
build execution grants its runtime/bootstrap transport cache insertion. Exact runtime
assets and digest-bound metadata can be reused; unasserted catalog snapshots are
fetched afresh and are not inserted as reusable source evidence. Cached bytes count
against per-file and batch allowances. A failed HTTP batch inserts no successful
subset. MD5 matches remain compatibility evidence even when addressed internally
by SHA-256. These transports hold operation-local capabilities, not global writers.
