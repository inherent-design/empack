# Command contract

This page specifies the v0.6.0-beta command surface. It is a target contract; use
`empack --help` and subcommand help for the executable actually installed.

## Authoring

| Command | Meaning |
| --- | --- |
| `init` | Create an authoring project or import a supported archive |
| `add` | Describe and resolve dependencies, preserving exact source intent |
| `remove` | Remove or demote selected logical roots with native ownership checks |
| `sync` | Apply authored intent while retaining compatible locked selections |
| `update` | Deliberately refresh selected eligible dependency versions |
| `adopt` | Verify selected existing content and record it without hidden installation |
| `build` | Produce explicitly selected consumer distributions |
| `recover` | Inspect or finish/restore an interrupted publication |
| `clean` | Retire explicitly owned artifacts, disposable cache or continuation |

Edit `empack.yml` directly or use authoring commands. Commit its exact generated lock
when distributing reproducible author sources. Build requires satisfied intent and
does not implicitly update dependencies. Removal of a manifest root does not grant
permission to delete user files or required shared dependencies.

Provider selectors, explicit pins, typed local/URL content, named file plans and side
placements normalize through the same engine. Ambiguous or unsupported inputs fail
with actionable decisions. Default add/update batches publish nothing when any item
fails. Explicit independent batches preserve failed groups and report partial effects.

## Installed instances

Missing manual downloads can be saved after approval. The command reports the exact
file key, byte count and SHA-256 address. Supply associations to `instance continue`;
resuming checks the original assertions and instance base before showing a new plan.
A preview never saves or removes pending state. If the base or publisher enrollment
has changed, inspect it and explicitly discard the stale recipe before selecting a
new operation.

| Command | Meaning |
| --- | --- |
| `instance install` | Install an exact native release into a selected root |
| `instance update` | Apply a selected release or explicitly subscribed channel |
| `instance continue --file KEY=PATH` | Resume a retained exact release with verified manual inputs |
| `instance discard-pending` | Explicitly discard a pending recipe without changing the installation |
| `instance repair` | Restore the recorded release without selecting newer dependencies |
| `instance options` | Inspect/change persistent optional choices |
| `instance rollback` | Return managed content to a retained completed release |
| `instance launch -- PROGRAM ARGS...` | Verify completed content and run a locally selected runtime under an instance lease |
| `instance inspect` | Explain release, ownership, choices, conflicts and update authority |

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

Select Modrinth, CurseForge, Prism, server or native empack recipes in authored
configuration. Where supported, select reference or bundled delivery independently
of snapshot/channel updating. Platform format compatibility, platform eligibility
and uploading are distinct results. Selecting every output is not the default.

For example:

```sh
empack build modrinth prism
empack build prism server --delivery references
empack build server --delivery bundled --format tar.gz
empack build modrinth --environment client
empack build empack --environment server --delivery references
```

With no consumer arguments, build uses the complete `distribution.recipes` list.
Explicit arguments start with snapshot policies: Modrinth/CurseForge/empack
references and bundled Prism/server content. `--delivery`, `--environment` and `--updates` override
the selected recipes. Invalid combinations fail before acquisition. Different
policies for the same consumer belong in separate authored recipe objects.
Continuation retains its saved recipes and rejects policy overrides.

Prism consumes ordinary platform archives without an empack integration. A native
Prism instance can instead carry launcher settings and explicitly subscribe through
empack. Server recipes preserve exact runtime and startup requirements.

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

Workdir and relative paths resolve from captured invocation context. State and cache
roots are separately configurable. Configuration precedence is explicit and cannot
cause an inherited environment value to override a supplied command-line value.

## Native release example

Set stable native publication identity and runtime requirements in `empack.yml`:

```yaml
distribution:
  recipes:
    - consumer: modrinth
      delivery: references
      environment: both
      updates: snapshot
  archive: zip
  native:
    pack-id: my-pack
    java-major: 21
    policies:
      config/server-defaults.toml: seed
sources:
  exclude:
    - private/**
```

`empack build empack --delivery bundled` writes
`dist/<name>-<version>-empack-bundled.empack` using the configured archive format.
Reference delivery uses an `empack-references` suffix. Native recipes share verified
acquisition, missing-download continuation and combined publication with the other
consumers. They preserve optional choices for installation. Extract the archive, then select its `release.json` with
`instance install` and the payload SHA-256 printed by the build. The digest names
the JSON payload, not the surrounding archive. Side selection applies the matching
override layer. Local snapshot selection does not enroll a publisher.

Publisher enrollment is explicit and separate from snapshot installation:

```sh
empack --workdir instance instance subscribe --pack example --channel stable https://example.org/stable.json --key PUBLIC_KEY_HEX
empack --workdir instance instance observe-channel
empack --workdir instance instance update
empack --workdir instance instance trust --key REPLACEMENT_PUBLIC_KEY_HEX
empack --workdir instance instance trust --revoke-all
```

Enrollment and observation save trust and authenticated sequence observations.
`instance observe-channel` fetches the enrolled HTTPS URL and saves the verified
observation after approval. `instance update` fetches the exact signed release
selected by that saved channel. Both commands also accept a local envelope path.
Update checks current trust, expiry and exact release identity before applying
content. Local assets, verified cache entries and exact download sources supply its
files. Remote immutable assets resolve relative to the authenticated release URL.
`--file KEY=PATH` supplies restricted content.
Content application does not install Java or establish runtime launch readiness.
Key replacement and revocation retain the highest observed sequence, including
across restart and managed rollback.

For an explicitly enrolled instance, a local runtime can request prelaunch updates:

```sh
empack --workdir instance --yes instance launch --check-updates --allow-offline -- /absolute/path/to/java -jar server.jar nogui
```

Omit `--allow-offline` to require a successful channel check. Fallback applies only
to channel connection failures or timeouts. Invalid signatures, revoked keys,
HTTP errors, changed runtime requirements and failed installation stop launch.
The saved channel floor precedes release acquisition. Preview or declined approval
stops the sequence; launch still verifies the completed files and acquires its
runtime lease. The executable and arguments remain locally selected, never supplied
by the pack publisher. Without `--check-updates`, launch uses the completed release
without consulting a channel.

## Publisher staging

Extract a native export, keep publisher keys outside the project and export, then
stage its signed immutable files:

```sh
empack --workdir ./publisher --yes release stage ./export --key-file ~/.config/empack/publisher.key
```

The output is `publisher/dist/releases/<release-id>/release.json` and its assets.
Serve `publisher/dist/` through static HTTPS hosting. Staging checks original asset
assertions and refuses different bytes at existing release addresses. It does not
advance a channel or enroll subscribers. Use `--dry-run` to inspect the release,
output paths and signing fingerprints before publication.

After the immutable release is available through HTTPS, prepare its channel pointer:

```sh
empack --workdir ./publisher --yes release publish-channel RELEASE_ID \
  --channel stable --base-url https://packs.example.org/ \
  --sequence 1 --expires UNIX_UTC_SECONDS \
  --key-file ~/.config/empack/publisher.key
```

Choose a future expiry within 31 days. This verifies the hosted envelope and assets
before writing `publisher/dist/channels/stable.json`. If deployment uses uploads,
upload this pointer last. Increase the sequence when changing channel metadata.
`--previous-key` accepts an old public key for authenticating the existing pointer
after signing-key rotation; it does not sign the replacement or enroll client trust.
