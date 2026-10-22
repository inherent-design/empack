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
this contract. Platform-targeted exports retain their policy in the receipt and
report the required external association. The receiving platform establishes that
association when installing its hosted project/version; generic archive import
remains a snapshot. Empack does not invent embedded project IDs or claim an upload
occurred. Modrinth platform exports require allowed hosting download domains;
snapshot mrpacks report domain ineligibility without rejecting a valid archive.
Tar/7z output is permitted for directory distributions where supported;
it must not be advertised as a launcher-importable ZIP.

Authored `build.profiles` maps portable names to complete recipes. Each
profile expands `target`, `delivery`, `environment`, `updates` and `format`.
Prism and server string presets select bundled snapshot ZIP outputs.
`build.default` is an ordered list of distinct profile names.
Unknown fields, dangling default names and unsupported policy combinations fail decoding.
Author-only projects may omit profiles and defaults; building then requires an
explicit configured profile or an ad-hoc target.
Names contain 1 to 64 ASCII letters, digits, hyphens or underscores. Profiles have
no inheritance or implicit relationship to other profiles.

Build explanation schema 2 includes `pack_id` and the project-owned Java minimum.
Generated automation requires this version and a current lock.

`build` selects the authored defaults. `build --profile NAME` selects that exact
profile; it is repeatable. A consumer shorthand such as `build prism` selects the
single configured Prism profile and preserves its delivery and update authority.
Missing or ambiguous consumer matches require a profile name or explicit
`--ad-hoc`. Ad hoc recipes use built-in snapshot defaults. CLI policy and format
overrides apply only to the invocation. Modrinth and CurseForge always use ZIP,
including when a mixed-consumer invocation requests another format.

`build --explain` and `build --explain --json` resolve the same effective profiles
and output names as execution without acquiring content or changing state. The
schema-2 report identifies value origins, recipient requirements, optional policy,
lock status and declared manual inputs. A missing or stale lock leaves input facts
unknown. This report does not verify downloads or prove build readiness.

Every `BuildOutput` carries its own archive format. Duplicate selections of the
same profile are collapsed; distinct profiles with colliding output paths fail
before acquisition. Saved continuation retains effective recipes, paths and formats.
Author or binding edits invalidate the saved context instead of changing its recipes.

Initialization, imports and ad-hoc builds default to reference delivery for
Modrinth, Prism and server snapshots. Schema-5 authored `prism` and `server` string
presets retain their bundled meaning; new scaffolds write explicit reference
profiles. Existing profiles and generated workflows remain author-owned. Native reference consumers require an explicit stable pack identity
and Java requirement; those values are not inferred from display names. Selecting a
valid policy does not establish a platform association or publisher trust. Consumer
activation binds the relevant evidence. Native, Prism and server exports with
`updates: empack` require explicit local enrollment before their initial snapshot
installation. Launcher wrappers
check the enrolled channel before launch; exported keys never create trust. Reference
and bundled delivery both use native ownership when `updates: empack`, with bundled
bytes carried as verified release assets. Snapshot bundles remain independent of
empack at runtime. Artifact names distinguish subscribed and snapshot recipes.
Every managed Prism or server recipe requires its preparation and launch integration:
reference snapshots and all `updates: empack` recipes. Custom Prism configurations
retain the generated `OverrideCommands`, `PreLaunchCommand` and `WrapperCommand`
settings. Managed `instance.cfg` uses the supported QSettings formats with exactly
one `ConfigVersion=1.2` or `ConfigVersion=1.3` in `[General]`. Version and command
keys use their canonical spelling and appear only in that section. Legacy,
unsupported, shadowed and multiline interpretations fail preparation. Other values
must have balanced quotes on one physical line; escapes remain allowed inside
quoted values. This bounded subset prevents consumer parser selection from changing
the meaning of verified settings. Only semicolon-prefixed lines are comments in
managed configurations. Hash-prefixed lines are rejected with a diagnostic directing
authors to use `;`: QSettings can interpret them as active settings, including quotes
and continuations that consume later lifecycle settings.

Structural padding uses only ASCII space (U+0020) and tab (U+0009). Line endings
are LF or CRLF; the final line may omit its terminator. Bare CR and other control
characters are rejected, including in comments. Unicode whitespace is not trimmed
into a section, comment or canonical key. Keys and section names retain the restricted
ASCII grammar; values may contain Unicode, including non-ASCII spaces in display
names. Value padding removes only space and tab, so unsupported characters cannot
be normalized into a required command or version. These are accepted-subset rules,
not a claim to implement every QSettings syntax extension.

Custom server entry points must match the generated scripts exactly;
shell text is not interpreted to certify an alternative lifecycle. Incompatible
replacements fail preparation without rewriting the template or publishing an
artifact. Names, icons, memory settings, server properties and other unrelated
content remain customizable. For these six managed entry points, the candidate
uses generated writable permissions: shell scripts are executable and batch scripts
are not marked Unix-executable, regardless of source-host permissions. The source
files are unchanged. This rule does not apply to unrelated `.sh` templates.
Bundled snapshots retain standalone command and
script customization. There is no implicit custom-integration bypass.

### Prism-owned icon

For managed Prism recipes, the selected game-root `icon.png` defaults to `seed`.
Prism writes that file before running the preparation hook, so it cannot be enforced
as immutable managed content. An explicit `seed` policy is preserved; an explicit
`managed` policy fails preparation with guidance to set `policies: {icon.png: seed}`
in `empack.bindings.yml`. The authored policy is not silently rewritten. Root-name
case variants receive the same rule; nested `icon.png` and other PNG files retain
their ordinary policies. Standalone bundled snapshots are unchanged.

This rule affects newly built release descriptors. Updating the empack executable
does not rewrite a previously published archive or its ownership policy. Rebuild
old distributions with the intended icon policy and use the normal instance update
workflow. For an existing regular icon file, Managed → Seed preserves its current
bytes, including launcher changes, without a conflict override. Seed → Managed
normally raises an ownership conflict; Prism additionally rejects that incoming
policy before conflict decisions. Managed → Managed retains the normal modified-file
checks in consumer-neutral installations and is invalid for a Prism icon.

Generic native exports remain consumer-neutral and can legitimately manage a root
image. Authors publishing native releases to Prism channels must explicitly retain
`policies: {icon.png: seed}` in their bindings. Every Prism installation, update,
repair, rollback and launch preparation validates the selected release's effective
icon policy. An incompatible managed icon is rejected with publishing guidance;
authenticated release policies are never rewritten on the recipient. This also
prevents a conflict override from reintroducing launcher-owned managed content.

## Build data flow

Capture intent, lock, source layers, templates and exact runtime once. Resolve saved
choices, construct the expected game inventory, project consumer requirements, then
acquire only bytes needed by each recipe. Shared verified content uses leases;
producing one consumer does not require publishing another consumer's artifact.

Native release recipes accept client, server or both environments. Side selection
retains applicable layers and optional choices, and excludes nonparticipating
dependency roles before acquisition. Unknown destination policies fail validation;
policies for an excluded environment do not become release mutation authority.

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

`pack.id` supplies stable pack identity and `runtime.java` declares the recipient
Java minimum. Integration bindings carry destination ownership policies. Delivery belongs to each recipe. Reference and bundled artifacts
have distinct names and payload identities. A receipt reports the exact release JSON
identity separately from the surrounding archive inventory. Optional choices remain
in the release for installation; native-only builds reject materialization choices.

## Modrinth

Write `modrinth.index.json`, exact SHA-1/SHA-512 assertions, sizes, HTTPS alternatives,
runtime dependencies and common/client/server overrides. Keep side participation
and optionality separate. Conditional fallback and grouped choices require an
explicit representable selection when the format cannot preserve them.

Generic format validity and Modrinth hosting eligibility are separate results.
Each mrpack build receipt includes a download-domain report. The CLI identifies
blocked destinations and hosts while retaining the valid generic archive. The
check uses exact domains: `cdn.modrinth.com`, `github.com`,
`raw.githubusercontent.com` and `gitlab.com`. It does not establish redistribution
rights, marketplace approval, upload, or a launcher association. URL paths and
credentials are omitted from this report.
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
Restricted provider files retain this filename evidence even when their download
alternatives are empty. A generic manual record with only a pin must be resolved
before CurseForge export; its logical role is not filename evidence.

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

Managed `instance.cfg` templates must preserve the generated preparation and
wrapper commands, even when no subscription is configured. Use the
`INSTANCE_PREPARE_COMMAND` and `INSTANCE_WRAPPER_COMMAND` template values with
`ini_quote`, and keep `OverrideCommands=true`. Removing the custom `instance.cfg`
lets the builder supply the generated configuration. A bundled snapshot may use
standalone commands; arbitrary user commands are not certified as native prelaunch
integration. Preserve argument boundaries, prelaunch
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
`start.sh` and `start.bat` scripts supply `nogui` after the runtime launch arguments.
`start_gui.sh` and `start_gui.bat` omit it. Both variants preserve separate user
arguments, including historical JAR and loader argument-file launch forms. Managed
variants use the same preparation and subscription checks before launch. Put
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
ZIP exports use a Unix creator marker and portable mode bits on every host,
including Windows, so executable scripts retain their intended permissions.
Check exact provider identities and consumer semantics, not merely archive readability.
Receipts record recipe, resolution, content evidence, conversions and runtime assets.

Modrinth recipes support client, server or both environments. Selection precedes
acquisition, so an excluded role does not require download evidence. Selected
references retain exact hashes and sizes; side-specific exports mark the other
side unsupported. Common and side overlays resolve to the effective bytes for
the selected environment before archive paths are assigned. The receipt retains
the complete recipe used to produce the archive.

Generated workflows build the authored default profiles or explicitly named profiles.
Their installation, staging and publication contract is [project automation](automation.md).
The default Prism template inherits launcher memory settings.

## Windows server entry points

Extract the whole server archive before running `install_pack.bat` or `start.bat`.
Bundled snapshots already contain their payload; installation confirms that state.
Reference or subscribed exports prepare their exact release through empack on PATH.
Starting uses `JAVA_HOME/bin/java.exe` when configured, otherwise Java on PATH.

The installer and both start batch entry points retain their console until a key is pressed and preserve the
operation’s exit code. Set `EMPACK_NO_PAUSE=1` for terminal automation and services.
The start script suppresses the nested installer pause. Failures explain the required
Java or empack prerequisite and direct the operator to the preceding output. Initial
Minecraft EULA refusal remains visible; the operator must read and accept `game/eula.txt`.
Empack never accepts the EULA automatically.
