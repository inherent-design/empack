# Resolution and acquisition ports

Target contract for v0.5.0-alpha.1. Code blocks are design sketches unless the
[implementation ledger](implementation.md) identifies a compiled API.

## 9. Resolution and import ports

### 9.1 Object-safe async conventions

Use static dispatch for pure helpers. Use object-safe traits only at infrastructure seams that benefit from replacement in tests or alternative implementations.

```rust
use std::{future::Future, pin::Pin};

pub type PortFuture<'a, T, E> =
    Pin<Box<dyn Future<Output = Result<T, E>> + Send + 'a>>;
```

A trait intended for `Arc<dyn Trait>` returns this explicit future type. Do not show native `async fn` methods in such a trait and assume they can be used as trait objects. This follows Rust's dyn-compatibility restrictions; concrete `Engine` methods can simply be `async fn`. [R12](https://doc.rust-lang.org/reference/items/traits.html#dyn-compatibility)

### 9.2 Provider catalog

```rust
pub trait ProviderCatalog: Send + Sync {
    fn resolve_selector<'a>(
        &'a self,
        selector: &'a ProjectSelector,
        query: &'a ResolveQuery,
        context: &'a ResolveContext,
    ) -> PortFuture<'a, SelectorResolution, ResolveError>;

    fn resolve_selection<'a>(
        &'a self,
        selection: &'a ProviderSelection,
        context: &'a ResolveContext,
    ) -> PortFuture<'a, ProviderResolution, ResolveError>;

    fn identify_file<'a>(
        &'a self,
        probe: &'a ContentProbe,
        context: &'a ResolveContext,
    ) -> PortFuture<'a, Identification, ResolveError>;
}

pub enum SelectorResolution {
    Exact(CanonicalProject),
    Choices(NonEmpty<ProjectCandidate>),
    NotFound,
}

pub enum Identification {
    Exact(ProviderResolution),
    Ambiguous(NonEmpty<ProviderResolution>),
    Unknown,
}
```

`ResolveQuery` includes game/loader compatibility, content kind, explicitly permitted alternative game versions, and provider preference. Search ranking is selection assistance, not canonical identity. Direct IDs bypass search ranking, not validation or ownership checks.

`ProviderResolution` returns canonical identity, exact files, requirements and dependency evidence, source metadata, and explicit capability limitations. Authentication failure, throttling, provider failure, and genuine not-found are separate errors. None silently becomes an unidentified local file unless the user's policy explicitly permits that fallback and the diagnostic retains why it happened.

The compiled search adapter uses [Modrinth search facets](https://docs.modrinth.com/api/operations/searchprojects/)
and [CurseForge's class/game search](https://docs.curseforge.com/rest-api/#search-mods).
Its retained windows expose pagination and incomplete coverage; they are not resolved
selections. See the [search implementation boundary](implementation.md#provider-search).

The current concrete catalog is listed in the [implementation ledger](implementation.md).
Its exact-selection adapters follow the [Modrinth version contract](https://docs.modrinth.com/api/operations/getversion/)
and [CurseForge file contract](https://docs.curseforge.com/rest-api/).
They retain all declared files and relations before any placement or optionality
conversion. A provider's environment metadata describes support; it does not
override the user's required/optional intent.

The compiled catalog supports content identification through
[Modrinth's hash endpoint](https://docs.modrinth.com/api/operations/versionfromhash/)
and [CurseForge's game-specific fingerprint lookup](https://docs.curseforge.com/rest-api/#get-fingerprints-matches-by-game-id).
A fingerprint result still requires declared digest and size checks. See the
[implementation ledger](implementation.md#content-identification) for the concrete
API and its current boundaries.

### 9.3 Transport is not exposed to command handlers

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

### 9.4 Import adapters produce data, not project changes

The compiled inspection boundary is documented in [the implementation ledger](implementation.md#normalized-import-inspection).
Wire semantics follow the [mrpack specification](https://support.modrinth.com/en/articles/8802351-modrinth-modpack-format-mrpack)
and the [CurseForge export structure](https://support.curseforge.com/support/solutions/articles/9000198500-exporting-a-modpack-for-curseforge-project-submission).
The interface below describes the target composition; it is not a claim that the
full prepare/import/publication operation is already exposed.


```rust
pub trait ImportAdapter: Send + Sync {
    fn format(&self) -> ImportFormat;

    fn inspect(
        &self,
        source: &mut dyn ArchiveRead,
        limits: &ImportLimits,
    ) -> Result<ImportedProject, ImportError>;
}

pub struct ImportedProject {
    pub intent: ImportedIntent,
    pub content: Vec<ImportedContent>,
    pub dependencies: DependencyEvidence,
    pub diagnostics: Vec<Diagnostic>,
    pub source: ImportProvenance,
}
```

Run synchronous archive parsing in a bounded blocking worker over a file-backed quarantined source. Parsing receives no live project root, backend runner, or publisher.

Preserve format record locations in diagnostics: file path, manifest JSON pointer or line, and imported destination. Datapack-folder inference is a pure helper returning evidence and a proposed layout choice; it does not immediately modify backend options.

Import format detection may recognize more formats than it supports. The baseline documents mrpack and CurseForge ZIP support, and separately notes that packwiz-directory import is detected but not implemented. Preserve that explicit distinction; do not advertise a recognized extension as completed import support. [E5](https://github.com/inherent-design/empack/blob/50c121f/docs/specs/import-pipeline.md)

`prepare_import` resolves provider records, checks representability, validates all destinations/layers, verifies required external or embedded bytes, and builds candidate intent before replacement is authorized. Required unresolved/restricted content returns `NeedsInput` rather than deleting the previous project.


## 10. Acquisition, content storage, and archive handling

### 10.1 Acquisition API

```rust
pub trait Acquisition: Send + Sync {
    fn acquire<'a>(
        &'a self,
        request: &'a AcquisitionRequest,
        context: &'a AcquisitionContext,
    ) -> PortFuture<'a, AcquisitionResult, AcquisitionError>;
}

pub struct AcquisitionRequest {
    pub source: AcquisitionSpec,
    pub expected: ExpectedContent,
    pub limits: TransferLimits,
    pub cache: CacheAccess,
}

pub enum AcquisitionResult {
    Ready(AcquiredContent),
    NeedsManualInput(PendingAcquisition),
}

pub struct AcquiredContent {
    lease: ContentLease,
    evidence: IntegrityEvidence,
    observed_size: u64,
    provenance: AcquisitionProvenance,
}

pub enum CacheAccess {
    ReadOnly,             // No index/LRU mutation on a cache hit
    ReadWrite,
    Disabled,
}
```

The acquisition layer streams to an owned quarantine file, checks status and origin policy, applies `Content-Length` as an early rejection only, and counts actual received bytes. It hashes incrementally and verifies expected size/digests before returning `AcquiredContent`.

An HTML error page with a successful status is not accepted as a JAR solely because of its filename. Content identification and archive parsing remain explicit validation steps. Do not overwrite declared source hashes with hashes of the response.

`CacheAccess` in a request is an upper-bound preference, not authority. `AcquisitionContext` privately contains either `Arc<dyn ContentLookup>`, `Arc<dyn ContentStore>`, or no cache capability, selected by the engine at the public entry point. Helpers cannot upgrade that context. `VerifiedAcquisition` itself stores no global cache writer and receives only a scratch-file factory, never a live project root factory. A write request under read-only authority returns `CapabilityDenied`.

Cache insertion is permitted only after verification and only with `ReadWrite` authority. A preview can use retained cache bytes through `ReadOnly` but cannot update access timestamps, write an index, evict objects, or create a durable continuation record.

### 10.2 Content leases and cache ownership

```rust
pub trait ContentRead: std::io::Read + std::io::Seek {}
impl<T: std::io::Read + std::io::Seek> ContentRead for T {}

pub struct ContentLease { /* private retained read handle + optional store pin */ }

impl ContentLease {
    pub fn id(&self) -> ContentId;
    pub fn len(&self) -> u64;
    pub fn open(&self) -> Result<Box<dyn ContentRead + Send>, ContentError>;
}

pub trait ContentLookup: Send + Sync {
    fn retain(&self, id: ContentId) -> Result<Option<ContentLease>, ContentError>;
}

pub trait ContentStore: ContentLookup {
    fn publish_verified(&self, file: VerifiedQuarantine)
        -> Result<ContentLease, ContentError>;
    fn plan_eviction(&self, policy: EvictionPolicy)
        -> Result<CacheEvictionSelection, ContentError>;
}
```

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

### 10.3 Archive interface and limits

```rust
pub trait ArchiveRead {
    fn entries(&mut self) -> Result<Vec<ArchiveEntry>, ArchiveError>;
    fn open_entry(&mut self, id: ArchiveEntryId)
        -> Result<Box<dyn std::io::Read + '_>, ArchiveError>;
}

pub struct ImportLimits {
    pub compressed_bytes: u64,
    pub entry_count: usize,
    pub manifest_bytes: u64,
    pub per_entry_expanded_bytes: u64,
    pub total_expanded_bytes: u64,
}
```

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
