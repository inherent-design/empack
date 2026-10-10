# Installed-instance lifecycle

An instance installs exact published releases while preserving player and operator
state. Author dependency resolution does not run implicitly during an update.

## Consumer layout

The completed record binds a fixed content layout: `game/` for native instances or
`.minecraft/` for Prism. `instance install --layout prism` selects the latter;
subsequent installation, repair and rollback retain it. Changing layout requires a
separate installation rather than moving ownership implicitly.

Prism prefers `minecraft/` if that directory exists. The Prism layout therefore
requires its absence and binds that absence through publication. A small owned
`.minecraft/.empack-layout` marker keeps the selected directory present even for an
empty release. A release cannot overwrite this marker or the `.empack-consumer` input directory. Launcher components and
icons remain outside the content installer's ownership. [Prism directory selection](https://github.com/PrismLauncher/PrismLauncher/blob/develop/launcher/minecraft/MinecraftInstance.cpp)

`instance prepare RELEASE --sha256 ID` is the consumer entry point. It installs the
initial snapshot when no completed record exists. Otherwise it verifies and repairs
the active release with saved choices; it cannot replay the original package over a
later update. The initial descriptor must name the same pack and runtime. A runtime
change requires updating the consumer integration before launch. Preparation failure
returns an error and grants no permission to start the game.

## State transitions

```mermaid
stateDiagram-v2
  [*] --> Uninstalled
  Uninstalled --> Prepared: select release and choices
  Installed --> Prepared: update or repair
  Prepared --> WaitingForInput: conflicts or manual content
  WaitingForInput --> Prepared: revalidate decisions and inputs
  Prepared --> Staged: authorize and verify acquisitions
  Staged --> Publishing: acquire instance lease and revalidate
  Publishing --> Installed: verify files and commit record
  Publishing --> RecoveryRequired: interruption or uncertain durability
  RecoveryRequired --> Installed: finish or restore verified state
  Installed --> Running: launch lease
  Running --> Installed: process retirement
```

Preparation failure keeps the last completed installation. A pending publication
blocks launch. An engine lock alone cannot stop a manually launched game; managed
launch holds an instance lease until the process retires. Launcher integrations
must either provide equivalent coordination or refuse updates while execution
cannot be safely established as stopped.

## Three-way file decisions

`previous` is the last installed baseline, `current` is captured native state, and
`incoming` belongs to the authenticated selected release after side/choice projection.
Comparisons include content, size and supported permission attributes.

| Previous | Current | Incoming | Decision |
| --- | --- | --- | --- |
| *none* | Absent | Managed file | Create |
| *none* | Any file | Managed file | Unowned collision, even if bytes match |
| Managed A | A | B | Replace A with verified B |
| Managed A | B | B | Retain matching desired bytes and accept B after verification |
| Managed A | Changed C | B or absent | Conflict; retain C |
| Managed A | Absent | B | Restore required content |
| Managed A | A | Absent | Remove exact unchanged file |
| Managed A | Absent | Absent | No file mutation; retire ownership |
| Any | Regular file | Seed | Preserve existing user bytes |
| Any | Absent | Seed | Create initial bytes without future replacement authority |
| Seed | Existing file | Managed | Explicit ownership decision required |
| Seed | Any file or absent | Absent | Preserve user state and retire seed tracking |
| Any | Directory/link/unsupported kind at selected file | File or removal | Reject unsafe shape |

An absent dependency in author intent is not sufficient evidence to remove shared
requirements. Conversely, an exact instance inventory and prior owned baseline can
justify retiring files absent from the next complete release.

## Configuration and mutable data

Managed configuration changes use the same three-way comparison as other files.
There is no automatic text merge based only on filename extension. A conflict offers
preserve, replace or a separately verified merge result. Replacing requires exact
observed-byte authorization.

Initial configuration and world templates are seeds. Existing worlds, saves,
logs, player preferences and runtime-generated data are excluded from ordinary
release replacement and rollback. New releases cannot change a seed to managed
ownership without an explicit local decision.

## Update and repair

Update selects a published release under the configured authority. Repair restores
the recorded release and choices without advancing a channel or resolving newer
provider files. Both reacquire exact missing content and verify original assertions.
A provider URL refresh may not change exact selection or expected bytes.

Missing manual downloads retain a bounded continuation record outside disposable
cache, bound to release, instance, choices, original assertions and observed base.
Resuming revalidates all bindings and prepares a new operation for authorization.

## Rollback and retention

Managed rollback selects a previously completed release and uses the same conflict
checks. It never rewinds played-world data. Previous release descriptors and any
required rollback content have explicit retention leases; a short-lived publication
journal is not the release history store. Unavailable retained content is a reported
acquisition obligation, not permission to guess or reuse different bytes.

## Launch and offline policy

Snapshot is the default update policy. Prelaunch channel checks require explicit
subscription and trust. Offline launch is an explicit setting permitting the last
completed installation when a network update check fails. It cannot bypass a failed
signature, known incompatible runtime, unresolved conflict or pending recovery.
Launch errors distinguish update availability, acquisition, publication and game
process failures. Updating empack itself requires independent tool policy.

## Native layout and snapshot application

The selected instance directory contains `game/` for installed content and
`.empack/instance.json` for ownership. Immutable release payloads are retained at
`.empack/releases/<sha256>.json`. Authoring documents and `pack/` are not created
or consulted. The root-bound instance record identifies its pack, side, installed
release, selected choices and prior completed releases. Copying the record to a
different native root does not authorize replacement there.

Snapshot application takes an explicit payload digest. The CLI form is
`empack --workdir <instance> instance install <release.json> --sha256 <digest>`.
`--side client|server` selects the environment. Relative immutable assets resolve
beneath the selected release directory through no-follow handles. `--file KEY=PATH`
associates a separately supplied file with its exact release identity; `--choice
KEY=VALUE` selects a stable alternative. Applying a snapshot never subscribes to
a channel or establishes publisher trust.

Preparation verifies supplied files and bundled assets without network access.
Missing referenced files appear as exact acquisition obligations in the preview.
Execution requires a network grant before using their HTTPS alternatives. Downloads
share the engine's transfer limits and resource admission; every selected file must
match both its original assertions and the release's content address before any
instance change is published. A failed transfer or verification preserves the
previous installation. A download URL never changes the selected file identity.

Provider acquisition refreshes only the declared project, version and file role.
Refreshed locators must remain compatible with the release's original assertions;
they cannot substitute a newer version. Provider world members share one verified
archive acquisition. Extraction checks each exact member address independently and
retains seed ownership. Approved execution may reuse and populate the disposable
verified-content cache; cached bytes remain subject to the same assertions.

A new instance uses declared choice defaults. Updates retain choices by stable key.
A newly introduced choice or an unavailable prior alternative requires a decision;
a changed publisher default does not overwrite a saved selection. Projection
rejects colliding paths before content staging, including case and Unicode aliases.

Preparation captures the prior record, retained descriptor, and the union of old
and incoming selected paths. Unrelated game data is outside this read set. Content
staging checks selected addresses and original source assertions independently.
Only actual changes enter the write set, while all captured observations remain
publication preconditions. Payload installation alone does not authorize game launch;
consumer/runtime preparation and launch coordination must also complete.

`instance inspect` reads the root-bound completed record and validates its retained
release descriptor. It does not inspect unrelated game bytes or mutate state.
`instance repair` retains the exact installed release, side and choices;
`instance rollback <release-id>` accepts only a retained completed release.
Both accept `--assets <directory>` and `--file KEY=PATH` for exact reacquisition.
Rollback can supply `--choice KEY=VALUE` when an older alternative needs a decision.
Neither command advances a channel or reduces a subscription's sequence floor.

Repair rejects a different release or changed choices even through the library API.
Rollback history authorizes selecting a descriptor, not overwriting changed managed
files. The same three-way comparison applies to all three commands. Previewing
maintenance captures and verifies inputs without replacing instance or game files.
