# Usage

Empack separates pack authoring from installed game instances. Use `--workdir` before the command to select the authoring project, instance or publisher directory. Relative input paths resolve from the directory where you invoked empack, not `--workdir`. This guide follows the current native CLI; `empack <command> --help` lists the options in your installed version.

## Reports, logs and exit status

Commands with `--json` reserve stdout for their report. Logs go to stderr for those commands even if `--log-output stdout` is configured. `--log-format` selects compact `text`, multiline `pretty` text or `json` log events, without changing the report schema; `--color always` does not add terminal escapes to JSON reports. Human-readable commands continue to honor the selected log destination.

Check the exit status as well as the report. A failed command can return a useful partial report, such as `update --all --json`; its diagnostic goes to stderr. A failure before a report is available does not produce a successful empty report. Cancellation requests do not replace the completed result: cleanup and recovery failures remain visible, and an operation that already completed successfully remains successful. Ctrl-C while answering a prompt cancels that decision; it does not grant approval. A later resume or recovery still requires fresh confirmation.

## Workspace status

Run `empack --workdir ./pack status` before choosing an author, publisher or instance operation. Add `--json` for the versioned report used by library clients, or select `--context author`, `publisher` or `instance` when a directory contains more than one context.

Status shows the pack, runtime and lock status, followed by any problems and next commands. Missing, invalid and out-of-date locks remain distinct; `--json` also includes the exact lock-document hash. Installed instances show their release and game directory. Inspection reads bounded control records without network requests, document rewrites, recovery or cache creation. Publisher storage is recognized but its contents are not authenticated by status. Pending records are reported as present; resuming still validates them and requests fresh approval. Interrupted publication must be inspected with `recover inspect` before live documents can be interpreted.

A successful status command means the inspection completed, not that its records are ready for a build or launch. In JSON schema 1, inspect `selected`, `contexts` and `findings`; `content_verified` is false. A valid author record can still have a missing, invalid or stale resolution lock. An ambiguous workspace has no automatic selection. `--context` chooses the context shown in the human report; JSON retains the other discovered contexts as well.

Status does not verify installed content, authenticate a channel or establish that Minecraft will launch. `update` previews author dependency changes and applies them with `--apply`; `instance update` previews a published pack release and applies it with `--apply`. Neither updates empack itself.

## Authoring

| Command                 | Meaning                                                                         |
| ----------------------- | ------------------------------------------------------------------------------- |
| `init`                  | Create an authoring project or import a supported archive                       |
| `add`                   | Add a new dependency, preserving exact source intent                            |
| `check`                 | Validate current authoring, recorded relationships and applicable local content |
| `manifest explain/diff` | Explain local authoring and compare recorded constraints without network access |
| `pin` / `unpin`         | Change exact provider selection intent without refreshing content               |
| `remove`                | Remove or demote selected logical roots with native ownership checks            |
| `sync`                  | Apply authored intent while retaining compatible locked selections              |
| `update`                | Check selected dependencies and preview or apply eligible updates               |
| `migrate`               | Plan or apply a Minecraft/loader change and explicit replacements               |
| `adopt`                 | Verify selected existing content and record it without hidden installation      |
| `build`                 | Produce explicitly selected consumer distributions                              |
| `recover`               | Inspect or finish/restore an interrupted publication                            |
| `clean`                 | Retire explicitly owned artifacts, disposable cache or continuation             |

Use exact dependency keys from `empack.yml` or `empack.lock` when selecting existing dependencies. `add` and `adopt --from` accept source names, provider URLs/IDs and paths to identify new content. Removal does not infer a key from a display title or provider string. Known dependents and file ownership still constrain what can be removed.

Initialization establishes `pack.id` once. Initialization recommends Java from the selected runtime; `init --java MAJOR` overrides that preference within the admitted policy. A display name or version change preserves the pack ID. See the [manifest guide](manifest.md) for source-specific declarations and author-only projects.

With explicit Minecraft and loader versions, initialization records those exact choices without consulting a catalog or running Java. Legacy Forge 1.7.10 loader versions may include the `-1.7.10` suffix; empack stores the canonical loader version. When a required version is omitted, initialization uses official catalog discovery. A chosen loader version is recorded in the lock without adding an authored pin. These steps do not prove that runtime downloads are available or that the server will start; build and launch verify their own requirements.

`init --force` and `init --from ARCHIVE --force` prepare a replacement before changing an existing project. Add `--dry-run` to inspect the affected files first. An invalid candidate or a declined plan leaves the existing project unchanged. An approved replacement can remove captured content under `pack/` and `overrides/`; it preserves existing custom templates, prior `dist/` artifacts and unrelated root files or directories. Worlds placed inside a replaced source root are author content in that scope; this command is not an installed-instance update.

Imported provider versions are recorded exactly in the lock without creating author pins. Ordinary `sync` keeps those selections; `update` can propose newer permitted versions, and `--apply` is required to apply them. Existing explicit author pins remain holds.

Imported overrides are recorded as local dependencies with accepted content in the lock. When you edit an imported file that is also its managed destination, `sync` and `update` protect the previously recorded bytes. Use `empack adopt KEY` with its exact dependency key from `empack.yml` to deliberately accept the edit, then build again. The approval preview compares the old and proposed versions, files, digests and requirements. Download sources are summarized by host; full URLs remain in the recorded data. This does not require changing the pack version. Custom source trees declared separately from dependencies retain their ordinary fresh-build behavior.

Modrinth public resolution needs no API key. CurseForge requests use the bundled client key. A nonempty `EMPACK_KEY_CURSEFORGE` overrides it; keep private overrides out of the manifest and version control. Resolution and referenced downloads require network access.

Edit `empack.yml` directly or use authoring commands. Common provider mods can use `label: modrinth:SELECTOR`; use a mapping for side, content kind or exact policy. Non-default role and placement decisions belong to optional `empack.bindings.yml`. Commit `empack.yml`, `empack.lock` and bindings when present to share reproducible author sources. Build requires satisfied intent and does not implicitly update dependencies. Removal of a manifest root does not grant permission to delete user files or required shared dependencies.

Provider selectors, explicit pins, typed local/URL content, named file plans and side placements normalize through the same engine. Ambiguous or unsupported inputs fail with actionable decisions. Default add/update batches publish nothing when any item fails. `--continue-independent` permits partial application only when complete dependency information and file relationships establish separate groups. Missing dependency information keeps the requests together; it does not block a whole operation when every request succeeds. Failed groups retain their previous state, and any partial application is reported.

## Checking and changing versions

```bash
empack update --all
empack update --all --json --quiet
empack update sodium --release-policy stable-only
empack update sodium iris --pin sodium=VERSION_ID --pin iris=VERSION_ID
empack update sodium iris --select
empack update sodium iris --apply
empack --dry-run migrate --minecraft 1.21.2 --loader fabric
empack --yes migrate --minecraft 1.21.2 --loader fabric
```

Select dependency names or `--all`. Use `--details` for catalog observations, `--quiet` to hide progress, or `--json` for a structured preview. The preview separates discovery from the prepared proposal and includes planned file content and attributes. A blocked or incomplete preview remains unsuccessful and can still return useful findings. JSON is a preview mode and cannot be combined with `--apply` or `--details`.

Update previews are the default; `--apply` is required to change the project, and `--yes` alone does not apply anything. A later `--apply` invocation prepares a fresh proposal; it does not execute a saved copy of the earlier preview. `--all` selects explicit author dependencies; retained libraries are checked as context rather than independently upgraded.

Ordinary updates retain author pins. `update KEY… --pin KEY=ID` explicitly changes selected provider author pins together, with the same approval and retained-dependent checks. An exact pin replaces that root's automatic release preference, as with `pin`. The preview separates pin edits from installed selection changes. The result reports changed and kept dependencies separately; checking a dependency does not mean it was upgraded. After a direct author/bindings edit, use `sync` to reconcile that intent instead. See [selection and canonical identity](design/authoring.md#selection-and-canonical-identity).

Use `update KEY… --select` to choose observed versions for named provider dependencies. The menu includes versions matching the pack's Minecraft and loader, labels prereleases and marks the installed version. Choosing deliberately pins that dependency and can override its automatic release preference. It still only previews unless `--apply` is present. The printed `--pin` command repeats the choice without reopening the menu. Selection requires an interactive terminal; it cannot be combined with `--yes`, `--all`, `--pin` or `--json`. Local and URL sources use their ordinary update path.

`update --all` shows version checks, available changes and problems alongside the prepared proposal. A targeted check such as `update sodium` also shows why that dependency is kept, including an exact pin. Use `--details` for the full list, version choices and check information; `--json` retains the complete structured report. `--quiet` hides progress on stderr. Unknown results mean a check could not finish, not that a dependency is up to date.

Version choices follow your Minecraft, loader, pins and release preferences. They do not establish that the whole pack works together. Empack checks required dependencies and reports known mod conflicts; update preparation checks the proposed files separately. See [assessment semantics](design/assessment.md) for the scope and limits of these checks.

A migration preview checks the candidate and native file changes while preserving the author files. Use `--dry-run --json` for a structured preview, including native blockers. Applying without `--dry-run` resolves again and requires approval of the native changes. Referenced payloads are verified when acquired; a metadata preview does not establish that those bytes are available or runnable. Valid locked selections stay selected. Use `--loader-version` for an exact loader and repeat `--accept-version` for explicitly accepted alternative game versions; the candidate replaces the previous alternative list. Replacing a project requires `--replace KEY=modrinth:CANONICAL_ID` or `KEY=curseforge:NUMERIC_ID` and deliberately resets that root to compatible version policy. It does not convert configurations, worlds or private files.

Add `--materialize` to acquire referenced bytes. Missing manual content can be saved and resumed with `pending resume ID --file KEY=PATH`. A changed author workspace invalidates the saved candidate. See the [assessment contract](design/assessment.md) for ranking, evidence limits and publication rules.

## Installed instances

Missing manual downloads can be saved after approval. The command reports the exact file key, byte count and SHA-256 address. Use `pending list`, then `pending resume ID --file KEY=PATH`; resuming checks the original assertions and instance base before showing a new plan. A preview never saves or removes pending state. If the base or publisher enrollment has changed, inspect it and explicitly discard the stale recipe before selecting a new operation.

| Command                                          | Meaning                                                                                    |
| ------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `instance install`                               | Install or apply an explicitly selected local snapshot into an instance root               |
| `instance ensure`                                | Launcher hook: install initially, then repair the active release without reverting updates |
| `instance update`                                | Check the subscribed channel and preview its update; use `--apply` to install              |
| `pending resume ID --file KEY=PATH`              | Resume a retained exact release with verified manual inputs                                |
| `pending discard ID`                             | Explicitly discard a pending recipe without changing the installation                      |
| `instance repair`                                | Restore the recorded release without selecting newer dependencies                          |
| `instance options`                               | Inspect/change persistent optional choices                                                 |
| `instance rollback`                              | Return managed content to a retained completed release                                     |
| `instance launch -- PROGRAM ARGS...`             | Verify completed content and run a locally selected runtime under an instance lease        |
| `instance check`                                 | Verify managed files, accepted overrides and saved state without repair                    |
| `instance inspect`                               | Show the completed release, saved choices and retained rollback releases                   |
| `instance subscribe` / `trust`                   | Enroll a channel and manage explicitly trusted publisher keys                              |
| `instance check-updates`                         | Authenticate and save a channel observation without installing content                     |
| `instance recover-runtime --acknowledge-stopped` | Clear runtime recovery evidence after all related processes have stopped                   |

`instance options` lists the completed release's saved values and alternatives. Use `instance options --choice KEY=VALUE` to change them. Newly enabled content uses retained asset locations, verified cache entries, explicit `--assets` or `--file` inputs, and exact provider sources. This command does not select a newer release; repair keeps the chosen values.

Instances default to snapshots. Channel following, publisher trust and offline launch policy require explicit configuration. Platform-managed packs remain under the platform updater; switching authority is deliberate. Empack tool updates are separate from pack updates.

Instance commands preserve worlds, operator data and unowned content. Modified managed files produce a conflict unless a bound explicit resolution is provided. A rollback changes managed pack content, not gameplay history.

A file conflict names the affected path and explains the available choices. `--preserve PATH` keeps an existing file as a local override; `--replace PATH` accepts the release version or its requested removal; `--merge PATH=FILE` uses a merged file you prepared. These paths are relative to `game/` or `.minecraft/`, so use `config/settings.txt`, not `game/config/settings.txt`. A missing file cannot be preserved or merged. Follow the named instance command to resolve conflicts before launch; `instance launch` does not accept conflict decisions. See [file ownership and decisions](design/instances.md#configuration-and-mutable-data) for the policy.

`instance check` verifies the completed release, managed file bytes and supported attributes, accepted overrides, layout and saved subscription state. It does not repair, download, launch, update a channel or clear recovery evidence. Seed files and initialized world content are player-owned and are not traversed or compared with publisher bytes. An active or unconfirmed managed runtime prevents a successful check; stop it and recheck, and inspect any remaining recovery evidence before acknowledging it. Use `--details` for individual checks, `--json` for the structured result, or `--quiet` to hide progress. Required failures return an error after reporting useful findings. This does not prove startup or gameplay compatibility.

`instance inspect` shows the installed pack, game directory, exact release, saved options, local overrides and rollback history. An update check shows the signed offer before approval. Saving it records the publisher's update number without installing files; that record remains if installation is later declined or fails. Installation has its own preview and approval.

## Check the current project

Use `empack check` to validate author files, bindings, the current lock, recorded dependency relationships and applicable local content assertions. This checks the selected state; it does not choose upgrades, repair files or create a new lock.

- `check --documents` checks author files and any present lock without reading pack content or contacting providers. Author-only work without a lock can pass this scope; missing resolution is reported. A stale or invalid present lock fails.
- `check --online` also fetches fresh metadata for every exact locked provider selection, including retained dependencies. It checks required relationships, participating incompatibilities and comparable file assertions. It does not search for newer versions, select missing dependencies or download pack content.
- Default and online checks require a current lock. Missing local sources or changed materialized bytes fail. Unmaterialized remote/manual references and unavailable manual dependency metadata are reported as unverified; they are not silently installed or treated as fresh evidence.

Required failures or incomplete required checks return an error after presenting useful findings. `--json` emits one structured report even when validation fails; diagnostics remain on stderr. `--details` includes successful checks and references, and `--quiet` hides progress. Documents and online scopes are mutually exclusive. None of these checks establishes startup or gameplay compatibility.

Use `manifest explain KEY` for one dependency's effective source, policy and placement, and `manifest diff` to compare authoring with the recorded lock. Use `update --all` to investigate a proposed upgrade instead of checking the current selections.

## Distribution selection

Define named output profiles in `empack.yml`. Each profile selects a consumer, delivery, environment, update owner and format. `build.default` selects which profiles a plain `empack build` builds together.

```yaml
build:
  default: [launcher]
  profiles:
    launcher:
      target: prism
      delivery: references
      environment: client
      updates: snapshot
      format: zip
    dedicated:
      target: server
      delivery: references
      environment: server
      updates: snapshot
      format: tar.gz
```

```sh
empack build --explain
empack build --profile launcher --explain --json
empack build --profile launcher --profile dedicated --optional-defaults
empack build prism --delivery references
empack build --ad-hoc empack --environment server --delivery references
```

A consumer argument selects its unique configured profile. If several profiles use that consumer, select a name. `--ad-hoc` explicitly starts from built-in snapshot policies with reference delivery for every consumer. `--delivery`, `--environment`, `--updates` and `--format` override selected values without editing the manifest. Modrinth and CurseForge require ZIP. Explanation shows each effective value and its origin, recipient requirements and known manual inputs; downloads and cache availability remain unverified.

Invalid policies and colliding output paths fail before acquisition. Continuation retains the effective profiles and rejects policy overrides or changed author inputs.

Prism consumes ordinary platform archives without an empack integration. A native Prism instance can instead carry launcher settings and explicitly subscribe through empack. Server recipes preserve exact runtime and startup requirements.

### Optional content and missing downloads

Materialized outputs need explicit optional-file choices. `--yes` approves a complete plan; it does not choose optional content. Use authored defaults or name a choice:

```sh
empack build prism --optional-defaults
empack build server --optional 'CHOICE=true'
```

Use the choice key printed by the command. Prism, server and CurseForge outputs support these selections. Modrinth/native-only builds reject the switches; mixed builds retain their authored participation while applying choices to the other outputs. `build --explain` shows each output's effective policy.

Reference exports may require `--allow-optional-metadata-loss` when their format cannot retain choice keys, defaults or descriptions. This acknowledges the stated conversion; it does not permit lost required files.

If acquisition requires a manual download, the command reports and saves the missing obligations after approval. Use the saved operation ID and exact file key printed by `pending show ID`:

```sh
empack pending list
empack pending show OPERATION_ID
empack pending resume OPERATION_ID --file 'EXACT_KEY=/absolute/path/to/download.jar'
```

Missing import files are reported with their exact selector and a local-file argument when representable. After saving the import, its Downloads-folder search is also available. Continuation keeps the original recipes and verifies the supplied bytes. Use `pending resume ID --file SELECTOR=PATH` for saved imports and installed releases; these identifiers are shown in the respective diagnostics.

## Interaction and failures

Without an interactive terminal, mutating plans require `--yes`. Piping `y` or `n` does not grant approval; closed input also returns a usage error with the next step. Use `--dry-run` to inspect a plan without approval. A resumed operation needs fresh approval even if its earlier save used `--yes`.

Initialization, import and instance previews identify the selected directory before approval.

Preview may resolve remote facts and use temporary scratch, but writes no project, instance, durable cache, subscription, tool installation or recovery record. Declining approval has the same preservation requirement. Unattended confirmation cannot invent missing choices or bypass integrity and ownership checks.

`empack doctor` reports the binary platform and locates Java on `PATH` without executing it or loading an author project. `--details` adds native capabilities and available build information. A missing Java is informational unless you request a check.

`doctor --java /path/to/java` runs a bounded local version probe; relative paths resolve from the invocation directory. Add `--minecraft 1.21.1` to compare that major with empack's recorded recommendation and admitted majors for that exact Minecraft version. Without `--java`, `--minecraft` checks the Java found on `PATH`. Unknown profiles remain unverified and make the requested check fail. Mods and loaders can impose stricter requirements; this is not a startup test. `--json` preserves the report on failure, and `--quiet` hides progress. Explicit probes cannot run with `--dry-run`. Managed server launch separately checks its selected Java against the pack's declared requirement after approval.

Manual inputs use exact obligation-to-path associations. Resume retains original assertions and rejects stale recipes. Browser opening and bounded download waiting are explicit options rather than side effects of confirmation.

Errors identify phase, affected object, expected/observed state, known effects and recovery action. Failure before publication differs from recovery required after possible effects. Completion is not reported until durable postconditions verify.

If publication was interrupted, inspect the selected root with the same state directory used by the interrupted command:

```sh
empack --workdir ./instance --state-dir ./state recover inspect
```

Recovery diagnostics print an inspection command retaining both selections. Inspection then prints the available commands with the operation ID. Commands are quoted for the labelled shell: Bash/zsh on Unix, PowerShell on Windows. They require `empack` on PATH and do not imply approval; review the plan before confirming or adding `--yes`. Paths that cannot be represented safely retain a diagnostic asking for the original native selections instead of a lossy copyable command.

`recover finish --operation ID` completes the verified candidate. For an interrupted publication that has not started restoring, `recover restore --operation ID` is an alternative that undoes its owned changes. Creation and an already-started restoration support finishing only. Keep both directory selections on subsequent commands. Recovery does not require valid author documents. External edits can require a decision; do not remove recovery records to force another install. Runtime recovery is separate: stop all associated processes before `instance recover-runtime --acknowledge-stopped`.

`clean builds` removes generated distributions, while `clean cache` retires disposable cached content. `clean all` means builds and cache; it does not erase author sources, installed worlds or recovery evidence. Preview the selected cleanup with `--dry-run`.

`clean continuation`, `clean import` and `clean sync` discard saved work for the selected project or import destination; retained input bytes remain. `clean retained` reclaims disposable inputs only from categories with no saved records. It preserves unknown neighboring files and recovery evidence, and refuses cleanup while a save owns the storage. These scopes are explicit; `clean all` does not select them. Stale saved work can be discarded without replaying its instructions. Other saved operations may still retain the same content category. Artifact cleanup respects runtime and publication ownership; an interrupted cleanup needs the usual recovery finish or restore before another artifact cleanup can proceed.

Workdir and relative input paths resolve from the directory where empack was invoked. Relative `--state-dir` and `--cache-dir` paths use that same directory, independently of `--workdir`. A relative initialization destination is a child of the selected workdir. State holds durable operation and recovery records; cache content is disposable.

Configuration precedence, from lowest to highest, is defaults, `.env`, `.env.local`, inherited environment variables, then explicit CLI options. Dotenv files are read from the invocation directory; selecting `--workdir` does not load that project's dotenv files. Quote paths containing spaces in dotenv files and pass each CLI path as one argument. The corresponding environment variables are `EMPACK_WORKDIR`, `EMPACK_STATE_DIR` and `EMPACK_CACHE_DIR`.

Command results and requested JSON use stdout. Checking messages, provisional catalog/acquisition progress, warnings and errors use stderr. Progress reports observed records or bytes, not an estimated completion percentage, and does not change the final result. `update --quiet` hides its progress while retaining reports and errors.

Help and version display flags remain available when unrelated dotenv files are malformed. Ordinary commands, including `doctor` and an invocation with no command, load configuration and report malformed dotenv as an error. With valid configuration, invoking no command prints help guidance without creating project or host state.

## Native release example

An instance release is an exact pack snapshot. Building or installing one locally does not publish it or create a GitHub release. The pack version, native release hash and empack engine version identify different things.

Set stable pack identity and runtime requirements in `empack.yml`:

```yaml
schema: 6
pack:
  id: my-pack
  name: My Pack
  version: "Autumn Update"
runtime:
  minecraft: "1.21.1"
  loader: neoforge
  java: {preferred: 21}
build:
  default: [native]
  profiles:
    native:
      target: empack
      delivery: bundled
      environment: both
      updates: snapshot
      format: zip
sources:
  exclude:
    - private/**
```

For custom installation policy, add `empack.bindings.yml`:

```yaml
schema: 1
bindings: {}
policies:
  config/server-defaults.toml: seed
```

`empack build --profile native` writes `dist/<name>-<version>-empack-bundled.empack` using the configured archive format. Reference delivery uses an `empack-references` suffix. Native recipes share verified acquisition, missing-download continuation and combined publication with the other consumers. They preserve optional choices for installation. Extract the archive, then select its `release.json` with `instance install` and the payload SHA-256 printed by the build. The digest names the JSON payload, not the surrounding archive. Side selection applies the matching override layer. Local snapshot selection does not enroll a publisher.

```sh
mkdir -p ./instance
empack --workdir ./instance instance install ./export/release.json --sha256 PAYLOAD_SHA256
empack --workdir ./instance instance inspect
```

Select an existing instance directory. `./export` is the extracted native archive; replace `PAYLOAD_SHA256` with the payload digest printed by the build. Assets next to `release.json` are discovered automatically. Add `--side server` for a server instance. The default `game` layout stores game content under `game/`; `--layout prism` uses `.minecraft/` and writes launcher components. Normal Prism users should import a Prism ZIP instead of constructing that layout manually. Applying another local snapshot uses `instance install` with its new digest.

Publisher enrollment is explicit and separate from snapshot installation:

```sh
empack --workdir instance instance subscribe --pack my-pack --channel stable https://packs.example.org/channels/stable.json --key PUBLIC_KEY_HEX
empack --workdir instance instance update
empack --workdir instance instance update --apply
```

Enrollment saves the channel URL and trusted keys. `instance update` fetches and verifies its latest offer without changing saved history or installed content. If the offer has not been saved, the result identifies the release and explains that an installation plan is not yet prepared. When the exact offer is already saved, the command can preview its file changes. `--yes` alone never applies an update.

`instance update --apply` asks separately to save the authenticated offer and to install the prepared release. Saving the offer advances replay protection even if later acquisition or installation fails or is declined. `--dry-run` prevents both changes. Use `instance update --channel-file CHANNEL_ENVELOPE` to select a signed local offer while still fetching its exact release and remote assets. It has the same preview/apply and trust rules and never changes the enrolled URL. To save an offer without installing it, use `instance check-updates [CHANNEL_ENVELOPE]`; without a path it fetches the enrolled HTTPS channel. `instance update [RELEASE_ENVELOPE]` is the offline alternative: it verifies a local signed release against the already saved offer without fetching a new channel. It still requires `--apply` to install.

Updates infer the validated installed side and layout; explicit conflicting options fail before acquisition. First installation defaults to client/game and prints that selection; choose `--side server` for a new server. Local assets, verified cache entries and exact download sources supply its files. Remote immutable assets resolve relative to the authenticated release URL. `--file KEY=PATH` supplies restricted content. A preview requiring manual files stops without saving waiting work; `--apply` permits saving it for `pending resume`. For Prism layouts, content application also updates the exact game/loader component profile in the same publication. Runtime-changing updates require stopping Prism and restarting it after application so it reloads the component metadata.

For an installed, explicitly enrolled **server export**, a local Java runtime can request prelaunch updates. This requires the managed entry point produced by `build server`; a generic `build empack` content archive does not provide it:

```sh
empack --workdir instance --yes instance launch --server --check-updates --allow-offline -- /absolute/path/to/java nogui
```

Omit `--allow-offline` to require a successful channel check. Fallback applies only to channel connection failures or timeouts. Invalid signatures, revoked keys, HTTP errors and failed installation stop launch. Prism runtime changes require a launcher restart; managed servers publish their new runtime before launch. The saved channel floor precedes release acquisition. Preview or declined approval stops the sequence; launch still verifies the completed files and acquires its runtime lease. Java remains locally selected; `--server` uses the completed release entry point. Without `--check-updates`, launch uses the completed release without consulting a channel.

Choose `empack build prism --updates empack` or `empack build server --updates empack` to export a consumer that requires an enrolled channel. Both reference and bundled delivery are supported. Enroll with `instance subscribe` in the extracted instance before its first launch. The generated consumer checks for updates; it never copies publisher keys into local trust. Managed server updates include runtime files; Prism runtime changes require a stopped-launcher update and restart. Snapshot recipes remain the default. Native archives support `build empack --updates empack`; their release payload enforces the same explicit enrollment requirement.

## Publisher hosting

Use `release init` for signing-key setup, `release stage` for immutable release files and `release channel create` for a local signed channel file after verifying the hosted release. These commands do not upload files. See [publishing](publishing.md) for the deployment sequence. Publishers share public keys through a trusted route; subscribers never need a private signing key.

For an empack-managed server release, run `empack instance launch --server -- java nogui`. Java is selected from your local PATH; the completed release supplies the verified JAR or loader argument-file path. Add `--check-updates` before `--` for an explicitly enrolled subscription. Changing Java or updating empack remains a local operator action.

`build modrinth --updates platform` and `build curseforge --updates platform` produce platform-targeted archives. They do not upload a project or associate a recipient's instance with one. Upload the artifact and install it through the platform's project/version interface to obtain platform updates. Importing the archive directly remains a snapshot. Modrinth platform exports reject unsupported hosting download domains; ordinary snapshot mrpacks may still use other HTTPS hosts.

## Shared pending inputs

Use the selected workspace or intended import destination for all four workflows:

```sh
empack --workdir my-pack pending list
empack --workdir my-pack pending list --json
empack --workdir my-pack pending show OPERATION_ID
empack --workdir my-pack pending show OPERATION_ID --json
empack --workdir my-pack --dry-run pending resume OPERATION_ID --file EXACT_KEY=/path/to/file
empack --workdir my-pack pending resume OPERATION_ID --file EXACT_KEY=/path/to/file
empack --workdir my-pack pending resume OPERATION_ID --downloads-dir /path/to/Downloads --open-downloads
empack --workdir my-pack pending resume BUILD_OPERATION_ID --downloads-dir /path/to/Downloads --wait-downloads 60
empack --workdir imported-pack pending resume OPERATION_ID --file EXACT_KEY=/path/to/file --exclude-auxiliary --java 21
empack --workdir my-pack pending discard OPERATION_ID
```

`pending list` summarizes saved work; `pending show ID` lists the recorded files, names, sizes and checksums. Add `--json` for the complete report, including recorded download addresses. Changed or unreadable records remain visible with their diagnostic. IDs accept unique prefixes and remain bound to the selected root and saved-record bytes. List/show do not fetch content or modify state. Resume verifies supplied bytes and asks for fresh workflow approval; discard conditionally removes only the selected recipe after approval. Preview never opens browsers or scans Downloads. Exact keys, unknown metadata and bounded assistance are described in [the pending-input contract](design/pending.md).

## Generated GitHub workflows

New projects include validation and release workflows plus `.github/scripts/empack-ci.py`. They build the profiles in `build.default`. Set `EMPACK_PROFILES` in both workflows to select explicit names, one per line. Both workflows apply authored optional defaults. Set `JAVA_VERSION` for your runtime; the generated value is the selected Java preference.

Commit coherent authoring documents, bindings and lock before running CI. A release tag must be `v` followed by the version in `empack.yml`. The release attaches every selected distribution and `SHA256SUMS`, using normalized download filenames. It does not upload to provider platforms. Existing workflow files are not automatically upgraded.

Reference Prism exports and all `updates: empack` Prism profiles require `OverrideCommands=true` and the generated preparation/wrapper commands. Keep exactly one `ConfigVersion=1.2` or `ConfigVersion=1.3` in `[General]`, with these command keys only in that section. Legacy, duplicated or differently spelled authority settings are rejected. Keep values on one line with balanced quotes. Use `;` for comment lines; hash-prefixed lines are rejected because Prism can interpret them as settings. Use ordinary spaces or tabs around keys and LF or CRLF line endings. Non-breaking spaces and other Unicode whitespace are not indentation or key padding; Unicode remains allowed in names and other user-facing values. Keep the `INSTANCE_PREPARE_COMMAND` and `INSTANCE_WRAPPER_COMMAND` template expressions when customizing `instance.cfg`; unrelated names, icons and memory settings are editable. A template that removes required integration fails preparation without being rewritten. For server entry points, see [custom scripts](servers.md#existing-exports-and-custom-scripts).

Prism inherits the recipient’s launcher memory settings by default. To set an instance heap, add `OverrideMemory=true`, `MinMemAlloc=512` and an appropriate `MaxMemAlloc` value in MiB to `templates/client/instance.cfg.template`.

See [project automation](design/automation.md) for installation verification, failure handling and the distinction between GitHub releases and native signed channels.

Server exports start without the Minecraft GUI through `start.sh` or `start.bat`. Use `start_gui.sh` or `start_gui.bat` to request the GUI. See [running a server](servers.md) for Java, installation, EULA handling and Windows errors.
