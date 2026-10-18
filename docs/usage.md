# Usage

Empack separates pack authoring from installed game instances. Use `--workdir` before
the command to select the authoring project, instance or publisher directory. Relative
input paths resolve from the directory where you invoked empack, not `--workdir`.
This guide follows the development CLI; `empack <command> --help` lists the installed options.

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

Status does not verify installed content, authenticate a channel or establish that
Minecraft will launch. `update` changes author dependency selections;
`instance update` applies a published pack release. Neither updates empack itself.

## Authoring

| Command | Meaning |
| --- | --- |
| `init` | Create an authoring project or import a supported archive |
| `add` | Describe and resolve dependencies, preserving exact source intent |
| `remove` | Remove or demote selected logical roots with native ownership checks |
| `sync` | Apply authored intent while retaining compatible locked selections |
| `outdated` | Read provider update evidence and joint dependency blockers |
| `update` | Deliberately refresh selected eligible dependency versions |
| `migrate` | Plan or apply a Minecraft/loader change and explicit replacements |
| `adopt` | Verify selected existing content and record it without hidden installation |
| `build` | Produce explicitly selected consumer distributions |
| `recover` | Inspect or finish/restore an interrupted publication |
| `clean` | Retire explicitly owned artifacts, disposable cache or continuation |

Modrinth public resolution needs no API key. For CurseForge, provide your client key
through `EMPACK_KEY_CURSEFORGE`; keep credentials out of the manifest and version
control. Resolution and referenced downloads require network access.

Edit `empack.yml` directly or use authoring commands. Common provider mods can use
`label: modrinth:CANONICAL_ID`; use a mapping for side, content kind or exact policy.
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
empack outdated --json --release-policy prefer-stable
empack --dry-run update sodium --release-policy stable-only
empack migrate --minecraft 1.21.2 --loader fabric --plan
empack --yes migrate --minecraft 1.21.2 --loader fabric
```

`outdated` distinguishes the locked version, policy-permitted candidate, compatible
candidate and newest observed upstream selection. Unavailable evidence is reported
as unknown, not up to date. Its joint dependency check is separate from each row;
it does not promise that configurations or gameplay will work.

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
file key, byte count and SHA-256 address. Supply associations to `instance continue`;
resuming checks the original assertions and instance base before showing a new plan.
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
delivery, environment, update owner and format. `distribution.defaults` selects
which profiles a plain `empack build` builds together.

```yaml
distribution:
  defaults: [launcher]
  profiles:
    launcher:
      consumer: prism
      delivery: bundled
      environment: client
      updates: snapshot
      format: zip
    dedicated:
      consumer: server
      delivery: bundled
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
policies: Modrinth/CurseForge/empack references and bundled Prism/server content.
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

Workdir and relative paths resolve from captured invocation context. State and cache
roots are separately configurable. Configuration precedence is explicit and cannot
cause an inherited environment value to override a supplied command-line value.

## Native release example

Set stable native publication identity and runtime requirements in `empack.yml`:

```yaml
distribution:
  defaults: [native]
  profiles:
    native:
      consumer: empack
      delivery: bundled
      environment: both
      updates: snapshot
      format: zip
  native:
    pack-id: my-pack
    java-major: 21
    policies:
      config/server-defaults.toml: seed
sources:
  exclude:
    - private/**
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
