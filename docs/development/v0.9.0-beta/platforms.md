# Platform constraints

The browser adapter exposes guarantees supported by its actual APIs. Feature
availability is probed at runtime and established by release acceptance; this
reference does not certify an empack browser implementation.

## Storage and ownership

OPFS synchronous file access is available in dedicated workers and provides local
random access for admitted codec steps. It does not expose user-visible paths.
See [FileSystemSyncAccessHandle](https://developer.mozilla.org/en-US/docs/Web/API/FileSystemSyncAccessHandle).

IndexedDB remains a maintained database API. Its transactions become inactive
outside their request lifecycle, so network and OPFS work precede the metadata
transaction. Explicit saves request strict durability where available and report
the actual transaction mode; this remains a browser durability hint rather than
a device-loss guarantee. See the [transaction lifecycle and durability contract](https://www.w3.org/TR/IndexedDB/#transaction).

Web Locks coordinate same-origin owners, including workers; expected-head
comparison still occurs inside the metadata transaction. Notifications do not
establish ownership. A busy project writer permits read-only project access only
when shared storage retention and reader coordination are available. If storage
coordination cannot be established, managed-content access is unavailable; disabling
edits does not protect readers from collection. A separately safe metadata-only
inspection may report diagnostics but cannot issue content leases or verify exports.
See [Web Locks](https://developer.mozilla.org/en-US/docs/Web/API/Web_Locks_API).

A queued exclusive request can prevent a later shared request from being granted.
Nested shared acquisition is therefore unsafe for an owner waiting on its own
child work. `ifAvailable` requests return without queueing when ownership cannot
be granted. See the [Web Locks grantability and queue rules](https://w3c.github.io/web-locks/).

The default OPFS sync access mode is exclusive. Shared read-only modes require
feature and behavior tests; `readwrite-unsafe` permits concurrent mutation and is
not an immutable-store solution. See [sync access handle modes](https://developer.mozilla.org/en-US/docs/Web/API/FileSystemFileHandle/createSyncAccessHandle).

Persistent-storage requests may be denied. Browser storage estimates are not
reservations, and deleting site data removes local work. SDK users need a portable
backup path even after successful local saves. See [persistent storage requests](https://developer.mozilla.org/en-US/docs/Web/API/StorageManager/persist).

## Scheduling and codecs

`wasm_bindgen_futures::spawn_local` schedules local futures through microtasks.
It does not make a long synchronous compression call cancellable. A native
`LocalSet` can host the same local orchestration while native owned jobs run on
workers. See [spawn_local](https://docs.rs/wasm-bindgen-futures/latest/wasm_bindgen_futures/fn.spawn_local.html)
and [LocalSet](https://docs.rs/tokio/latest/tokio/task/struct.LocalSet.html).

The driver yields between bounded codec/hash steps using `scheduler.yield()` when
available and a tested task-queue fallback otherwise. Awaiting an immediately
resolved promise is insufficient. See [scheduler.yield](https://developer.mozilla.org/en-US/docs/Web/API/Scheduler/yield)
and the [HTML event loop](https://html.spec.whatwg.org/multipage/webappapis.html#event-loops).

Synchronous archive readers require real synchronous backing inside a bounded
step. Asynchronous file reads must not be bridged with a busy wait. Whole-archive
entry points, metadata scans and finalization need their own responsiveness tests.
Use incremental shared hashing; [SubtleCrypto.digest](https://developer.mozilla.org/en-US/docs/Web/API/SubtleCrypto/digest)
requires its input in memory.

## Network and delivery

A CORS response exposes only permitted headers. Opaque responses cannot supply
verifiable bytes. Catalogs, redirects and actual file/CDN routes need separate
browser checks; native HTTP success is insufficient. See [Fetch response types](https://developer.mozilla.org/en-US/docs/Web/API/Response/type).

Modrinth documents browser-accessible CORS responses and identifying client headers.
Browser control of `User-Agent` differs from native HTTP. The adapter must establish
its supported identification behavior and conservative rate allowance without
assuming it can copy native headers. See [Modrinth API policy](https://docs.modrinth.com/api/)
and [browser-controlled request headers](https://developer.mozilla.org/en-US/docs/Glossary/Forbidden_request_header).

A save picker requires a user gesture and is not available everywhere. A browser
writer's successful close establishes its supported file-finalization boundary;
a download link establishes only a request. See [save picker](https://developer.mozilla.org/en-US/docs/Web/API/Window/showSaveFilePicker),
[writable file streams](https://developer.mozilla.org/en-US/docs/Web/API/FileSystemFileHandle/createWritable)
and [download semantics](https://developer.mozilla.org/en-US/docs/Web/API/HTMLAnchorElement/download).

## Acceptance and packaging

Playwright browser projects provide useful automated coverage, but its WebKit build
is not branded Safari and its mobile profiles emulate device settings. Actual
Safari and mobile-device evidence remain required. See [Playwright browsers](https://playwright.dev/docs/browsers).

Cargo features are additive. Native effects are separated with crate/target
boundaries and target-specific dependency-tree checks, not contradictory global
feature switches. See [Cargo features](https://doc.rust-lang.org/cargo/reference/features.html).
