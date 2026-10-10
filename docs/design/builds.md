# Consumer outputs

A distribution recipe identifies the consumer, dependency delivery and update
authority. They are separate from archive compression and client/server selection.

## Recipe model

```rust
pub enum Consumer { Modrinth, CurseForge, Prism, Server, Empack }
pub enum Delivery { References, Bundled }
pub enum UpdateAuthority { Snapshot, Platform, Empack }
```

The pure `distribution::Recipe` value validates policy combinations before acquisition. References
can include authored assets; bundled pack content does not mean an offline Minecraft
client or an unconditional right to redistribute dependency bytes.

| Consumer | Artifact | Dependency delivery | Update authority |
| --- | --- | --- | --- |
| Modrinth | `.mrpack` ZIP | Format references and permitted overrides | Snapshot or platform |
| CurseForge | Manifest ZIP | Exact provider references and permitted overrides | Snapshot or platform |
| Prism | Native instance ZIP | References through empack, or bundled pack content | Snapshot or empack |
| Server | Directory or supported archive | References through empack, or bundled pack content | Snapshot or empack |
| Empack | Native release manifest and assets | Exact references and optionally bundled assets | Snapshot or empack |

CurseForge and Prism recipes select a client environment; dedicated server recipes
select server. Modrinth and native release recipes can preserve both environments.
Per-file environment requirements still participate in each projection.

Platform update authority requires platform project/version association; writing an
archive does not create that association. Automatic marketplace upload is outside
this contract. Tar/7z output is permitted for directory distributions where supported;
it must not be advertised as a launcher-importable ZIP.

Authored `distribution.recipes` is a nonempty ordered list of objects. Each object
requires `consumer`, `delivery` and `environment`; `updates` defaults to `snapshot`.
Unknown fields, old target strings and unsupported combinations fail decoding.
The exact four-field recipe survives planning, build receipts and restart requests.
Deduplication compares the complete recipe, so delivery or authority changes never
collapse into the same request.

Initialization and imports default to Modrinth references, bundled Prism and bundled
server snapshots. Native reference consumers require an explicit stable pack identity
and Java requirement; those values are not inferred from display names. Selecting a
valid policy does not establish a platform association or publisher trust. Execution
must bind the relevant evidence before acquiring or publishing an output.

## Build data flow

Capture intent, lock, source layers, templates and exact runtime once. Resolve saved
choices, construct the expected game inventory, project consumer requirements, then
acquire only bytes needed by each recipe. Shared verified content uses leases;
producing one consumer does not require publishing another consumer's artifact.

Native reference consumers establish exact content addresses from the selected game
inventory before emitting a release. They retain original provider selections and
source assertions alongside those addresses. Excluded environments and disabled
choices do not require acquisition. Authored assets remain embedded; referenced
dependencies retain acquisition instructions. Projecting a consumer never rewrites
the author lock or adds invented source evidence.

Prepare every requested output before combined publication. A later failed recipe
leaves earlier published artifacts unchanged. Build does not silently sync, upgrade
provider selections or infer dependencies from untracked metadata. Acquisition keys
contain the logical dependency key and exact file role from the native lock.

Untracked included source files contribute their captured bytes, without provider
identity or download authority. `sources.exclude` applies to every source layer;
archives have no implicit exemption. Explicit locked sources and placements remain
observation obligations even when a broad exclusion matches them.

## Native release batches

The empack consumer uses the shared build request and acquisition pipeline. Reference
and bundled recipes may appear together with platform and launcher outputs. A native
release establishes exact content addresses before publication; source assertions
remain separately recorded. Missing restricted bytes use the same saved build request
and verified association mechanism as other consumers.

`distribution.native` contains stable pack identity, Java requirements and destination
ownership policies. Delivery belongs to each recipe. Reference and bundled artifacts
have distinct names and payload identities. A receipt reports the exact release JSON
identity separately from the surrounding archive inventory. Optional choices remain
in the release for installation; native-only builds reject materialization choices.

## Modrinth

Write `modrinth.index.json`, exact SHA-1/SHA-512 assertions, sizes, HTTPS alternatives,
runtime dependencies and common/client/server overrides. Keep side participation
and optionality separate. Conditional fallback and grouped choices require an
explicit representable selection when the format cannot preserve them.

Generic format validity and Modrinth hosting eligibility are separate results.
Hosting validation uses the documented download-domain rules, not an assumption
that all valid HTTPS URLs are accepted. Standalone archive import does not subscribe
the instance to an arbitrary update URL.

References: [format](https://support.modrinth.com/en/articles/8802351-modrinth-modpack-format-mrpack)
and [sharing](https://support.modrinth.com/en/articles/8797522-sharing-modpacks).

## CurseForge

Write `manifest.json` and an overrides directory in a ZIP. References identify
exact CurseForge projects/files and required participation. Preserve exact loader
selection, authored configuration and representable optional content. Reject lossy
placement or environment conversion unless the author explicitly selects a supported
projection. Content that cannot be represented is named in the diagnostic.

Project/file references retain the provider filename and standard content directory.
A renamed file or custom destination cannot be encoded in this manifest. Reject it
instead of exporting a reference that installs somewhere else. Multiple selections
from one provider project are also rejected when the consumer cannot preserve them.

A required local override is embedded at its selected client destination. Client
overrides take precedence over common content; server-only content is excluded.
Optional overrides require an explicit selection before export. Reference optionality
can become `required: false` only after explicit acceptance of lost choice keys,
defaults and descriptions. An unresolved choice is not silently enabled.

A Modrinth project name or slug cannot establish a CurseForge equivalent. A verified
cross-provider file association or explicitly permitted embedding is required.
Format validity is not hosting approval or redistribution permission. Report hosting
eligibility separately and do not silently embed provider-restricted files.

Verification reads the candidate manifest through a separate strict decoder and
compares project/file IDs, participation, metadata and loader selection with the
selected inventory and exact lock. ZIP byte verification alone does not establish
these semantics. Check captured provider permissions for selected references after
environment, precedence and optional-choice projection. Attributes on excluded files
do not invalidate a recipe; selected references cannot discard observed attributes.

Reference: [CurseForge export contract](https://support.curseforge.com/support/solutions/articles/9000197908-exporting-a-modpack-for-curseforge-project-submission).

## Prism

Produce an importable instance ZIP with `instance.cfg`, exact `mmc-pack.json`
components, selected icons/configuration and `.minecraft` content. The launcher
obtains its normal game binaries, libraries and assets. Ordinary snapshots require
no empack updater when pack content is bundled.

Reference delivery carries an exact release and embedded authored assets under
`.minecraft/.empack-consumer/`. This input directory also establishes Prism's game
layout before its first prelaunch hook. It is reserved against selected game files
and templates. Installed payloads are created by the instance engine, not preseeded
as unowned archive members. Templates cannot occupy a future installed path.

The generated `instance.cfg` invokes `empack instance prepare` with an exact release
hash, explicit instance root and Prism layout. The executable must be on the
launcher's PATH and satisfy the descriptor's engine requirement. Failed preparation
must stop launch. Subsequent preparation retains the active release and saved
choices; it cannot reinstall the archive's original release over an update. The
launcher profile must still match the active runtime.

Captured user configurations remain user input; their arbitrary commands are not
certified as native prelaunch integration. Preserve argument boundaries, prelaunch
exit status and runtime-component requirements. Do not allow platform and empack
updaters to manage the same files without an explicit authority transfer. Real
launcher import and execution tests establish compatibility, not only JSON parsing.

Reference: [Prism imports](https://prismlauncher.org/wiki/getting-started/download-modpacks/).

## Dedicated servers

Prepare the exact vanilla, Fabric, Quilt, Forge or NeoForge server runtime and
selected server content. Preserve historical supported loader variants, runtime
asset evidence and generated start scripts. Java selection and EULA acceptance
remain operator responsibilities. The installer never accepts the EULA automatically.

Both deliveries place runtime files and selected game content under `game/`. Root
`start.sh` and `start.bat` scripts enter that directory and preserve separate Java
arguments, including historical JAR and loader argument-file launch forms. Put
server configuration templates under `templates/server/game/`; launcher scripts
remain at the distribution root.

Reference delivery packages `.empack-consumer/release.json` and authored assets.
`install_pack.sh` and `install_pack.bat` invoke `empack instance prepare` with the
exact descriptor hash and server environment. The generated start scripts run that
step first and stop on failure. Bundled snapshot delivery needs no empack installer.
Template files and runtime members cannot collide with future installed content.

Fabric/Quilt launcher layouts, Forge-family installer profiles, libraries and
arguments are independently inspected after bounded installer execution. Exit zero
or finding a JAR is insufficient. Snapshot bundles and reference distributions
share one expected runtime; delivery changes acquisition, not runtime semantics.

Updates require a stopped/coordinated server. Worlds, player data and operator
configuration use instance policies rather than archive overwrite behavior.

## Native empack

Generate the [release payload and signed envelope](releases.md) from verified exact
content. A hosted channel is a separate publication artifact. Private authored paths,
credentials and host-state directories never appear in the portable release.

## Templates and verification

Preserve expressions until build-time inputs are known. Encode shell, INI and Java
properties values for their output syntax. Treat binary templates as bytes. A template
cannot silently replace a verified runtime file or collide with another placement.

Independently inspect archive member names, duplicates, case/Unicode collisions,
permissions, sizes, digests, unexpected entries and actual decoded-byte limits.
Check exact provider identities and consumer semantics, not merely archive readability.
Receipts record recipe, resolution, content evidence, conversions and runtime assets.
