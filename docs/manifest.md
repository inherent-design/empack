# Manifest

Edit `empack.yml` to describe your pack. Run `empack sync` to satisfy those requests,
then build from the exact selections recorded in `empack.lock`.

```yaml
schema: 5
pack:
  id: workshop
  name: Workshop
  version: "Autumn Update 2"
  authors: [Mannie]
runtime:
  minecraft: "1.21.1"
  loader: neoforge
  java: 21
dependencies:
  openloader: "modrinth:open-loader"
  menu:
    source: "modrinth:fancymenu"
    side: client
  house-rules:
    path: vendor/house-rules.jar
  textures:
    path: vendor/textures.zip
    type: resource-pack
    side: client
    optional: true
build:
  default: [client, server]
  profiles:
    client: prism
    server:
      target: server
      format: tar.gz
```

The example shows syntax, not a tested mod combination. Keep configuration files
under `pack/`, side-specific files under `overrides/client/` or `overrides/server/`,
and rendered files under `templates/`. Ordinary configuration does not need a
dependency record.

## Sources and pins

A provider source accepts a readable exact selector or canonical ID. The lock keeps
the resolved project identity. Sync retains valid selections; `update` deliberately
selects newer eligible content. A renamed or reassigned provider slug does not
silently change an existing project binding.

Use `pin` to select a particular provider version or file ID:

```yaml
menu:
  source: modrinth:fancymenu
  pin: "PROVIDER_VERSION_ID"
  side: client
```

`empack pin menu` records the current locked selection. `empack unpin menu` removes
that restriction without selecting a newer file. Pinning removes a dependency's
`releases` override because automatic preference does not apply to an exact pin.
Unpinning uses the project preference unless you add an override again. Human version names are display
labels, not dependency ranges.

A local `path` is the source file, not its installed destination. `add` retains a
file already inside the author checkout at that source path; an external file is
copied into project storage to keep the declaration portable. World archive imports
retain interpreted members as their own sources. Use `adopt` to accept deliberate
changed bytes. A direct `url` normally requires expected content evidence:

```yaml
textures:
  url: https://example.org/textures.zip
  sha256: "EXPECTED_64_CHARACTER_SHA256"
  type: resource-pack
  side: client
```

When no independent digest is available, explicitly choose first-observed bytes:

```yaml
custom:
  url: https://example.org/custom.jar
  accept-observed: true
```

`accept-observed` applies only to URLs without `sha256` or `digests`. Both the short
`url` form and expanded `source: {kind: url, downloads: [...]}` require one of these
policies. The first accepted bytes establish an observed identity, not independent
authentication. Later sync retains matching locked evidence and rejects changed
bytes. `manifest explain` shows which evidence policy applies.

Optional `mirrors` must return the same asserted bytes. Hash mismatches stop the
operation. A provider pin cannot constrain a local file or direct URL.

## Selection preferences

```yaml
dependency-policy:
  releases: prefer-stable
dependencies:
  experimental:
    source: modrinth:PROJECT_ID
    releases: any
```

`prefer-stable` is the default. `stable-only` excludes provider-classified prereleases;
`any` gives them equal preference. These labels describe provider classifications,
not a guarantee that a mod works. Preferences govern new automatic choices; changing
them does not certify or refresh every retained or exact-pinned selection. Implicit required dependencies use the project
preference. `--release-policy` on update, outdated or migration overrides preferences
for that operation without rewriting authored policy. Exact pins remain exact regardless of project
preference. A pinned dependency cannot also declare its own release preference.

## Integration and outputs

Use `empack.bindings.yml` only for non-default destinations, stable file-role
selection, archive mappings or installation policy. Keep it in version control
alongside the manifest and lock. See [the model reference](design/model.md) for
advanced forms and per-side requirements.

`build.profiles` names outputs. The `prism` and `server` presets bundle pack content
as fixed snapshots in ZIP archives. Expanded profiles override individual options;
`empack build --explain` displays the exact selected recipes. Omit `build` until you
need outputs. The pack ID stays unchanged across renames and release labels.
`runtime.java` declares a minimum Java major, not compatibility with every newer JVM.
Initialization uses 21 unless you select `init --java MAJOR`; choose a value appropriate
for your Minecraft and loader combination.

Use `empack manifest check` for local validation, `manifest explain menu` for an
effective dependency view, and `manifest diff` to inspect authoring changes against
the available lock. The lock does not retain old display metadata or build profiles,
so this is a constraint comparison, not a historical manifest diff. These inspection
commands do not install or update content or contact providers.

Commands preserve unrelated comments and ordering. Existing block declarations remain
blocks when adding or removing a pin or changing a loader, source or build profile.
If an edited YAML value uses
anchors, flow collections or multiline syntax that cannot be changed safely, expand
that value into a block mapping and retry. Empack reports the unsupported edit instead
of silently reformatting the whole file.

## Editor completion

The repository includes [author schema 5](schemas/empack.schema-5.json) and
[bindings schema 1](schemas/empack.bindings.schema-1.json) as JSON Schemas. Configure
your YAML editor to associate these local files with `empack.yml` and
`empack.bindings.yml`. Keep both schemas in the same directory so their references
resolve without a network request.

Editor checks cover syntax, supported fields and source-specific options. Run
`empack manifest check` for cross-document references, shared optional choices,
portable paths and effective runtime/profile validation. Neither editor validation
nor manifest inspection verifies downloaded bytes or provider compatibility.

Inspection reports known YAML editing limitations with document paths. A valid flow
mapping, alias or multiline value can remain readable while requiring expansion
before a targeted edit. These warnings do not modify input or certify every other
possible edit. `manifest diff` checks automatic layout and explicit bindings with
the same root-satisfaction rules used by reconciliation.
