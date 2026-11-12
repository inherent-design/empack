# Authoring troubleshooting

Use `empack status --json` for document and lock state, `manifest diff` for authored changes, and `build --explain` for selected outputs. These inspections do not verify installed payload bytes or prove Minecraft startup.

## Saved operations need review

Saved operation records retain interrupted changes and runtime ownership. They are separate from your author files, lock, Minecraft settings and disposable download cache. If empack cannot match older records safely to the current project directory, it reports the project and affected record directories and refuses the operation.

Keep those directories. Use the empack version that created them to inspect unfinished work and complete or restore it where supported. If that version cannot establish the association either, preserve the records for manual recovery review. Updating mod versions does not repair this association. A separate `--state-dir` selects different operation records; it does not recover the originals or establish that bypassing them is safe for writes.

## After an interruption or damaged saved file

Keep the project or instance together with its selected `--state-dir`. Before trying another change, read the reported operation and cause. A stopped command, a stopped game and a completed file change are different outcomes; an error can include work that already completed.

| What empack reports                                  | What to do                                                                                                                                                                                                                                                                                                                     |
| ---------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| A file change needs recovery                         | Use `recover inspect` with the reported project and state directories. Review the proposed files before `recover finish --operation ID`, or `recover restore --operation ID` when offered. [Recovery and cleanup](usage.md#interaction-and-failures) explains the available actions.                                           |
| Saved work needs files                               | Use `pending show ID`, provide the exact requested files and resume with fresh approval. A checksum mismatch is not permission to accept different bytes. Discard only work you deliberately abandon.                                                                                                                          |
| Runtime retirement is unconfirmed                    | Stop all associated game/runtime processes first. Then use `instance recover-runtime --acknowledge-stopped`. An absent parent process or a reboot is not an automatic acknowledgement.                                                                                                                                         |
| The runtime retired, but terminal restoration failed | Read the console-cleanup diagnostic. This alone does not mean processes are still running or require runtime acknowledgement; a separate ownership-cleanup error does.                                                                                                                                                         |
| An instance or subscription record is unreadable     | Preserve `.empack`, the instance and its host state. Do not delete the record to reinstall or re-enroll: it holds ownership, local choices or publisher history. `instance repair` requires valid completed metadata; it cannot reconstruct lost history. Review a consistent backup rather than synthesizing an empty record. |
| A disposable content cache is unavailable            | Empack may reacquire exact content from its source. If the source is also unavailable, supply the requested verified file. Explicit cache cleanup is separate from saved inputs and recovery records.                                                                                                                          |
| A registered bootstrap engine is missing or corrupt  | Keep the diagnostic and inspect the selected private engine cache. The bootstrap refuses to execute or silently replace that registered object. This differs from an interrupted private download, which it can safely retry. See [engine cache](design/bootstrap.md#official-acquisition-and-cache).                          |

For a disk-full or permission failure, correct the storage problem before retrying inspection. Do not remove journals or before/after copies to free space; they can be needed to complete or restore an interrupted change. Recovery checks the current files again and refuses conflicting external edits. If a record is malformed, unsupported or missing its backing data, preserve the evidence for review rather than forcing an action whose result cannot be established.

## Reference distributions

New projects use referenced Modrinth, Prism and server snapshots. Modrinth archives work through a compatible launcher. Managed Prism/server exports carry the [bootstrap JAR](design/bootstrap.md), which selects their exact native empack version locally, from its cache or by verified download. Java must already be available. Preparation acquires the exact released files and stops when required inputs are missing; restricted files remain explicit verified inputs.

Reference delivery can still include authored files and prepared runtime assets. Review those assets before distribution. Choose `--delivery bundled` deliberately when you want included dependency bytes. Existing profiles are preserved; schema-6 `client: prism` and `server: server` presets still mean bundled snapshots. To change an existing project, write `target: prism` or `target: server` with explicit `delivery: references`, run `sync`, and inspect `build --explain`.

## Player options reset after a launcher update

Check which updater applied the change. A mrpack's `options.txt` is an override file, even when the author project marks it `seed`. The format cannot convey that ownership policy to the launcher. See [Modrinth output](design/builds.md#modrinth) for the format boundary.

Empack preparation and updates preserve an existing regular file when the incoming native release marks it `seed`; a missing file receives the released default. Use that empack-managed workflow when relying on seed ownership. A launcher applying mrpack overrides follows its own update rules. Back up player settings before changing update workflows; omitting a previously tracked override from a later archive is not a preservation guarantee.

## Partial assessments and retained selections

`outdated` separates the current, policy-permitted, Minecraft/loader-compatible and upstream selections. A newer beta can match the runtime while `prefer-stable` retains a release. An exact pin remains exact. A successful partial report does not mean every root was assessed; `outdated --strict --json` gives automation a failing exit status for incomplete evidence while preserving the report.

Malformed records leave their root unknown. Independent catalog checks continue under the original allowance; throttling, authorization, deadlines and transport failures stop further queries. Joint closure assessment identifies the roots blocking it. Update preparation still validates retained inventory and placements separately. Use `outdated KEY…` to check named dependencies and `--details` for the full list and version choices; the full JSON report identifies its scope. Field/invariant diagnostics describe malformed eligible evidence without dumping provider responses. See [assessment semantics](design/assessment.md).

`update --continue-independent` isolates preparation/publication conflicts after input resolution succeeds. It does not isolate catalog failures or malformed local sources that fail before a complete candidate is available. Those failures publish nothing. Use the named dependency to inspect the requested source, pin, game and loader. Do not remove dependencies merely to make a report succeed.

A retained-dependent error names records that must be resolved in the same request. Include those keys in the selected update only when you intend to assess them too. Repeated `--pin KEY=ID` explicitly selects reviewed provider versions for that group; ordinary updates retain exact author pins. If you already edited author files or bindings, use `manifest diff` and `sync` to reconcile that intent first. See [selection and canonical identity](design/authoring.md#selection-and-canonical-identity). Companion files need explicit roles and per-file participation in `empack.bindings.yml`; selecting a primary file alone does not decide where a companion resource pack belongs.

## Runtime and environment evidence

Malformed `.env` or `.env.local` files stop configuration loading with the filename and syntax position. Diagnostics omit the source line because it may contain a credential. Inspect and correct that file locally; do not paste its contents into a public report.

Forge and NeoForge remain distinct provider loader labels. Empack does not infer a legacy compatibility exception from Minecraft 1.20.1 or accept all Forge files for NeoForge. An exact, verified local/URL source can retain known working bytes, but its integrity does not establish loader compatibility.

Provider game/loader declarations do not prove a mod's minimum loader requirement, configuration compatibility or dedicated-server support. Authored participation remains authoritative. Review client-only content before creating a server profile; correct the dependency and any explicit binding together rather than deleting the client payload. JAR layout inspection is not a semantic loader or side audit.

`outdated` assesses dependencies, not loader upgrades. Review the official loader catalog and preview an explicit target before applying it:

```sh
empack migrate --minecraft 1.21.1 --loader neoforge --loader-version VERSION --plan
```

Use the project's actual game and loader, and retain any deliberate alternative-game policy. Test runtime changes in an isolated copy with a disposable world. A zero Java exit status alone is not a successful server startup; check readiness and fatal loading errors. Archive verification and startup are separate checks from gameplay.

## Catalog deadlines

Provider work shares a bounded deadline. For a large pack, increase it explicitly:

```sh
empack --net-timeout 1200 sync
empack --net-timeout 1200 outdated --strict
```

Failed resolution leaves the published author project unchanged. Retrying performs new catalog work; there is no implied saved catalog continuation or unlimited budget.

## Older author projects

Author schema 6, bindings schema 1, lock schema 3 and native release schema 2 are supported. Older author, lock and native release formats are rejected rather than reinterpreted. Preserve interrupted work and use its original compatible executable to finish or recover it before creating current-format state. `migrate` changes the runtime request; it is not a document-schema converter.

Keep a full backup before recreating a manifest or re-importing a supported archive. Work in a new author directory, preserve pins, bindings, sources and templates, and verify the resulting inventories. An untracked file is not automatically safe to replace because an older tool once generated it. Explicit adoption describes and verifies existing files; it does not grant blanket overwrite authority. Do not run schema reconstruction against a live installed instance or player world.

## Bootstrap encoding appendix

Keep `.empack-tools` intact when moving a managed distribution. Generated commands select its JAR and UTF-8 invocation files through relative paths and set `-Dfile.encoding=UTF-8` for the bootstrap only. Pack names and other metadata remain Unicode data; a non-English display name is not itself an invalid pack.

External command-line arguments also depend on the JVM's operating-system conversion. On Unix, use an available UTF-8 execution locale for Unicode launcher arguments, such as `C.UTF-8` where installed. The traditional ASCII `C` locale remains suitable for ordinary ASCII invocations; it does not make every external JVM argument Unicode-safe. Reading an explicitly UTF-8 file and decoding the process command line are separate operations.

On Windows, a UTF-8 console does not imply a UTF-8 system ANSI code page. Some Java launchers can replace characters outside that code page before the JAR receives them, even with `-Dfile.encoding=UTF-8`. Relative generated paths and UTF-8 static inputs avoid exposing many instance-directory names to that conversion, but do not establish lossless handling of arbitrary custom Prism/JVM arguments. The bootstrap cannot reconstruct replaced characters or reliably distinguish replacement from a legitimate question mark. It does not change global regional settings or infer that a user's name is invalid.

Missing or malformed invocation files produce ordinary input errors before delegation. Preserve the complete distribution and compare its generated files before changing them; runtime credentials do not belong in these static files. [Bootstrap invocation and selection](design/bootstrap.md#static-invocation-files) describes the file boundary. Native engine errors, game output, consumer import and gameplay remain separate evidence.
