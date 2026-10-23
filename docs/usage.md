# Usage

Empack separates pack authoring from installed game instances. Use `--workdir` before
the command to select the authoring project, instance or publisher directory. Relative
input paths resolve from the directory where you invoked empack, not `--workdir`.
This guide follows the current native CLI; `empack <command> --help` lists the
options in your installed version.

## Workspace status

Run `empack --workdir ./pack status` before choosing an author, publisher or instance
operation. Add `--json` for the versioned report used by library clients, or select
`--context author`, `publisher` or `instance` when a directory contains more than
one context.

Status reads bounded control records without network requests, document rewrites,
recovery or cache creation. It distinguishes missing, malformed and stale author
locks and shows the human pack version, exact release, runtime and game directory
when those records are available. Publisher storage is recognized but its contents
are not authenticated by status. Pending records are reported as present; resuming
still validates them and requests fresh approval. Interrupted publication must be
inspected with `recover inspect` before live documents can be interpreted.

A successful status command means the inspection completed, not that its records
are ready for a build or launch. In JSON schema 1, inspect `selected`, `contexts`
and `findings`; `content_verified` is false. A valid author record can still have a
missing, invalid or stale resolution lock. An ambiguous workspace has no automatic
selection. `--context` chooses the context shown in the human report; JSON retains
the other discovered contexts as well.

Status does not verify installed content, authenticate a channel or establish that
Minecraft will launch. `update` changes author dependency selections;
`instance update` applies a published pack release. Neither updates empack itself.

## Authoring

| Command | Meaning |
| --- | --- |
| `init` | Create an authoring project or import a supported archive |
| `add` | Describe and resolve dependencies, preserving exact source intent |
| `manifest check/explain/diff` | Inspect local authoring and recorded constraints without network access |
| `pin` / `unpin` | Change exact provider selection intent without refreshing content |
| `remove` | Remove or demote selected logical roots with native ownership checks |
| `sync` | Apply authored intent while retaining compatible locked selections |
| `outdated` | Read provider update evidence and joint dependency blockers |
| `update` | Deliberately refresh selected eligible dependency versions |
| `migrate` | Plan or apply a Minecraft/loader change and explicit replacements |
| `adopt` | Verify selected existing content and record it without hidden installation |
| `build` | Produce explicitly selected consumer distributions |
| `recover` | Inspect or finish/restore an interrupted publication |
| `clean` | Retire explicitly owned artifacts, disposable cache or continuation |

Initialization establishes `pack.id` once. `init --java MAJOR` declares the minimum
recipient Java version (default 21); adjust it for your game and loader. A display
name or version change preserves the pack ID. See the [manifest guide](manifest.md)
for source-specific declarations and author-only projects.

Modrinth public resolution needs no API key. For CurseForge, provide your client key
through `EMPACK_KEY_CURSEFORGE`; keep credentials out of the manifest and version
control. Resolution and referenced downloads require network access.

Edit `empack.yml` directly or use authoring commands. Common provider mods can use
`label: modrinth:SELECTOR`; use a mapping for side, content kind or exact policy.
Non-default role and placement decisions belong to optional `empack.bindings.yml`.
Commit `empack.yml`, `empack.lock` and bindings when present to share reproducible
author sources. Build requires satisfied intent and
does not implicitly update dependencies. Removal of a manifest root does not grant
permission to delete user files or required shared dependencies.

Provider selectors, explicit pins, typed local/URL content, named file plans and side
placements normalize through the same engine. Ambiguous or unsupported inputs fail
with actionable decisions. Default add/update batches publish nothing when any item
fails. Explicit independent batches preserve failed groups and report partial effects.

## Assessing and changing versions

```bash
empack outdated --strict --json --release-policy prefer-stable
empack --dry-run update sodium --release-policy stable-only
empack migrate --minecraft 1.21.2 --loader fabric --plan
empack --yes migrate --minecraft 1.21.2 --loader fabric
```

`outdated` distinguishes the locked version, policy-permitted candidate, compatible
candidate and newest observed upstream selection. Unavailable evidence is reported
as unknown, not up to date. The required-dependency closure is checked separately
from each row. Known incompatibility declarations also include retained selections,
but the report does not decide whether their final client/server placements overlap.
A limited declaration scan retains the available rows and marks its list incomplete.
Candidate compatibility remains `not-assessed` until update preparation. Neither
assessment nor preparation proves that configurations or gameplay will work.

A migration plan leaves the author files unchanged. Applying the same command
without `--plan` resolves again and requires approval of the native changes. Valid
locked selections stay selected. Use `--loader-version` for an exact loader and
repeat `--accept-version` for explicitly accepted alternative game versions; the
candidate replaces the previous alternative list. Replacing a project requires
`--replace KEY=modrinth:CANONICAL_ID` or `KEY=curseforge:NUMERIC_ID` and deliberately
resets that root to compatible version policy. It does not convert configurations,
worlds or private files.

Add `--materialize` to acquire referenced bytes. Missing manual content can be
saved and resumed with `pending resume ID --file KEY=PATH`. A changed author
workspace invalidates the saved candidate. See the [assessment contract](design/assessment.md)
for ranking, evidence limits and publication rules.

## Installed instances

Missing manual downloads can be saved after approval. The command reports the exact
file key, byte count and SHA-256 address. Use `pending list`, then
`pending resume ID --file KEY=PATH`; resuming checks the original assertions and
instance base before showing a new plan. `instance continue --file KEY=PATH` remains
available for an instance-specific continuation.
A preview never saves or removes pending state. If the base or publisher enrollment
has changed, inspect it and explicitly discard the stale recipe before selecting a
new operation.

| Command | Meaning |
| --- | --- |
| `instance install` | Install or apply an explicitly selected local snapshot into an instance root |
| `instance prepare` | Launcher hook: install initially, then repair the active release without reverting updates |
| `instance update` | Apply the exact signed release selected by the saved subscribed channel |
| `instance continue --file KEY=PATH` | Resume a retained exact release with verified manual inputs |
| `instance discard-pending` | Explicitly discard a pending recipe without changing the installation |
| `instance repair` | Restore the recorded release without selecting newer dependencies |
| `instance options` | Inspect/change persistent optional choices |
| `instance rollback` | Return managed content to a retained completed release |
| `instance launch -- PROGRAM ARGS...` | Verify completed content and run a locally selected runtime under an instance lease |
| `instance inspect` | Show the completed release, saved choices and retained rollback releases |
| `instance subscribe` / `trust` | Enroll a channel and manage explicitly trusted publisher keys |
| `instance check-updates` | Authenticate and save a channel observation without installing content |
| `instance recover-runtime --acknowledge-stopped` | Clear runtime recovery evidence after all related processes have stopped |

`instance options` lists the completed release's saved values and alternatives.
Use `instance options --choice KEY=VALUE` to change them. Newly enabled content
uses retained asset locations, verified cache entries, explicit `--assets` or
`--file` inputs, and exact provider sources. This command does not select a newer
release; repair keeps the chosen values.

Instances default to snapshots. Channel following, publisher trust and offline launch
policy require explicit configuration. Platform-managed packs remain under the
platform updater; switching authority is deliberate. Empack tool updates are separate
from pack updates.

Instance commands preserve worlds, operator data and unowned content. Modified
managed files produce a conflict unless a bound explicit resolution is provided.
A rollback changes managed pack content, not gameplay history.

## Distribution selection

Define named output profiles in `empack.yml`. Each profile selects a consumer,
delivery, environment, update owner and format. `build.default` selects
which profiles a plain `empack build` builds together.

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

A consumer argument selects its unique configured profile. If several profiles use
that consumer, select a name. `--ad-hoc` explicitly starts from built-in snapshot
policies with reference delivery for every consumer.
`--delivery`, `--environment`, `--updates` and `--format` override selected values
without editing the manifest. Modrinth and CurseForge require ZIP. Explanation
shows each effective value and its origin, recipient requirements and known manual
inputs; downloads and cache availability remain unverified.

Invalid policies and colliding output paths fail before acquisition. Continuation
retains the effective profiles and rejects policy overrides or changed author inputs.

Prism consumes ordinary platform archives without an empack integration. A native
Prism instance can instead carry launcher settings and explicitly subscribe through
empack. Server recipes preserve exact runtime and startup requirements.

### Optional content and missing downloads

Materialized outputs need explicit optional-file choices. `--yes` approves a complete
plan; it does not choose optional content. Use authored defaults or name a choice:

```sh
empack build prism --optional-defaults
empack build server --optional 'CHOICE=true'
```

Use the choice key printed by the command. Reference exports may require
`--allow-optional-metadata-loss` when their format cannot retain choice keys, defaults
or descriptions. This acknowledges the stated conversion; it does not permit lost
required files.

If acquisition requires a manual download, the command reports and saves the missing
obligations after approval. Supply the exact file using the name it reports:

```sh
empack build --continue --associate-download 'FILENAME=/absolute/path/to/download.jar'
```

Continuation keeps the original recipes and verifies the supplied bytes. Use
`--import-file SELECTOR=PATH` with `init --continue` for imports, or
`instance continue --file KEY=PATH` for installed releases; these identifiers are
shown in the respective diagnostics.

## Interaction and failures

Preview may resolve remote facts and use temporary scratch, but writes no project,
instance, durable cache, subscription, tool installation or recovery record. Declining
approval has the same preservation requirement. Unattended confirmation cannot invent
missing choices or bypass integrity and ownership checks.

Manual inputs use exact obligation-to-path associations. Resume retains original
assertions and rejects stale recipes. Browser opening and bounded download waiting
are explicit options rather than side effects of confirmation.

Errors identify phase, affected object, expected/observed state, known effects and
recovery action. Failure before publication differs from recovery required after
possible effects. Completion is not reported until durable postconditions verify.

If publication was interrupted, inspect the selected root first:

```sh
empack --workdir ./instance recover inspect
```

Use the reported operation ID with either `recover finish --operation ID` to finish
the verified candidate or `recover restore --operation ID` to undo that operation's
owned changes. These are alternative actions. External edits can require a decision;
do not remove recovery records to force another install. Runtime recovery is separate:
stop all associated processes before `instance recover-runtime --acknowledge-stopped`.

`clean builds` removes generated distributions, while `clean cache` retires disposable
cached content. `clean all` means builds and cache; it does not erase author sources,
installed worlds or recovery evidence. Preview the selected cleanup with `--dry-run`.

Workdir and relative input paths resolve from the directory where empack was invoked.
Relative `--state-dir` and `--cache-dir` paths use that same directory, independently
of `--workdir`. A relative initialization destination is a child of the selected
workdir. State holds durable operation and recovery records; cache content is disposable.

Configuration precedence, from lowest to highest, is defaults, `.env`, `.env.local`,
inherited environment variables, then explicit CLI options. Dotenv files are read from
the invocation directory; selecting `--workdir` does not load that project's dotenv
files. Quote paths containing spaces in dotenv files and pass each CLI path as one
argument. The corresponding environment variables are `EMPACK_WORKDIR`,
`EMPACK_STATE_DIR` and `EMPACK_CACHE_DIR`.

Help and version display flags remain available when unrelated dotenv files are
malformed. Ordinary commands, including `version` and an invocation with no command,
load configuration and report malformed dotenv as an error. With valid configuration,
invoking no command prints help guidance without creating project or host state.

## Native release example

Set stable pack identity and runtime requirements in `empack.yml`:

```yaml
schema: 5
pack:
  id: my-pack
  name: My Pack
  version: "Autumn Update"
runtime:
  minecraft: "1.21.1"
  loader: neoforge
  java: 21
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

`empack build --profile native` writes
`dist/<name>-<version>-empack-bundled.empack` using the configured archive format.
Reference delivery uses an `empack-references` suffix. Native recipes share verified
acquisition, missing-download continuation and combined publication with the other
consumers. They preserve optional choices for installation. Extract the archive, then select its `release.json` with
`instance install` and the payload SHA-256 printed by the build. The digest names
the JSON payload, not the surrounding archive. Side selection applies the matching
override layer. Local snapshot selection does not enroll a publisher.

```sh
mkdir -p ./instance
empack --workdir ./instance instance install ./export/release.json --sha256 PAYLOAD_SHA256
empack --workdir ./instance instance inspect
```

Select an existing instance directory. `./export` is the extracted native
archive; replace `PAYLOAD_SHA256` with the payload digest printed by the build.
Assets next to `release.json` are discovered automatically. Add `--side server` for
a server instance. The default `game` layout stores game content under `game/`;
`--layout prism` uses `.minecraft/` and writes launcher components. Normal Prism
users should import a Prism ZIP instead of constructing that layout manually.
Applying another local snapshot uses `instance install` with its new digest.

Publisher enrollment is explicit and separate from snapshot installation:

```sh
empack --workdir instance instance subscribe --pack my-pack --channel stable https://packs.example.org/channels/stable.json --key PUBLIC_KEY_HEX
empack --workdir instance instance check-updates
empack --workdir instance instance update
```

Enrollment and observation save trust and authenticated sequence observations.
`instance check-updates` fetches the enrolled HTTPS URL and saves the verified
observation after approval. `instance update` fetches the exact signed release
selected by that saved channel. Both commands also accept a local envelope path.
Update checks current trust, expiry and exact release identity before applying
content. Updates infer the validated installed side and layout; explicit conflicting
options fail before acquisition. First installation defaults to client/game and
prints that selection; choose `--side server` for a new server. `instance update
--check` composes a fresh observation and update through separate approvals. Check
approval persists the anti-replay floor even if installation never follows;
preview saves nothing and never authorizes a later update. Local assets,
verified cache entries and exact download sources supply its files. Remote immutable assets resolve relative to the authenticated release URL.
`--file KEY=PATH` supplies restricted content.
For Prism layouts, content application also updates the exact game/loader component
profile in the same publication. Runtime-changing updates require stopping and
relaunching Prism so it reloads that profile. Empack does not install Java or client
game binaries; Prism performs its normal runtime preparation. Server runtime
preparation remains a separate obligation.
Key replacement and revocation retain the highest observed sequence, including
across restart and managed rollback. These are separate maintenance actions, not
installation steps. To rotate trust, run
`empack --workdir instance instance trust --key REPLACEMENT_PUBLIC_KEY_HEX` after
independently verifying the replacement public key. To disable channel updates,
use `empack --workdir instance instance trust --revoke-all`.

For an installed, explicitly enrolled **server export**, a local Java runtime can
request prelaunch updates. This requires the managed entry point produced by
`build server`; a generic `build empack` content archive does not provide it:

```sh
empack --workdir instance --yes instance launch --server --check-updates --allow-offline -- /absolute/path/to/java nogui
```

Omit `--allow-offline` to require a successful channel check. Fallback applies only
to channel connection failures or timeouts. Invalid signatures, revoked keys,
HTTP errors and failed installation stop launch. Prism runtime changes require a
launcher restart; managed servers publish their new runtime before launch.
The saved channel floor precedes release acquisition. Preview or declined approval
stops the sequence; launch still verifies the completed files and acquires its
runtime lease. Java remains locally selected; `--server` uses the completed release
entry point. Without `--check-updates`, launch uses the completed release
without consulting a channel.

Choose `empack build prism --updates empack` or `empack build server --updates empack`
to export a consumer that requires an enrolled channel. Both reference and bundled
delivery are supported. Enroll with `instance subscribe` in the extracted instance
before its first launch. The generated consumer checks for updates; it never copies
publisher keys into local trust. Managed server updates include runtime files; Prism
runtime changes require a stopped-launcher update and restart. Snapshot recipes
remain the default. Native archives support `build empack --updates empack`; their
release payload enforces the same explicit enrollment requirement.

## Publisher hosting

See [publishing](publishing.md) for key generation, signed release staging, static
HTTPS deployment and channel updates. Publishers share public keys through a trusted
route; subscribers never need a private signing key.

For an empack-managed server release, run
`empack instance launch --server -- java nogui`. Java is selected from your local PATH; the completed release supplies the
verified JAR or loader argument-file path. Add `--check-updates` before `--` for an
explicitly enrolled subscription. Changing Java or updating empack remains a local
operator action.

`build modrinth --updates platform` and `build curseforge --updates platform`
produce platform-targeted archives. They do not upload a project or associate a
recipient's instance with one. Upload the artifact and install it through the
platform's project/version interface to obtain platform updates. Importing the
archive directly remains a snapshot. Modrinth platform exports reject unsupported
hosting download domains; ordinary snapshot mrpacks may still use other HTTPS hosts.

## Shared pending inputs

Use the selected workspace or intended import destination for all four workflows:

```sh
empack --workdir my-pack pending list
empack --workdir my-pack pending list --json
empack --workdir my-pack pending show OPERATION_ID --json
empack --workdir my-pack --dry-run pending resume OPERATION_ID --file EXACT_KEY=/path/to/file
empack --workdir my-pack pending resume OPERATION_ID --file EXACT_KEY=/path/to/file
empack --workdir my-pack pending resume OPERATION_ID --downloads-dir /path/to/Downloads --open-downloads
empack --workdir my-pack pending discard OPERATION_ID
```

IDs accept unique prefixes and remain bound to the selected root and saved-record
bytes. List/show do not fetch content or modify state. Resume verifies supplied
bytes and asks for fresh workflow approval; discard conditionally removes only the
selected recipe after approval. Preview never opens browsers or scans Downloads.
Exact keys, unknown metadata, v0.6 compatibility and bounded assistance are described
in [the pending-input contract](design/pending.md). Existing `build --continue`,
`init --continue`, `sync --continue` and `instance continue` remain supported.

## Generated GitHub workflows

New projects include validation and release workflows plus `.github/scripts/empack-ci.py`.
They build the profiles in `build.default`. Set `EMPACK_PROFILES` in both
workflows to select explicit names, one per line. Both workflows apply authored optional
defaults. Set `JAVA_VERSION` for your runtime; the generated value is `21`.

Commit coherent authoring documents, bindings and lock before running CI. A release tag
must be `v` followed by the version in `empack.yml`. The release attaches every selected
distribution and `SHA256SUMS`, using normalized download filenames. It does not upload
to provider platforms. Existing workflow files are not automatically upgraded.

Reference Prism exports and all `updates: empack` Prism profiles require
`OverrideCommands=true` and the generated preparation/wrapper commands. Keep exactly
one `ConfigVersion=1.2` or `ConfigVersion=1.3` in `[General]`, with these command keys
only in that section. Legacy, duplicated or differently spelled authority settings
are rejected. Keep values on one line with balanced quotes. Use `;` for comment
lines; hash-prefixed lines are rejected because Prism can interpret them as settings.
Use ordinary spaces or tabs around keys and LF or CRLF line endings. Non-breaking
spaces and other Unicode whitespace are not indentation or key padding; Unicode
remains allowed in names and other user-facing values. Keep the
`INSTANCE_PREPARE_COMMAND` and `INSTANCE_WRAPPER_COMMAND` template expressions when
customizing `instance.cfg`; unrelated names, icons and memory settings are editable.
A template that removes required integration fails preparation without being rewritten.
For server entry points, see [custom scripts](servers.md#existing-exports-and-custom-scripts).

Prism inherits the recipient’s launcher memory settings by default. To set an instance
heap, add `OverrideMemory=true`, `MinMemAlloc=512` and an appropriate `MaxMemAlloc` value
in MiB to `templates/client/instance.cfg.template`.

See [project automation](design/automation.md) for installation verification, failure
handling and the distinction between GitHub releases and native signed channels.

Server exports start without the Minecraft GUI through `start.sh` or `start.bat`.
Use `start_gui.sh` or `start_gui.bat` to request the GUI. See
[running a server](servers.md) for Java, installation, EULA handling and Windows errors.
