# Authoring troubleshooting

Use `empack status --json` for document and lock state, `manifest diff` for authored
changes, and `build --explain` for selected outputs. These inspections do not verify
installed payload bytes or prove Minecraft startup.

## Reference distributions

New projects use referenced Modrinth, Prism and server snapshots. Modrinth archives
work through a compatible launcher. Reference Prism/server exports require empack
on the recipient machine; their scripts acquire the exact released files and stop
when required inputs are missing. Restricted files remain explicit verified inputs.

Reference delivery can still include authored files and prepared runtime assets.
Review those assets before distribution. Choose `--delivery bundled` deliberately
when you want included dependency bytes. Existing profiles are preserved; schema-5
`client: prism` and `server: server` presets still mean bundled snapshots. To change
an existing project, write `target: prism` or `target: server` with explicit
`delivery: references`, run `sync`, and inspect `build --explain`.

## Partial assessments and retained selections

`outdated` separates the current, policy-permitted, compatible and upstream
selections. A newer beta can be compatible while `prefer-stable` retains a release.
An exact pin remains exact. A successful partial report does not mean every root
was assessed; `outdated --strict --json` gives automation a failing exit status for
incomplete evidence while preserving the report.

Malformed records leave their root unknown. Independent catalog checks continue
under the original allowance; throttling, authorization, deadlines and transport
failures stop further queries. Joint closure assessment requires complete inputs.
Update preparation still validates retained inventory and placements separately.

`update --continue-independent` isolates preparation/publication conflicts after
provider resolution succeeds. It does not isolate catalog failures. A catalog
failure publishes nothing. Use its named dependency to inspect the requested source,
pin, game and loader. Do not remove dependencies merely to make a report succeed.

A retained-dependent error names records that must be resolved in the same request.
Include those keys in the selected update only when you intend to assess them too.
Companion files need explicit roles and per-file participation in
`empack.bindings.yml`; selecting a primary file alone does not decide where a
companion resource pack belongs.

## Runtime and environment evidence

Forge and NeoForge remain distinct provider loader labels. Empack does not infer a
legacy compatibility exception from Minecraft 1.20.1 or accept all Forge files for
NeoForge. An exact, verified local/URL source can retain known working bytes, but
its integrity does not establish loader compatibility.

Provider game/loader declarations do not prove a mod's minimum loader requirement,
configuration compatibility or dedicated-server support. Authored participation
remains authoritative. Review client-only content before creating a server profile;
correct the dependency and any explicit binding together rather than deleting the
client payload. JAR layout inspection is not a semantic loader or side audit.

`outdated` assesses dependencies, not loader upgrades. Review the official loader
catalog and preview an explicit target before applying it:

```sh
empack migrate --minecraft 1.21.1 --loader neoforge --loader-version VERSION --plan
```

Use the project's actual game and loader, and retain any deliberate alternative-game
policy. Test runtime changes in an isolated copy with a disposable world. A zero
Java exit status alone is not a successful server startup; check readiness and fatal
loading errors. Archive verification and startup are separate checks from gameplay.

## Catalog deadlines

Provider work shares a bounded deadline. For a large pack, increase it explicitly:

```sh
empack --net-timeout 1200 sync
empack --net-timeout 1200 outdated --strict
```

Failed resolution leaves the published author project unchanged. Retrying performs
new catalog work; there is no implied saved catalog continuation or unlimited budget.

## Older author projects

Author schema 5, bindings schema 1 and lock schema 2 are supported. Reading an older
schema-2 lock with absent optional incompatibility evidence does not imply support
for lock schema 1. An empty incompatibility list is unknown historical evidence.
`migrate` changes the runtime request; it is not a document-schema converter.

Keep a full backup before recreating a manifest or re-importing a supported archive.
Work in a new author directory, preserve pins, bindings, sources and templates, and
verify the resulting inventories. An untracked file is not automatically safe to
replace because an older tool once generated it. Explicit adoption describes and
verifies existing files; it does not grant blanket overwrite authority. Do not run
schema reconstruction against a live installed instance or player world.
