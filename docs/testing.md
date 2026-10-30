# Verification

The [acceptance contracts](design/verification.md) define the current release target.
Tests verify the checked-out implementation; a contract or test count is not evidence
that an unwired capability works. Record run-specific results in CI and PRs.

The v0.8.4 overhaul tracks the complete requirement denominator and compiled
discovery in the [verification registry](../tests/verification/README.md). Its
unbound requirements remain incomplete; structural validation is not a pass rate.

## Local checks

```sh
mise run check
mise run clippy
mise run test
mise run smoke
```

`mise run test` runs Python script contracts, the fixed deterministic workspace
suite (including executable tests), and doctests. `scripts/verify.py` is the shared
local/CI runner. Rust suites build the ordinary product package in a separate target
directory before compiling the workspace test graph. The `python` suite executes
script contracts independently, without selecting an empack binary. The `doctest`
suite runs the pinned rustdoc in both workspace feature graphs and records whether
each example ran, only compiled, or established an expected compilation failure.
Native CI requires these records on each host; a listing or a successful build does
not replace them. Use Nextest 0.9.148 for the executable and library selections.
Also check library tests with minimal features so workspace feature unification
cannot hide test-only dependencies. Architecture checks keep `empack-core` free of I/O/runtime dependencies.

```sh
cargo nextest run -p empack-lib --lib --no-default-features
```

The separated executable suites have explicit boundaries:

| Test binary | Evidence |
| --- | --- |
| `cli_arguments`, `cli_dependencies`, `cli_init` | Offline parsing, refusal and initialization |
| `cli_build` | Offline artifact inventory, profile selection, native snapshot installation and cleanup |
| `journey_import` | Pinned Modrinth/CurseForge archives, complete exported inventory and traversal refusal |
| `journey_authoring` | Literal layered authoring, local JAR update/adoption, rebuild and removal |
| `journey_pending` | Public build/sync inspection, verified manual association, retained partial inputs, restart, stale refusal and discard |
| `journey_pending_import` | Public import selectors, retained-archive restart, exact exported bytes and changed-destination refusal |
| `journey_pending_instance` | Public instance keys, partial retention, executable restart, stale installation and exact discard |
| `journey_publisher` | Local signed-channel enrollment, update conflicts, repair, rollback and trust changes |
| `journey_local_exports` | Offline resourcepack export and reference Prism preparation |
| `live_cli_init` | Official loader catalog resolution |
| `live_provider_cli` | Provider identity, pinned direct-URL acquisition and convergence |
| `live_server_cli` | Server acquisition/export and instance preparation; no game startup |

Live binaries contain ignored tests so a default run cannot silently claim their
coverage. Select them explicitly with strict prerequisites:

```sh
cargo nextest run -p empack-tests --test cli_arguments --test cli_dependencies --test cli_init --test cli_build --test journey_authoring --test journey_local_exports
EMPACK_E2E_STRICT=1 cargo nextest run -p empack-tests --test live_cli_init --test live_provider_cli --test live_server_cli --run-ignored ignored-only --test-threads 2
```

The local JAR authoring journey uses a source path with spaces and a distinct bound
installation path. Changing the source first proves ordinary sync refusal, then
explicit update installs the new bytes while retaining authored intent. Adoption
refuses mismatched source/destination bytes; matching changed bytes update only the
lock observation. Repeat synchronization and independently inspected Prism/mrpack
archives establish convergence and fresh output without a version bump. Removal
preserves source files, neighboring author work and empty directories, rejects an
occupied destination directory, and on Unix refuses linked ancestors leading outside
the project. These are executable fixtures; consumer startup and final-package
qualification require separate evidence.

The multi-file provider host journey connects explicit primary/companion roles through
add, synchronization, a renamed primary in an updated version, and adoption. It checks
complete native client/server inventories and both optional selections in bundled
Prism output against independent expected bytes. Native release descriptors retain
optional-choice metadata; Prism materializes the choice. Missing companions, missing
choices and unknown choices preserve author inputs and prior artifacts. These are
controlled catalog/transport and host-build checks, not public CLI or launcher
execution. Exact filename selectors remain version-sensitive; the `primary` role
is the stable selection when a provider renames its primary file.

The public `journey_content_kinds` scenario shares only literal archive inputs and
expected destinations with the host kind matrix. It drives the selected executable
from a written schema-5 project, refusing ambiguous ZIP inputs and malformed worlds
before adding mods, resource packs, shaders, datapacks and one wrapped world. Complete
native client/server and Prism file inventories check side selection, optional choices
and world-group metadata. Removing optional source/installed bytes and evicting the
cache proves that unselected content is unnecessary while selected bytes remain
mandatory. Host state lives outside the project so project snapshots include all
author files and artifacts without confusing journal retention with author changes.
Separate instance tests prove that initialized worlds retain player edits and missing
members across update, repair and rollback. None of these fixtures claims game startup.

The resourcepack scenario shares its assertions between offline client exports and
live server preparation. Missing Java cannot skip the offline client case. Component
fixtures created through codecs are distinct from the literal authoring journey;
neither proves that Minecraft or Prism consumed the resulting pack. `journey_import`
and `journey_verified_import` cover controlled local import paths. `cli_interactive`
uses real PTY/ConPTY sessions; `live_manual_assistance` adds an official provider
lookup with a fixture desktop opener. The pending-build journey starts with a component fixture, then obtains operation IDs
and association keys only from `pending list/show --json`. Fresh executable invocations
reject wrong bytes, resume renamed verified content and refuse stale drafts. All four
pending families now exercise fresh terminal approval: build, import publication,
instance continuation and synchronization acquisition. Decline preserves author/player
files, prior artifacts, supplied downloads, cache and saved work after the executable
exits. The shared external harness owns PTY/ConPTY handling and offline command setup;
no terminal controls are added to the product. The terminal helper also verifies
successful process exit: expected prompt text and EOF cannot hide a later failure. State snapshots assert preservation without interpreting
private continuation records. The same decision fixture now checks closed input and
piped affirmative/negative answers, each refused without `--yes`, plus a read-only
`--yes --dry-run` before terminal decline. Local initialization, addition and build
also prove that explicit approval reaches independently checked publication.
Forced initialization/import retain author attributes and player-world sentinels.
Shared fixture observations include bytes, empty directories and Unix rwx mode bits
or the Windows read-only attribute;
timestamps are excluded and links/special entries are not admitted by this observer.
For zero-byte files, the observer records empty content from metadata without an
EOF read, which Windows mandatory locks can reject. Nonempty content still requires
a successful read. A Windows test holds a real exclusive lock and checks both
the empty observation and refusal once that locked file contains data.
Native recovery has a separate headless-host refusal check. These checks do not
qualify terminal EOF/cancellation, actual consumers or packaged execution.
Windows recovery-shell fixtures initialize a fresh PowerShell native-output
pipeline with an OS command before measuring the printed empack commands. Setup
may create only the three known empty profile directories inside the fixture;
project/state/cache changes or new files fail. The version and recovery operations
then retain their full no-write snapshots. This setup is local to shell fixtures;
ordinary executable help/inspection tests still begin with an absent home.

Requirements inspection runs with an empty controlled `PATH`, then with synthetic
Java tools advertising old, current or malformed versions if executed. It must
discover eligible paths without executing them or creating state, and must reject
directories and Unix files without executable permissions as Java candidates.
The output distinguishes path discovery from selected-runtime compatibility;
approval and actual Java validation remain separate launch tests.

Offline prelaunch has distinct unit, host and executable evidence. The typed
classifier admits only network/deadline failures; the host tests actual HTTP error
responses, bounded/encoded metadata and redirect policy. Signed-channel fixtures
also reject unknown keys, corrupt signatures, expiry, replay and invalid selected
releases while retaining the completed installation and durable sequence floor.
The CLI fixture uses ordinary proxy configuration to observe and refuse a real
HTTPS CONNECT. It requires explicit `--allow-offline`, observes the selected local
runtime, and refuses damaged content, a changed descriptor or revoked trust.
The fallback warning uses the normal human status stream; command failures remain
on stderr. These loopback/proxy checks do not qualify a live external publisher.

Synchronization also checks partial retention across
cache eviction. Import resumes a retained archive after deleting the original and
keeps incomplete requirement discovery explicit. Its unsupported-transport fixture
does not establish a live provider's restricted-download policy. Instance journeys
use a literal two-file manual release, retain a verified input
through a partial resume, remove the original download and descriptor, evict ordinary
cache and exercise retained cleanup, then complete from public keys through both
public and legacy continuation commands. They compare the complete game inventory and preserve player notes. A separate real installation
stales the saved operation; both discard interfaces preserve that installation and
remove only the selected record. Retained-content fault journeys locate a known native
backing by its fixture bytes, without decoding a private record. Corruption fails
verification; missing build, sync or instance content becomes a fresh obligation
without partial publication. Missing import-archive content prevents restoration.
Recovery compares the complete output inventory, and build/sync faults preserve a
second project's saved work in the same host store. A two-input import exercises
manual-payload faults separately from archive faults through both `pending resume`
and `init --continue`: retained rows remain visible without claiming complete input
discovery, missing bytes become fresh obligations, and the completed export preserves
both exact payloads and author overrides. Synchronization also completes its partial
retention journey through both public resume and `sync --continue`. Component/native
cases separately bind saved plans, changed observations, fresh grants and retired
resource reservations. A shared record fault fixture corrupts actual saved records from
all four families without interpreting their private payloads. Malformed JSON and
unsupported schemas remain visible as unavailable; text and JSON inspection preserve
storage, resume refuses them, and explicit discard removes only the selected record.
Pure selector tests cover ambiguous prefixes separately from native stale-cleanup
ownership. A controlled provider-import fixture gives identically named files distinct
bytes and original digests: exact role selectors are required, other-role bytes fail
verification, and successful associations retain per-role bytes and source evidence.
This host fixture does not substitute for live-provider or final-package execution.
Manual-association and fresh-approval journeys have explicit final-package cases on
all five release targets. Passing them with a development executable leaves those
package obligations unexecuted. Moving upstream pack inputs belong to `live_import_survey`, not the pinned
regression corpus.

Configured build-profile checks start from literal author text and populated local
content. The actual executable must preserve subscription policy and TAR.GZ format
through default, named and unique-consumer selection. Explicit overrides and ad-hoc
reference builds must agree with explanation about output paths, field origins and
recipient requirements. Independent ZIP/TAR readers compare complete inventories,
payload digests, runtime declarations and exact generated commands. The generated
INI comparison accepts LF and CRLF checkout line endings without trimming settings
or changing asset comparisons; this does not establish Prism UI acceptance.
Ambiguous or colliding selections preserve prior
artifacts, while multiple configured formats publish together. Missing/stale locks
remain inspectable without effects and cannot authorize a build. The external TAR
reader checks bounded members and consumes the gzip trailer; its dependencies belong
to the test harness, not the distributed product.

The provider-reference export journey starts from a captured component record,
not a live catalog. The same real CLI exports complete Modrinth and CurseForge
manifests with exact identities, digest/size assertions and side-specific overrides.
Preserved optional metadata needs explicit loss consent. CurseForge selections
become required or disappear; reference-only materialization switches fail before
replacing artifacts, including during explanation. A mixed Modrinth/Prism explanation
reports the different effective policies. Renamed provider filenames are rejected by
CurseForge and remain explicit mrpack destinations. These checks do not qualify
provider responses, hosting eligibility or actual consumer imports.

The template/archive case reuses that literal profile fixture for reference and
standalone bundled Prism outputs in ZIP, TAR.GZ and 7z. Independent readers compare
complete member sets, bytes, lengths, digests and modes across formats. Binary
inputs remain literal; client templates override common inputs and exclude server
inputs. Newlines, Unicode, quotes and backslashes pass through explicit shell, INI
and properties encoders. On Unix, one archived shell script executes directly with
its recorded mode and returns the original value. Missing variables, invalid text
and duplicate destinations preserve author inputs and all prior outputs. Container
parity does not establish that the launcher imports every format.

The multi-output fixture changes configuration and template bytes without changing
its version label. Independent archive inventories must show those fresh bytes.
A later template failure preserves both prior archives, including with `--clean`;
a successful cleanup removes obsolete output files while retaining author inputs
and directories. Native batch tests separately inject publication interruption and
verify finish/restore outcomes. Those controlled faults do not establish actual
process-termination recovery.

Missing-content checks distinguish unresolved obligations from legitimate reference
exports. Controlled URL/provider failures and deliberately omitted retained slots
cannot publish an incomplete candidate. Removing a captured materialized input or
embedded archive after preparation preserves prior output. Each archive encoder
also rejects an incomplete stage against a separately declared inventory. Public
fixtures use cold storage for missing local sources and resume corrupt/missing
retained inputs; independent readers check the complete successful archive. An
excluded optional source has no byte obligation, while a selected reference is not
proof that bundled bytes exist.

Native snapshot fixtures export a literal project with common/client/server layers
and a default-disabled choice through both delivery policies and ZIP/TAR.GZ/7z.
Independent readers compare complete membership, content digests and output modes.
The public instance command rejects a wrong descriptor digest and corrupted included
bytes before installing the exact selected inventory. Repeated preparation preserves
saved choices, player-written seed data and unrelated files. Game client/server and
Prism client placements are checked separately. Prism placement preserves the
existing component metadata and launcher settings; invalid author text is not
interpreted as intent for an instance operation. First installation refuses existing
selected files without an explicit replacement decision, even if their bytes match
and `--yes` was supplied. An unowned layout marker cannot establish ownership, and
Prism refuses server selection or a competing `minecraft/` directory. These refusal
checks preserve target bytes and attributes. Missing pack identity
or Java declarations preserve prior output and host storage. These local sources
require included assets for both policies; controlled producer tests separately
verify URL references, bundled assets and original source assertions. Final-package
cases repeat the public journey against each extracted native executable.

The choice-change journey uses a separate recipient cache. Read-only option
inspection preserves instance and host storage. An uncached optional asset becomes
pending without changing the saved choice; the public pending key accepts exact
renamed bytes and rejects wrong content. Disabling an unchanged managed optional
file retires it. Modified content keeps the old selection until an explicit conflict
decision is supplied; preservation leaves that content and unrelated player data
intact. Choice-key changes across releases and actual consumers remain separate
acceptance work for the public signed-update path. Engine fixtures separately check
changed defaults, newly required choices, and removal of retired keys.

Reference Prism fixtures independently hash the exported descriptor and assets,
then prepare an extracted instance through the real executable. The hash comes from
the known emitted prelaunch setting; altered descriptors and equal-length corrupted
assets must fail without changing instance files. Successful preparation installs the
complete selected inventory, preserves a world sentinel and locally edited seed
configuration, and converges on repetition. Rebuilding after an author edit changes
the release identity even when the version label stays the same. The bundled journey
separately checks client-only projection, same-version rebuilds and empty managed
command hooks. These fixtures do not interpret arbitrary Qt syntax or execute Prism;
actual import, preparation and wrapper execution remain consumer obligations.

Server-export cases distinguish artifact inspection, native script execution and
official runtime probes. A literal project builds reference and bundled ZIP,
TAR.GZ and 7z against the [pinned vanilla inventory](../tests/fixtures/runtime/README.md).
The streaming reader compares complete member sets, digests, sizes and output modes,
including server-only content, source assertions and generated preparation settings.
Large runtime files are hashed without retaining their bytes in memory. Fabric ZIP
and 7z checks remain separate from the pinned vanilla matrix. Real shell and cmd.exe
fixtures check argument boundaries, preparation ordering and immediate-child status;
their controlled programs do not establish production empack or Java behavior.
Official launcher probes reach Minecraft help or the historical EULA refusal. Actual
interactive server-console and final-package acceptance remain outstanding gates.

Download assistance separates pure provider-page interpretation, native file scans,
host preparation, real terminal decisions and live metadata lookup. Scans verify
content assertions rather than accepting a unique filename. The wait fixtures reject
a sole wrong same-length file, preserve downloads and compare the complete resulting
archive against an independent inventory shared with the pending-build journey.
The controlled desktop launcher checks arguments, timeout, failure and retirement
following cancellation; it does not launch or supervise a real browser. The live
Modrinth case verifies page selection but still uses fixture payload bytes. Windows
opener behavior and remaining page-limit/failure cases stay explicit gaps in UX-037.

Retained cleanup tests distinguish record ownership, category retention and partial
reclamation. Active saves, replacement records and changed backing produce specific
refusals; later-store failures retain an exact account of completed scopes and surviving
bytes. A fresh capture can finish cleanup and ingest the same content again while
preserving downloads and recovery preimages. Native eviction fault tests cover failure
and cancellation within a store. Separate host tests cancel only after an admitted
file deletion: the service must retire and return its actual result, including partial
completion context or completed success. Precancellation cannot start a service.
These component tests do not substitute for the broader process-crash recovery matrix.

Public cleanup checks build a real artifact, then exercise default, builds, cache
and combined selections through preview, missing approval, invalid targets,
application and repetition. They preserve invalid author text, templates, player
files, another project's recovery evidence and unknown cache neighbors. A new
artifact publication may replace the selected project's completed journal; cache
cleanup alone leaves its entire prior state unchanged. Cache storage is initialized
by component setup, so these checks do not establish acquisition behavior.

Empty cache, continuation, import, sync and retained cleanup also run without author
documents and must create no host or project entries. Native tests separately cover
stale selections, retained readers, links, interrupted artifact finish/restore and
partial eviction followed by re-ingestion. Populated pending-record CLI cleanup,
permission failures and remaining interruption/platform variants are still distinct
UX-079 obligations.

Remote-acquisition fixtures count bytes across failed mirrors, exercise stalled
headers and bodies, and retain author files through refused replacement. A shared
native HTTP fixture signals when a request actually arrives before triggering
cancellation or caller abandonment. It distinguishes EOF/reset from a socket timeout;
a timeout cannot stand in for connection retirement. Tests also inspect the operation
outcome so a worker panic cannot pass as successful cleanup. Credential fixtures
use the existing private origin mapping to exercise authenticated same-origin and
cross-origin redirects. These are controlled wire checks, not live TLS/provider or
public-executable HTTPS qualification.

The [archive header corpus](../tests/fixtures/archives/README.md) supplies literal
ordinary ZIP and ZIP64 bytes to source inspection, opaque inspection, and artifact
verification. It covers malformed record arithmetic and metadata admission in
addition to the existing member-name, CRC, and extraction-budget tests. Boundary
cases retain valid ZIP64 extension data at the combined metadata limit and reject
the next byte. The fixture driver uses no production archive encoder to construct
its expected inventory. Native captured-input faults and future browser execution
remain distinct evidence obligations.

The mixed CurseForge host fixture uses controlled provider records with no download
URL plus embedded author overrides. It resumes after the original archive is gone,
rejects mismatched supplied bytes, and retains exact provider identity separately
from local override ownership. Converting the provider file into a local or URL
source is refused before publication, even with valid supplied bytes. Re-export
checks the complete literal manifest and override inventory: supplied restricted bytes remain provider references. Optional
metadata loss requires a separate export acknowledgement, and refusal preserves
prior artifacts. This is controlled host evidence; the public pinned archive fixture
and actual credentialed provider checks establish different parts of the workflow.

Import-decision journeys refuse auxiliary-file exclusion without explicit consent
and refuse a requested runtime that contradicts the archive. They then import and
re-export the retained content against a literal inventory. Missing optional-default
and metadata-loss decisions have separate diagnostic assertions: refusal preserves
author documents, supplied content, the original archive and prior artifacts.
Controlled host tests cover provider-kind routing and world archive interpretation;
they do not substitute for executable evidence of every routing combination.

The populated migration host journey starts from literal authoring and bindings,
materializes controlled provider JARs, and updates an automatic root alongside an
exact pin. It captures the displayed JSON plan before migrating, then compares it
with application and independently expected client/server native archive contents.
A primary file role survives provider filename changes. A retained library must be
compatible with the new runtime; local sources keep dependency uncertainty explicit.
Preview, refusal and repeated synchronization check source preservation. These are
in-process host checks with controlled services. `live_provider_cli` separately
exercises ordinary CLI pin/update commands against Modrinth and checks the retained
selection report. Neither boundary claims actual consumer startup or complete
migration qualification.

Canonical provider selection has separate syntax, DTO ownership, controlled HTTP,
host-alias and live executable cases. Both controlled catalogs check slug, ID and
project-URL equivalence; CurseForge additionally verifies exact-slug search versus
direct numeric lookup. The shared live journey checks one canonical root after
approved re-adds, no-op synchronization, and preservation on refused replacements
and failed lookup. It snapshots author files without reading disposable caches or
recovery records. Commands have a 120-second deadline.

Search has separate host, executable and terminal evidence. Controlled catalog
responses distinguish an empty result from authentication, not-found, throttling
and server errors. Headless queries refuse before an observed proxy request and
preserve the project, lock and prior artifacts; this is not a network sandbox.
The live terminal journey filters displayed Modrinth labels by canonical project
identity, cancels the search, declines publication, then explicitly approves the
selected root. Scratch files remain separate from author files, and selection
alone does not authorize publication. Each terminal expectation is bounded at
120 seconds. Host pagination tests do not substitute for public terminal paging;
that path and credentialed CurseForge search remain separate qualification work.

Direct URL integrity has distinct parser, host, executable and live evidence.
The host journey covers compact/expanded declarations with authored assertions or
first-observation consent. It commits the lock, checks repeat-sync convergence,
then removes retained content and serves changed bytes at the same URL; publication
must fail without changing the lock or prior artifacts. Its private loopback adapter
does not establish production TLS behavior. The ordinary executable separately
acquires the immutable 3,573-byte More Chat History JAR from the pinned import corpus,
checks its recorded SHA-512/size and retained observation, and verifies preview and
repeat-sync preservation. That exercises production HTTPS without modifying trust
configuration or adding a product test endpoint.

`python3 scripts/verify.py live-provider-cli` selects the Modrinth and direct-URL
executable cases.
`python3 scripts/verify.py live-curseforge` also includes the CurseForge executable
journey and requires explicit `EMPACK_KEY_CURSEFORGE` before discovery/execution.
An absent credential is blocked evidence, not an empty passing selection. These
tests use current compatible provider selections; the independently hashed real-pack
corpus below retains a separate immutable-input contract. Public alias editing and
the remaining provider permutations still require their own acceptance evidence.

The offline `journey_selection` CLI fixture starts from literal authoring and bindings
plus a fixed provider lock assembled as component setup. Pin/unpin preserve the
selected inventory, custom placement, comments, logical alias and unrelated YAML
anchor/alias. Invalid grouped requests and previews preserve every project file;
two syncs after each edit retain the same content. Pinning removes the root release
preference; unpinning keeps the selected version and returns to the project preference.
The fixture does not prove live provider ownership: a separate controlled provider
case rejects a version belonging to another project. The native publication test
also checks stale-input refusal and preservation of locally modified payloads.

The populated synchronization matrix in `cli_manifest` distinguishes current, absent,
stale, malformed and unsupported locks under both `sync` modes. It verifies exact
restoration and repeated convergence, preserves unrelated author work, and refuses
implicit adoption even when untracked bytes match the source. A missing source and
installed copy is tested after cache eviction. Local hashes remain observations,
not declared provider evidence. The shared model fixture now uses literal payload
digests instead of creating a native staging lease; its pure scope tests cover
retained selections and required closure. Native fixtures separately cover reference
preservation, publication races and foreign metadata. Final-package execution of
the public matrix is a separate five-target obligation, not established by local tests.

Selected-update tests connect frozen provider records to the host preview, completion
receipt and materialized inventory. Updating an automatic root beside an exact pin
must report one changed selection and one retained selection, preserve unselected
records and authoring, and retain explicit file-role destinations. Pure candidate
cases cover reverse and exact-version incompatibilities, side separation, grouped
updates and preservation of negative evidence when the selected version is unchanged.
The host refusal matrix checks the whole populated project after both dry-run and
approved requests; no conflicting retained dependency is silently removed.

The live CLI sentinel checks an exact pin, unpins without upgrading, then performs
an automatic update. It compares the actual change count with the resulting pin and
independently hashes materialized bytes. A live catalog may already be current; that
run does not replace the frozen fixture proving a changed selection. Broader public
release-policy permutations and other-platform execution remain separate obligations.

Assessment fixtures compare complete report fields with independent expectations,
including exact pins and the distinction between permitted, compatible and upstream
versions. Controlled catalogs exercise malformed records, shared transfer/deadline
limits, retained incompatibilities and incomplete pagination. The public executable
checks populated manual and unavailable-provider rows in text and JSON, preserving
all author files and printing the report before strict-mode failure. A bounded local
proxy refuses CONNECT for this unavailable-provider case; it supplies no provider
records and avoids racing an OS-dependent connection-refusal delay against the
catalog deadline. A manual-only
root set stays complete even when the lock retains a forgotten provider dependency;
that does not establish compatibility of the retained inventory. The live pinned
journey checks a successful populated strict assessment through actual Modrinth
routes. Live observations do not replace the controlled failure and policy cases.

Runtime migration has a populated live executable journey alongside frozen host
cases. Real catalog selection changes Minecraft 1.21.1 to 1.21.2 while retaining an
exact pin, custom placements and local configuration. Plan JSON is compared with
preview/application text and the applied inventory; the public CLI intentionally
requires `--plan` for JSON. Client/server native outputs and regenerated Prism
components are inspected independently. A stale authored Prism profile must refuse
output while retaining author files and prior artifacts. Deliberate provider
replacement resets only the selected root; a redundant canonical replacement must
not erase an exact pin, even when authoring uses a selector. Offline executable
refusals and native publication tests cover invalid arguments, stale captures and
unrelated edits. These are catalog, content and configuration checks, not launcher
startup or gameplay evidence.

Independent-batch fixtures distinguish default all-or-none execution from explicit
partial completion. The public CLI test verifies named ready/blocked groups, nonzero
partial results, preserved files, a real changed-content update and truthful retry
counts. Malformed source interpretation still fails before independent groups are
prepared and preserves the whole request. Pure planners cover coupled requests; native
API tests cover coherent partial publication, stale approval and resource retirement. None of these checks permits
an unrelated failed request to disappear from the result.

The authoring journey accepts changed local content only after both source and installed
copies agree. Preview and refusal preserve all authoring and previous artifacts;
approved adoption changes the lock observation while retaining payload modification
times and attributes. Its SHA-256 and size checks distinguish accepted local bytes
from provider-declared evidence. First-lock reconstruction is tested separately: it
retains identity and byte evidence, but cannot retain an inferred display title that
was present only in the missing lock. Controlled provider cases check exact byte
identification, incompatible identities, copied placements and absence of latest-version
queries. A handler matrix refuses unknown identity, competing provider matches and
multiple matching file roles without edits or a completion receipt. The live CLI
reuses the pinned JAR corpus for URL, exact-version and byte-identified adoption,
checking absent/invalid-target refusal, provider-byte mismatches, payload attributes,
declared versus observed identity and repeated convergence. These fixtures still require execution on every
required host and product configuration; their presence is not release qualification.

Removal fixtures distinguish forgetting a root from deleting its selected content.
The offline CLI checks canonical identity under an author alias, grouped refusal,
reserved `--deps`, incomplete retained evidence and known dependents. Forgetting
changes documents while retaining the exact selection; acknowledging uncertainty
never bypasses a known requirement. Full file snapshots retain sources, worlds,
unowned directories and previous artifacts. A live provider journey adds an authored
alias, re-adds by canonical identity and removes through that identity after repeated
synchronization. Pure selection tests, native publication/confinement tests and
operation-authority tests supply separate evidence; live success does not replace
native conflict or stale-observation checks.

The publisher journey builds literal native projects, stages them using a disposable
private key, and checks the signed payload independently with Ed25519. Fresh CLI
processes enroll, observe local channel envelopes, install, update, repair, roll back,
and change trust. Assertions cover refused tampering and expiry, explicit conflict
resolution, replay floors, and preservation of player configuration and initialized
worlds. A compiler-bound local runtime observes literal arguments and the game
working directory while signed update, repair, duplicate launch and stopped-runtime
acknowledgement are refused. Refused publication can retain one exact verified
incoming cache blob; installation, ownership evidence and existing cache content
remain unchanged. Normal retirement permits the authenticated update to proceed.
Explicit preserve and merge decisions remain identifiable as local
overrides through fresh repair and update processes. A later edit needs a new
decision; explicit replacement restores publisher bytes and removes the override.
The signed envelope and publisher payload remain unchanged throughout. A generated
bootstrap descriptor requires enrollment before its first
preparation. After a signed update, repeating that older bootstrap command retains
and repairs the active release instead of downgrading it. Wrong digests and changed
bootstrap bytes are rejected without changing the installed instance. These are
the documented local-envelope routes. They do not execute hosted
HTTPS publication, Java, or Prism, and do not substitute for those acceptance cases.

Runtime/profile checks reuse those signature helpers with literal native inputs.
A public Prism-layout journey stages and enrolls A, observes B, then exercises
preview, refusal, explicit update, repeated old-bootstrap preparation and rollback.
Independent complete game/profile expectations include the layout marker, exact
Minecraft and Fabric components, user metadata, player configuration and a played
world. Publisher inputs, keys and immutable releases remain unchanged. This checks
installed metadata through the executable; it does not start Prism or Minecraft.

Native engine tests separately bind the complete profile and game read set:
a late profile edit must survive a stale-snapshot refusal before new content is
published. Pure transition tests cover all supported loader component identities,
preserved user attributes and unchanged byte reuse. The host prelaunch fixture
checks approved channel observation separately from installation: clients retain
their current release when the runtime changes, and only a supported managed-server
entry point can proceed after separate update approval. Both offline settings are
tested. A unit-only transport adapter supplies real controlled asset acquisition;
it is absent from the ordinary product API. Actual consumer reload/import and
interactive runtime qualification remain separate acceptance work.

Hosted channel publication has separate evidence. Its controlled-host engine test
first establishes a verified warm cache, then requires fresh envelope and asset
requests before replacing the pointer. It checks typed remote failures, invalid
sequence/expiry, denied network approval and a late local edit while preserving
publisher, source and host bytes. The ordinary executable suite stages two literal
release fixtures, verifies their frozen envelopes, and checks preview/refusal with
no observed proxy connection. A passive proxy is not an OS network sandbox.

The opt-in `publisher-cli` suite uses the selected product against an immutable
GitHub HTTPS fixture commit. It checks corrupt/missing hosted inputs, explicit
previous-key verification during rotation, replay and same-sequence metadata
refusals, exact republication and independently verified channel signatures and
fields. Inputs, immutable releases, unrelated notes and prior pointers are compared
across failures. A successful command creates only the local channel file and
instructs uploading it last; it does not deploy that file or start a consumer.

```sh
python3 scripts/verify.py publisher-cli --product-profile release
```

The corpus is original synthetic Apache-2.0 data with public fixture keys and
recorded input/asset/envelope hashes. Git attributes retain exact bytes on every
checkout. The real HTTPS test remains ignored in offline suites and is selected by
the shared runner, relevant trusted CI changes and scheduled compatibility checks.
All declared platform/product variants still need execution; one Linux live job or
an in-process loopback test cannot establish all of them.

The opt-in `native-remount` suite repeats that signed publisher/player journey on
macOS using two disposable 128 MiB APFS sparse images. It forces a device-number
change after enrollment and installation, checks that UUID/inode/creation identity
and fixture bytes survive, then exercises inspection, authenticated update, repair,
rollback and trust floors. It uses `hdiutil`, `diskutil` and `plutil`, with Finder
browsing/auto-open disabled. Fixture cleanup rechecks image ownership before detach;
confirmed absence completes retirement even after an earlier tool failure. A still
attached image is an error, and failed cleanup preserves its temporary directory
for diagnosis. The held-file regression covers device reassignment, retirement
between attempts, repeated detach and bounded refusal while a reader remains open.
No forced detach is used. Those assertions do not establish the cause of a separate
host failure where the image's devices remain attached after unmount.

```sh
python3 scripts/verify.py native-remount --product-features default --test-features minimal
```

The same suite remounts saved import and instance continuations. Public IDs and
host/project bytes remain unchanged; continuation verifies supplied and retained
inputs, preserves player notes and checks independent output inventories. The
import case remounts again after creating the project, then requires valid author
status and convergent sync/export. Native cases run serially to avoid interference
between their device-allocation experiments; the same two images support repeat
remounts without growing the fixture.

Saved-build and synchronization cases cover the other continuation families. A
build may have no cache directory; its absence is preserved just like existing
cache contents. Synchronization resumes retained bytes after the original download
and ordinary cache are removed, then checks exact materialization, convergent sync
and safe retained-content cleanup through both continuation commands. Restricted
provider records are component setup; these cases do not establish live provider
access or runnable mod content.

Creation recovery also crosses real process exit and remount. A private libtest
process exits at four durable index/intent/rename/sync boundaries. After device
reassignment, the ordinary CLI must rediscover the same operation IDs without
writes and finish the exact files while preserving neighbors. Existing-root
publication repeats intent, replacement, directory-sync and verification failures
for both finish and restore, checking candidate versus original inventories and
convergent read-only inspection. The runner supplies
its compiler inventory only to the test process; the test selects and hashes the
library driver from that inventory. These protocol fixtures are deliberately not
valid author documents and do not establish pack or runtime acceptance.

Native PR verification requires the suite in both product/test graphs on macOS.
Other hosts record explicit `NOT_APPLICABLE` evidence; missing permissions on
macOS are a failure, not an applicability exemption. The native suite retains its
successful identity observations in JUnit. This is local signed-envelope and
filesystem evidence, not a physical reboot, hosted publication, Java or GUI test.

The [pinned import corpus](../tests/fixtures/import-corpus/README.md) uses small
repository-owned archives with fixed hashes and separately reviewed expectations.
Run `python3 scripts/check-import-corpus.py` or `mise run test:import:corpus` for
that selection; it also runs in the ordinary CLI and deterministic suites.
`scripts/survey-imports.py` deliberately resolves moving upstream packs. Its reports
identify themselves as rolling observations, record acquired archive and executable
digests, and cannot qualify the pinned corpus. The small synthetic corpus does not
replace real-pack acceptance. The separate `python3 scripts/verify.py pinned-import`
suite acquires the fixed Fabulously Optimized 6.5.0 archive and checks complete
index/override preservation, two-sync convergence, and every downloaded mod's
bytes in a bundled export. Edited imported local dependencies require explicit
adoption; the fixture checks refusal preservation and fresh output without a version
bump after that acceptance. Its fixture contains metadata and hashes, not upstream
assets. This is live executable evidence, not launcher/game acceptance or coverage
of all real-pack formats.

Scheduled compatibility runs on the default branch each Monday. The shared runner
selects the synthetic and real-pack pinned corpora, official provider/catalog probes, and
current runtime checks. During the first seven UTC dates of each month it substitutes the broader
runtime suite, with both Java 8 and 21 installed. Missing credentials or prerequisites
remain blocked evidence. `python3 scripts/check-live-compatibility.py --plan-only`
writes the dated selection without claiming execution; omit that flag to run it.

A separate job runs `scripts/survey-imports.py --profile weekly`, selecting one of
the curated packs per UTC week without backfilling a failed slot. It admits at most
100 MiB per pack archive and 60 seconds per archive transfer, checks the transfer
deadline between bounded socket reads, and retains failed observations. Socket timeouts
bound a blocked read; they do not make Python reads instantaneously cancellable.
Manual fixture downloads have a 256-file/2-GiB total allowance and a 1-GiB per-file
limit. Each empack command has a 600-second deadline. The probe and survey jobs have
90- and 60-minute budgets respectively; no whole-command retry masks a failed mutation.

Survey archive caches bind the exact provider/project/file/URL selection and retain
an observed digest. Reuse verifies both identity and bytes and is labeled
`retained-observation`, not fresh provider execution. A new upstream selection cannot
reuse the previous version's bytes. The jobs upload reports even on failure and have
read-only repository access; manual dispatch on another branch cannot obtain their
credentials. This workflow is scheduled evidence, not a substitute for qualification
of the exact release candidate. Broader pinned real-pack coverage remains outstanding.

The executable harness lives in `empack-tests::harness::process`, outside the
product dependency graph. Local runs may discover a binary and print the selected
path. Qualification requires `EMPACK_QUALIFY=1`, an absolute `EMPACK_E2E_BIN`,
`EMPACK_EXPECT_BINARY_SHA256`, and `EMPACK_EXPECT_VERSION`. Wrong bytes or version
fail before the scenario. This is executable selection, not full package
qualification; target/features and consumer evidence still need the release record.
Resolve and verify the selected product before starting an observer or interaction
deadline. Hashing a large debug executable is harness setup, not time spent waiting
for a running command to reach its prompt.

Children receive a positive environment allowlist and fixture-local home, cache,
state and temporary paths. Tests explicitly add any needed fixture tools or product
settings. Harness controls and ambient credentials are not forwarded. A failed
mutating command is not replayed after matching a timeout diagnostic. Nextest CI
retries are disabled so the original failure remains visible. Live runtime probes
retain bounded, best-effort launcher output on timeout, with at most 64 KiB per
stream. These tails help diagnose the failure; they do not turn a timeout into a
pass or replace complete successful output assertions. Managed runtime probes execute
the generated installer first (300-second bound), require completed ownership and
selected content, then execute the generated start script (90-second bound). Startup
still invokes preparation again, exercising convergence before reaching Minecraft.
Installation failure cannot be counted as startup evidence. These help/EULA probes
are separate from interactive console and gameplay acceptance.

After building the executable, compare the editor schemas with the actual CLI:

```sh
uv run scripts/check-authoring-schema.py target/debug/empack
```

This exercises accepted and rejected authoring forms without creating host state or
cache storage. JSON Schema checks complement composed domain validation; they do not
replace it.

Formatting and Clippy apply to all targets/features. Native filesystem/process
claims require real temporary roots and OS primitives; pure planner tests cannot
prove link handling, locks or crash durability.

### Inherited terminal regression

On macOS and Linux, the normal native suite includes a real controlling-PTY test:

```sh
cargo test -p empack-lib --lib inherited_runtime_owns_terminal_and_restores_it -- --nocapture
```

It supervises an isolated runtime process, checks foreground ownership before any
console reads, sends input and terminal Ctrl-C, and verifies normal shutdown,
cancellation, nonzero exit, failed exec, disabled terminal suspend keys, external
stop/resume, cancellation while stopped, and restoration of terminal ownership and
settings. The fixture changes terminal settings to make restoration observable.
It proves host process/terminal behavior; it is not Minecraft gameplay acceptance.
The Unix executable test
`cli_pty_instance_launch_restores_console_after_input_interrupt_and_failure`
connects public export/install to the selected product's `instance launch` through
a controlling PTY. A fixture runtime reads console input and changes echo settings;
normal shutdown, graceful terminal Ctrl-C, nonzero exit and host cancellation must
restore the invoking shell's settings and allow a follow-up read. The test checks
the final diagnostic/status, retirement of the owned group, preserved player bytes
and a subsequent public launch. It uses the compiler-bound native test runtime with an explicit signal handler,
not Java or generated server entry points; those and Windows console behavior
require separate evidence.
Managed Prism tests separately verify the root icon's seed policy and rejection of
an explicit managed policy before publication, while keeping other PNGs managed.
Engine launch fault injection separately establishes that failed terminal cleanup
after confirmed retirement reports the error without retaining runtime ownership;
uncertain-retirement tests still require recovery. Signed-channel tests use the
native release producer and a persisted Prism layout, reject managed icon updates,
and preserve launcher bytes through corrected seed updates and launch preparation.
Historical managed-icon fixtures verify migration to seed without conflict overrides.

Executable finalization is tested separately from process supervision:

```sh
cargo test -p empack --bin empack
cargo test -p empack-lib --lib launch_failure_summary
```

These checks retain cleanup, recovery, uncertainty and partial-effect diagnostics
when cancellation is set, retain success after a late interrupt, and verify that
launch failure headlines do not imply that the runtime never ran. The executable
fixtures test result presentation; the engine and PTY fixtures establish ownership
cleanup and actual process retirement.

The `managed_runtime_exit_uses_empack_classification` host test runs a fixture child
with exit statuses 0 and 7, then verifies managed CLI classification and the retained
runtime-status diagnostic. It does not launch Java. The Unix
`cli_interrupt_exits_130` test sends SIGINT to the actual empack executable during
a bounded local HTTP request and checks both exit 130 and stderr. Run it against a
freshly built binary to test executable wiring:

```sh
cargo build -p empack --all-features
cargo test -p empack-tests --all-features --test cli_exit_codes cli_interrupt_exits_130
```

Resource-lifetime checks distinguish cancellation from worker retirement. The
runtime's dropped-handle test writes and synchronizes a real four-byte file, then
holds its owner behind a channel. Dropping the caller must cancel the worker token
without releasing reservations, evicting the active operation or publishing a
terminal result. Releasing the owner permits a typed cancelled result and zero
remaining reservations. The bounded registry still requires explicit result
release before admitting another operation. Readiness, retirement and release use
channels and observed phases rather than elapsed sleeps as evidence.

Captured Unix processes have a separate tiny-limit check: each stream may use its
full allowance, while overflow with an already-full progress queue must report the
limit failure after the owned group is gone. Existing tests retain closed-pipe
deadlines, descendant retirement and disconnected progress observers. These
checks bind UX-080 alongside acquisition and API receipt retention; they do not
complete its remaining platform and public-workflow variants. A macOS zombie
fixture holds an exited child unreaped, observes the real group-signal failure,
then reaps it and confirms absence. A signal error cannot override subsequent
proof of retirement. A separate real live group with an injected signal-denial
result must still fail retirement and retain the recovery diagnostic. Chunked HTTP
fixtures check the specific byte-limit and deadline errors, including socket
retirement, rather than accepting any failure. Payload acquisition checks also
retain reservations through the last reader, fit two seven-byte inputs into a
sixteen-byte scratch allowance, and retire stalled verifier/response ownership
when the caller cancels or drops its future. These reuse existing component tests.
Reservations are admission estimates, not measured OS memory use or enforced
filesystem quotas.
Inherited interactive consoles do not have captured-output limits.

The executable's local proxy fixture explicitly selects blocking reads after
accepting a connection, then applies its bounded read timeout. It cannot rely on
platform-specific inheritance of the listener's nonblocking mode. Proxy-thread
panics retain their original cause. Native image cleanup reports the freshly
observed owned backing's entities and exact-file open handles when its expected
mount is already absent. It never selects the host filesystem in that state or
grants new detach authority. A busy-detach failure remains a failure; these
diagnostics do not establish its cause or extend cleanup limits.

## Shared suite runner

```sh
python3 scripts/verify.py deterministic
python3 scripts/verify.py cli
python3 scripts/verify.py telemetry
python3 scripts/verify.py live-catalog live-provider-cli live-modrinth server-cli --product-profile release
```

Fixed selections live in `scripts/verification/suites.py`. `mise run test:cli` runs
the offline executable suite. `mise run test:live` explicitly selects live CLI
routes and the rolling import survey as well. Telemetry checks are
ignored by default and fail if an explicitly selected product cannot produce a trace;
absence of the feature is not a successful test.

`mise run test:filter NAME` is a local exploratory Nextest substring filter, not a
qualification suite. `mise run survey:imports:pr` and
`mise run survey:imports:curated` survey the selected latest upstream packs; the
pinned corpus remains a separate check under `mise run test:import:corpus`.

`live-provider` includes both providers and requires `EMPACK_KEY_CURSEFORGE`.
The separate `live-modrinth` and `live-curseforge` selections name exact discovered
provider tests. `runtime-current` selects the managed vanilla and NeoForge launcher
checks plus the Fabric classpath check. `live-runtime` retains the full historical
matrix and additionally requires `EMPACK_TEST_JAVA8_HOME`. Missing named tests or
prerequisites cannot silently reduce one of these selections. CI live probes and
`test:live` build an independent release-profile product; normal deterministic and
CLI checks retain their development-profile builds. The test driver remains separate
from that product. Use `--product-profile release` for comparable local live evidence.
The receipt records the selected profile, and the CI gate rejects a development
artifact as a substitute for the live release-profile run. This does not change
installer/startup deadlines or retry a failed operation.

Each invocation creates a fresh directory under `target/verification/` containing
compiler discovery, Nextest JUnit, bounded redacted logs, a source-file digest manifest,
and `receipt.json`. The Rust inventory must match the reviewed platform and test-feature membership,
including ignore policy, before execution. `--test-features minimal` uses the
workspace with `--no-default-features`; `--test-features all` uses `--all-features`.
The harness still requests its explicit `test-utils` dependency in the minimal
graph. These flags describe the test graph, not the separately built product.
Receipts retain both discovery and execution commands; the gate checks their exact
selection and graph rather than trusting a feature label. Every selected test must
produce a passing outcome. Empty required binaries, missing results, skipped selected
tests, retries, failed commands and changed
source/binary bytes cannot yield a suite pass. The final CI gate also checks the
classification digest and the compiler inventory's actual host target. Missing tools
or prerequisites produce `BLOCKED`; the Unix-only desktop fixture on Windows and
the APFS remount fixture outside macOS produce explicit `NOT_APPLICABLE` results.

The runner does not retry an attempt. Re-running it creates another record and retains
the earlier failure. These are suite records, not final release qualification or proof
that every requirement has a bound case. The CI report joins validated Rust results to
the concrete cases and required platform/product-feature variants in the registry.
Missing variants remain `NOT_RUN`; a partial or pending decomposition cannot qualify
its requirement even when its existing cases pass.

Run Python tooling through the same entry point:

```sh
python3 scripts/verify.py python --output target/verification-python
```

This fixed suite uses unittest discovery, an isolated worker environment and a
15-minute deadline. It records each test's lifecycle alongside discovery, source,
classification and interpreter identities. Admission replays those events: subtest
failures, expected failures, skipped assertions, setup errors and incomplete runs
cannot become passing cases. Missing or repeated identities are rejected. PR CI
requires this record even for documentation-only changes; a green tooling job alone
is insufficient. Python cases use the `independent` feature variant and cannot
substitute for empack, packaged artifacts or actual consumers. Raw `unittest` remains
useful for focused debugging but does not produce an admissible receipt.

The worker uses the existing Rustup installation and selects the repository's
pinned compiler with automatic toolchain installation disabled. Cargo home and
application home/state/cache remain isolated; host Cargo credentials, configuration
and compiler wrappers are not inherited. The receipt identifies the compiler and
Cargo used by the small offline build fixtures. A missing pinned installation
blocks the suite before tests run; Cargo's `--offline` alone does not prevent
Rustup from downloading a toolchain.

Doctests are discovered and executed separately and their receipts are joined to
their own acceptance cases. Source-edited or interrupted tooling attempts remain
nonqualifying. `report-verification.py --receipt` accepts both native and Python
records and preserves earlier failed attempts.

To select an independently built executable, supply all identity fields explicitly:

```sh
python3 scripts/verify.py cli --binary /absolute/path/empack --sha256 EXPECTED_SHA256 --version EXPECTED_VERSION --target TARGET_TRIPLE --product-features default --product-profile release
```

The runner verifies digest and version. For an external executable, target, features and build profile
are caller-supplied provenance; final release qualification must bind them to the build
record. A locally built product records the runner's ordinary-package build selection.
Neither mode establishes actual consumer startup, platform coverage elsewhere, or final
archive installation.

## Change selection and final gate

The Verification workflow starts for every PR, including documentation and stacked
branches, and for pushes to `main` or `dev`. The former branch and post-merge
workflows are consolidated into this path. Pushes compare the exact before/after
trees; a new branch selects every tracked path. A missing comparison revision or
checkout mismatch fails planning. There is no path filter that can suppress the
terminal gate.
`scripts/ci-verification.py` compares immutable base/head revisions with renames
disabled, so moving code into a documentation path cannot hide its removal. Markdown
and editor-schema changes run Python, registry structure, Markdown and executable
schema checks. Other changes also select the deterministic native matrix, feature
checks, ARM64 compilation and one deterministic coverage run.
Each event gets fresh evidence for its checked-out revision. No PR-result reuse is
implemented; even an equivalent-looking merge is not treated as previously tested.
Lockfile, toolchain and verification changes therefore select fresh checks. ARM64
cross-build remains compilation evidence only.

Each native target runs the deterministic suite against the ordinary default product,
under the minimal workspace graph, then repeats the deterministic suite plus telemetry
contracts under the all-features graph against a separately built telemetry product.
The gate requires both product/graph pairs; a telemetry build or an all-features
inventory cannot stand in for the default product and minimal test graph.

Provider, runtime and managed-client paths select additional relevant probes.
Shared dependency/toolchain or verification changes select representative routes
across those boundaries. Changes to the CurseForge adapter or its mixed-provider
probe require the credentialed suite as well. This is change-based PR/branch evidence;
it does not replace the complete provider/loader matrix required for a candidate.

Live and consumer jobs run only on repository branches or a maintainer's manual
dispatch. Forks use GitHub-hosted runners for secretless checks, receive no provider
or coverage credentials, and cannot satisfy a required live job by skipping it.
The final gate reports the missing trusted execution. A maintainer can review and
test the change on a repository branch; the workflow does not use
`pull_request_target` to execute fork code with secrets. The selected CurseForge
job expects the `EMPACK_KEY_CURSEFORGE` repository or organization secret and records
`BLOCKED` if unavailable.

Manual dispatch also runs the rolling import survey and the existing import-smoke
driver. Broad surveys are excluded from ordinary PRs and instrumented coverage.
Pinned real-pack inputs and scheduled rotations use separate selections; an
upstream survey is not a stable regression oracle. Import/export changes select
the fixed `pinned-import` suite, while docs-only changes do not acquire packs.

Use `verification-gate` as the required PR status. It requires every selected job
to succeed, including the complete native matrix, and checks each uploaded suite
receipt against its compiler inventory and JUnit: source revision, target, product
features, selected identities, first-attempt results and evidence digests must agree.
Missing, cancelled, skipped, blocked or inconsistent evidence fails the gate.
Use the plan's actual checkout revision when inspecting CI receipts. A pull-request
checkout can be a synthetic merge commit with the PR head as a parent; the API's
head SHA alone is not the receipt's source revision. This does not permit substituting
an older source tree or treating a later merge as already qualified.
For admitted Rust receipts, its `acceptance` section retains each bound case's exact
tests, required host/product variants, artifact digests and supporting receipt digests.
It rejects substituted evidence boundaries, mismatched revisions and contradictory
records claiming the same receipt identity. All 109 requirement groups remain in the report, including undecomposed
native work and staged portable work. A passing selected CI suite does not make those
groups complete. The report and source registry digests travel together.

The gate writes `gate.json` and a readable `gate.md` together; CI uploads both and
adds the Markdown to its job summary. The view lists requirement groups, their
binding status, exact execution variants, and source/executable/receipt hashes.
Failed evidence admission produces a diagnostic summary without a passing acceptance
table. Report generation time is not an execution timestamp. The renderer does not
revalidate standalone JSON, apply waivers, or calculate coverage from test counts;
the JSON and original job artifacts remain the evidence to inspect.

Doctest cases retain their required mode (`run`, `compile`, or `compile_fail`) and
workspace test graph independently of product features. The release candidate gate
admits both graphs from each native host using the same observed-rustdoc verifier.
Discovery and execution normalize native source-path separators to the registry's
repository paths; item names, line numbers, modes and ignore policy remain exact.
Duplicate identities are rejected after normalization, including mixed spellings.
Compiler-only examples cannot replace executed examples, and failed attempts remain
visible even when a later run passes.

The resulting CI record has `qualification: false`: the remaining case decomposition,
script, consumer and device evidence and final-package checks are not yet a complete
release qualification record. Selecting this required status in repository
branch protection is an administrative step; changing workflow YAML does not set it.

To inspect failed runs alongside later attempts, supply each receipt explicitly:

```sh
python3 scripts/report-verification.py --revision FULL_COMMIT_SHA --version 0.8.4-beta --receipt /path/to/first/receipt.json --receipt /path/to/second/receipt.json --output /path/to/new-report
```

This command writes `attempts.json` and `attempts.md` after rechecking the selected
receipts against their retained inventories, JUnit and the checkout's classification.
The requirement registry must also match the input captured by the run. Changing
an assertion or case requires new evidence; an unchanged test name cannot carry
an old passing result into the new requirements. Rust, Python and rustdoc admission
all enforce this binding.
Keep each attempt directory intact. Use the source revision and product version
actually recorded by the selected runs; this command does not execute tests or qualify
a different candidate. Select all relevant attempts explicitly: it cannot discover
omitted history. Identical receipt files are counted once. Distinct attempts retain
their dates, suite names, test graph and executable hashes. A failed test followed by
a pass stays failed in the combined view, including when two suites execute that test
within one receipt. Missing tests remain unexecuted.

A blocked preflight can explain the missing prerequisite without supplying an
executable or passing tests. Unreadable, malformed, mismatched or changed evidence
is listed as rejected and contributes no acceptance results. Failure of the runner's
final source or product check invalidates its earlier test results. Reports preserve
failed and blocked outcomes ahead of unstarted suites; they never turn an incomplete
run into success. Exit status is `0` for passing selected attempts, `2` for a blocked
selection, and `1` otherwise. Even `0` does not establish release qualification.
Strict CI and archive gates still require one complete passing attempt per selected
configuration. The report does not grant waivers or replace those gates.

## Strict external checks

```sh
mise run test:live
mise run smoke:runtime
mise run smoke:providers
```

Strict execution fails when prerequisites are missing. Runtime checks need public
Mojang, loader and Maven access plus Java 21 through `JAVA_HOME` or `PATH`. Historical
Forge cases use `EMPACK_TEST_JAVA8_HOME`. Provider checks use declared credentials;
`EMPACK_KEY_CURSEFORGE` supplies the explicit live CurseForge suite. Never record
credentials, signed locators or downloaded third-party bytes in committed reports.

The runtime task builds the CLI before running. Managed-server probes use
`EMPACK_E2E_BIN` when supplied, otherwise the workspace debug executable. They
extract a reference distribution, run its generated installer and launcher, require
a completed native instance record, and reach Minecraft help through local Java.
Host state and caches remain in temporary directories.

The runtime matrix covers vanilla, Fabric launcher layouts, Quilt, historical and
modern Forge, and early/current NeoForge families. Validate official installer
profiles, expected libraries and launch arguments. Help output or EULA refusal
proves launcher execution, not gameplay; tests never accept the EULA.

## Consumer and release evidence

Inspect exported manifests and archives independently, then test the actual supported
consumer. Prism ZIP imports, Modrinth and CurseForge format acceptance, server startup
and native instance updates are separate gates. Do not assume a TAR/7z tree is
importable wherever a ZIP is accepted.

Use a separate Prism profile through `--dir` and import each generated archive
through `--import`. Authenticate through the launcher when needed; never copy
account files into test fixtures or publish launcher logs containing credentials.
Check the imported runtime components and selected file bytes. Prism may use
`minecraft/` for platform imports and `.minecraft/` for native instance archives.

For a referenced Prism export, launch the imported instance and verify both native
preparation and the `Prism → empack → Java` process chain. Change the test release
descriptor without updating its expected hash, then launch again: empack must fail
preparation, Prism must propagate the nonzero result, and Java must not start.
Restore the descriptor afterward. Modrinth and CurseForge snapshot imports must
launch without empack hooks. Record the consumer version and distinguish successful
import, game startup and gameplay; these are different assertions.

Native release acceptance includes A-to-B updates, choices, user-edited configuration,
manual content, offline policy, trust failures, runtime changes and rollback. Crash
tests terminate a real subprocess at durable boundaries and recover in a new process.
Use a played-world sentinel to prove update/rollback confinement.

Curated large-pack workflows run through the production CLI and resource profile.
Inspect inventory and selected bytes, not only artifact size or exit status. Compare
repeated sync and rebuild behavior with unchanged version labels.

## Extracted archive checks

Run the fixed offline CLI suite against an existing distribution archive on its
native execution host:

```sh
python3 scripts/check-release-archive.py --archive /absolute/path/empack-0.8.4-beta-aarch64-apple-darwin.tar.gz --sha256 EXPECTED_ARCHIVE_SHA256 --version 0.8.4-beta --target aarch64-apple-darwin
```

The checker captures and hashes the archive before extraction, admits only the
expected regular executable, checks Unix executable attributes, and extracts outside
the checkout into a path containing spaces. It invokes the shared `cli` suite with
the extracted executable's exact digest and version, without discovery or rebuilding
the product. Each public command still uses the harness's isolated home, state and
cache. Missing native execution support is `BLOCKED`; cross compilation is not a
substitute. The test harness itself is compiled from the current test checkout.

`target/verification-packages/` retains one package record and the underlying suite
receipt, compiler inventory, JUnit and bounded logs per attempt. Archive and executable
digests are rechecked afterward. A changed, failed or unexecuted archive cannot pass.
The record distinguishes the test-source revision from the product's caller-supplied
identity; it does not establish the product's build provenance by itself. The release
workflow joins this evidence to the corresponding build record. It executes each of
the five archives on its native target, including ARM64 Linux; cross-compilation alone
does not satisfy execution. The gate rechecks source, version, profile, features,
archive/executable digests and the underlying compiler inventory and JUnit results.
Unix packaging excludes host filesystem metadata, including macOS AppleDouble entries.
Only those tested archives enter the publication payload. Publication verifies its
checksums and the gate's evidence digest again, without rebuilding.

`workflow_dispatch` accepts the reviewed version and exercises this pipeline without
publishing. Tag publication now requires both `archive-gate` and `candidate-gate`.
The latter retains all 88 native obligations and makes the 21 staged portable
obligations mandatory from v0.9. Incomplete case decomposition, absent required
evidence boundaries and missing observations block qualification, even when every
archive test passes. Missing platform execution remains a failure, not a cross-build
pass. A configured workflow is not proof that its five-host execution has succeeded.

The current registry is incomplete, so the candidate gate deliberately blocks
publication. Completing its native/consumer case bindings, collecting their evidence
and adding the corresponding workflow producers remain v0.8.4 implementation work.
The workflow currently supplies the final-archive receipts; it does not yet supply a
complete candidate matrix. No approved low-risk waiver policy is installed, and this
gate accepts no waivers. Critical or actual-consumer omissions cannot be excused by a
caller-supplied result label.

Release builds now run `scripts/prepare-release.py` in a clean disposable checkout.
The requested version must match the reviewed workspace release target. Stamping
changes only the workspace version and its local package entries in `Cargo.lock`;
external dependencies and their checksums remain unchanged. The input record retains
the source commit/tree, original and stamped Cargo-file digests and toolchain digest.
Build metadata uses the source commit's date, and both Cargo and cross build with
`--locked`. A new dependency or an ambiguous lockfile shape requires a source change,
not implicit dependency resolution during publication. `Cross.toml` forwards only the
public `GIT_HASH` and `BUILD_DATE` stamping variables into the cross-build container.

Build artifacts retain `build.json`; execution artifacts retain the package and suite
receipts. `scripts/release-artifacts.py gate` validates their complete five-target set
and writes `archive-evidence.json` plus `SHA256SUMS` alongside the unchanged archives.
The evidence explicitly records that full candidate qualification remains separate.

To evaluate an already checked payload without publishing:

```sh
python3 scripts/qualify-release.py gate --payload /path/to/checked-native-archives --consumer /path/to/prism.json --archive-evidence-sha256 EXPECTED_GATE_DIGEST --revision FULL_COMMIT_SHA --version 0.8.4-beta --output /path/to/candidate.json
```

Run from the exact clean candidate checkout. This rechecks the archive payload's
identity and evaluates the source registry, then writes JSON and Markdown with
explicit outstanding obligations. Supplemental `--receipt` inputs are rechecked
against inventory/JUnit, the same source/version and the actual shipped binary for
that target. Bare suite receipts cannot admit arbitrary feature builds.

For a required telemetry variant, run the fixed auxiliary producer from the clean
candidate checkout:

```sh
python3 scripts/check-auxiliary-build.py --version 0.8.4-beta --output .git/auxiliary
```

It stamps a disposable source clone, builds only the ordinary `empack` package with
`--features telemetry --release --locked`, and runs the shared CLI and telemetry
suites against the retained executable. The build uses an isolated Cargo home, a
separate target directory, the pinned native compiler and the candidate's build date.
The original source checkout and default archives remain unchanged. Add each emitted
`build.json` to candidate evaluation with repeated `--auxiliary` options; keep its
executable and `suite/` evidence alongside it. Admission checks the prepared inputs,
compiler, fixed command, executable bytes and independently replayed suite results.
These are trusted workflow producer records, not third-party signed attestations.

Auxiliary builds can satisfy their explicit feature/host cases, but never package
cases or replace any of the five shipped archives. Failures remain failures even if
a later attempt passes. The candidate workflow produces telemetry evidence on Linux,
macOS and Windows; it does not treat cross-compilation as native execution.
Repeated `--consumer` inputs admit the fixed Prism parser producer for the same clean
source. Admission checks the supported Qt/compiler environment, probe and fixture
identities, and every recorded setting. A claimed PASS with broken required commands
is rejected; an accurately recorded failure remains failed. This evidence contributes
only to the parser case, independently of product features. It cannot qualify actual
Prism import/startup or any missing native case. The release workflow runs the same
parser action as PR CI and requires its record before candidate evaluation.
Repeated `--tooling` inputs admit the fixed Python producer for the same clean source.
The candidate workflow runs it before evaluation and retains its discovery, lifecycle,
log and receipt files. It contributes only to explicitly bound tooling cases; it does
not supply product-feature, archive or consumer proof.
The command returns `0` only for full qualification, `2` for missing obligations and
`1` for failed or rejected evidence.

Package cases reference ordinary executable test IDs, but require additional proof
from the archive gate. Running the same test through ordinary CLI CI does not satisfy
its package case. Each package variant identifies an exact target triple; Intel and
ARM64 evidence cannot substitute for each other merely because they share an OS.
The initial `UX-085` package case checks local content export and instance preparation
on all five targets. It is a partial binding, not complete package acceptance.

The release job rechecks both the archive-evidence digest and the qualification
digest from its prerequisite jobs, including source, version and payload association,
before publishing unchanged bytes. Candidate diagnostics stay in a separate workflow
artifact; they do not become extra distribution files. A blocked candidate cannot
reach publication through a passing archive job.

## Coverage and release qualification

Measure production-source coverage separately from inline test bodies where tooling
permits. Coverage supplements behavioral evidence and must use the candidate revision.
Do not combine unrelated historical runs into a same-head acceptance claim. Native
platform, provider, runtime and consumer results identify their tested revision,
tool versions and explicit exclusions. Release publication follows successful
acceptance of one immutable candidate and dependency graph.

Generated project automation is covered by `scripts/tests/test_project_automation.py`
and the executable `smoke_generated_workflow` contract. The latter initializes a pack
with spaces in its name, builds Modrinth and Prism profiles through the generated
helper, rejects a mismatched tag and excludes stale output. Python tests verify
download digests, staging names, checksum contents and draft publication ordering.
Windows CI executes generated batch scripts with native `cmd.exe`; Java fixtures
check argument selection and exit status without claiming Minecraft gameplay.

Server archive tests check both console and GUI launchers in ZIP, TAR and 7z outputs.
Native shell and Windows command fixtures verify the default `nogui` argument, its
absence from GUI launchers, argument boundaries, preparation ordering and exit codes.
Live runtime probes exercise console launchers; they do not certify desktop GUI rendering.

`cli_server` uses a Cargo-built executable in the external test package to observe
Java probe and runtime invocations. It checks explicit/PATH program selection,
approval, minimum versions, malformed probe output, exact arguments and game cwd.
Failed probes and unsuccessful runtime exits must allow a later launch without
stale runtime ownership. Player data, `eula=false`, completed installation records
and input/tool bytes remain unchanged. The fixture records its digest and is absent
from the ordinary product package's dependency graph.

A compiler-discovered private driver supplies the actual generated server scripts
as component setup. The test executes those files unchanged through the selected
product and local fixture, using Bash on Unix and `cmd.exe` on Windows. Console/GUI
arguments, `JAVA_HOME`/PATH selection, preparation-before-probe, failure refusal and
managed versus direct-runtime exit status have separate assertions. These checks
do not start a JVM or a graphical application; official loader help/EULA probes and
actual interactive consumer acceptance remain separate obligations. The setup
driver's empty ordinary invocation is not counted as acceptance evidence.

## Managed consumer interpretation

Managed Prism configuration tests include absent, unsupported, duplicated and
misplaced version markers, command shadowing, multiline quoting, and hash-prefixed
settings with open quotes or trailing continuations under both supported versions.
Include ordinary space/tab controls and Unicode whitespace at key, section, comment
and value boundaries. Accept LF/CRLF and unterminated final lines; reject bare CR and
unsupported controls. Unicode display-value fixtures remain accepted without changing
author bytes. Semicolon controls preserve the effective lifecycle settings. Compare the
same fixtures with Prism 11.1.1's actual `INIFile::loadFile` implementation, using
a clean upstream checkout and Qt 6.4 or newer within major version 6, as required
by that Prism revision. Qt is opt-in test tooling, not an empack runtime dependency.
The probe rejects unsupported Qt versions before compilation. Earlier Qt 5 runs are
supplemental diagnostics, not qualification of Prism 11.1.1. Consumer-parser execution
is separate from launching Minecraft. Its receipt records application and empack
revisions, dirty state, fixture SHA-256, platform/architecture, compile/runtime Qt
versions, compiler identity, build configuration, probe executable digest, observation
time and source-input digests. The schema-2 record retains every fixture observation;
settings failures return a nonzero status after writing the observations. The gate
recomputes the outcome from those settings rather than trusting the status field.
Dirty-source results remain diagnostic only. The producer currently runs on Linux
and macOS; the required parser case selects Linux, matching the CI runner. The same
observations support UX-053's Prism integration and UX-083's headless configuration
contract without multiplying execution counts. Actual import and startup remain
UX-084 obligations. This does not qualify other consumer operations or hosts.
The probe compiles upstream `INIFile.cpp` and `Json.cpp`
without vendoring or changing their parsing code:

```sh
python3 scripts/check-prism-config.py --prism-source /path/to/PrismLauncher-11.1.1 --qmake /path/to/qmake > /path/to/prism.json
```

Use a clean checkout at `7d2d3c1ec6fcf8255ebacd02d9f792688575dfb4` and supported Qt 6
development tools plus `make`. The shared fixtures also run through empack's public build
preparation. The probe verifies accepted settings and reproduces the legacy
shadow and hash-prefixed quote/continuation counterexamples; it does not certify
every rejected representation.

Managed server tests check archive attributes in ZIP, TAR.GZ and 7z for identical
scripts captured with and without executable source permissions. On Unix, extract
and directly execute the shell entry points with controlled fake external tools.
Windows tests execute batch entry points with `EMPACK_NO_PAUSE=1`, checking full
diagnostic text, empty syntax-error output and preserved exit status. Source
permissions and authored bytes remain unchanged after candidate preparation.

## Documentation checks

The repository-relative validator checks inline local Markdown links and anchors,
fenced blocks and selected prose rules. It does not check external URLs or claim to
parse every Markdown extension. File checks work without Git metadata:

```sh
python3 scripts/check-docs.py docs/testing.md docs/design/builds.md
```

Git-dependent checks are explicit. After fetching the relevant branches, validate a
stacked contract snapshot without switching the working tree:

```sh
python3 scripts/check-docs.py --ref origin/dev-v0.9-verified-export --base origin/dev-v0.9-browser-store --docs-only --strict-prose docs/development/v0.9.0-beta
```

This checks parent ancestry, changed-file scope and diff whitespace in addition to
the selected snapshot's documents. Ref names are inputs; no developer-local branch
list, fixed main revision or `.git/` helper is required. Documentation checks do not
run the native or browser/device acceptance matrix.
