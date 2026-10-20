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
that restriction without selecting a newer file. Human version names are display
labels, not dependency ranges.

A local `path` is the source file, not its installed destination. Use `adopt` to
accept deliberate changed bytes. A direct `url` requires expected content evidence:

```yaml
textures:
  url: https://example.org/textures.zip
  sha256: "EXPECTED_64_CHARACTER_SHA256"
  type: resource-pack
  side: client
```

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
not a guarantee that a mod works. Implicit required dependencies use the project
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
the available lock. These inspection commands do not install or update content.
