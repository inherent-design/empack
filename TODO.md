# v0.6.0-beta implementation

The target is one native engine for pack authoring, consumer exports and installed
instance updates. The contracts in [docs/design](docs/design/README.md) define
behavior; this file is the implementation checklist. It does not certify release
readiness or replace executable evidence.

## Accepted design

- Users author `empack.yml`; `empack.lock` records exact resolution. Neither
  document mirrors another package manager's metadata.
- Remove all packwiz observation, metadata maintenance, ignore policy, installer
  trees, JAR acquisition, command generation and executable dependencies. Retain
  no packwiz import/export or mixed-authoring promise.
- Preserve Modrinth and CurseForge imports, provider/direct/local content, mods,
  resources, shaders, datapacks, worlds, exact pins, named file roles, multiple
  placements, side layers, optional groups and explicit conversion decisions.
- Consumer families are Modrinth, CurseForge, Prism, dedicated server and native
  empack. Dependency delivery and update authority are separate dimensions.
- Platform archives are ordinary consumer artifacts; they do not require empack.
  Format validity, platform eligibility and marketplace upload are distinct.
  Marketplace upload/account automation is outside this scope.
- Native releases are immutable portable manifests with exact content and runtime
  requirements. Mutable channels point to authenticated releases. Static HTTPS
  hosting is sufficient; no custom server or daemon is required.
- Snapshot installation is the default. Channel subscription is explicit.
  Unattended updates require signed metadata and an explicitly trusted publisher.
  Release descriptors never authorize updating the empack executable.
- One native executable provides installation and updates. A thin launcher helper
  may acquire a verified executable; it contains no second resolver or installer.
- Instance state records installed release, publisher/subscription, side, choices,
  installed file identities and ownership. It is neither an author lock nor an
  eviction cache. Recovery journals remain host-private and root-bound.
- Compare previous installed, current and incoming files. Retire unchanged owned
  files; preserve unowned files and report changed managed files. Initial config
  seeds transfer to user ownership. Worlds and runtime-generated data are not
  ordinary update targets. Managed rollback never implies world rollback.
- Default mutations publish only a complete verified candidate. Explicit independent
  batches remain available for author dependency operations, not partial release
  activation. Launch requires a completed installation and no pending recovery.
- Supported loader families, archives, templates, resource admission, cancellation,
  source integrity, manual acquisition, cache leases and recovery remain required.
- No automatic legacy schema migration, standalone game launcher, OS package manager
  integrations, arbitrary remote scripts or background update service is included.

## Contracts and documents

- [x] Replace author/backend descriptions with the consumer and instance contracts.
- [x] Implement strict release/channel payload codecs and signed-envelope verification.
- [x] Wire exact local snapshot content application through the engine and CLI.
- [x] Wire retained-release inspection, exact repair and managed rollback.
- [x] Export native snapshots through shared verified acquisition and batch publication, preserving original
  assertions, provider provenance, layers, choices and explicit source rules.
- [x] Define strict, independently versioned author, lock, release, channel and
  instance schemas. DTO parsing never constructs trusted or approved proof values.
- [x] Specify author source inclusion in `empack.yml`, not `.packwizignore`.
- [x] Specify release canonical identity and signing envelope, key enrollment,
  rotation, revocation, expiry and monotonic channel sequence handling.
- [x] Keep expected source assertions, computed content addresses, publisher
  authentication and user authorization distinct in diagnostics and receipts;
  expose typed release-authentication, identity and engine-compatibility failures.
- [x] Specify the public request/decision/outcome API and the CLI command groups.
- [x] Preserve the feature surface in consumer capability and acceptance tables.

## Instance engine

- [x] Acquire referenced files and provider archive members after approval, with
  exact provider refresh, bounded transfers and all-selected verification.
- [x] Implement pure three-way file decisions, including wrong-kind, missing-file,
  unowned collision, changed-content and permission-change cases.
- [x] Construct complete plans with collision checks, root bindings, read sets,
  exact acquisition obligations and instance-record postconditions.
- [x] Persist choices by stable choice key; request decisions for new choices,
  changed constraints and removed alternatives without silently resetting choices.
- [x] Install, update and repair exact release selections through shared preparation,
  grants, staging, verification and recoverable publication.
- [x] Support manual restricted downloads and restart continuation with original
  digests, release identity and supplied-file associations retained.
- [x] Implement changed-config decisions and initial-only configuration seeds;
  never infer mergeability from an extension or overwrite a played world.
- [x] Keep durable instance ownership separate from disposable content caches.
- [x] Implement explicit managed rollback with retained previous release evidence;
  do not rely on the publication journal's short-lived preimages for release history.
- [x] Coordinate prelaunch/server execution and updates with an instance lease.
  Document the limit for programs started outside empack's coordination.
- [x] Allow explicitly configured offline launch only from a completed installation;
  distinguish update-check failure from failed or uncertain publication.

## Release distribution and trust

- [x] Derive portable native releases from the exact lock and selected inventory;
  strip host paths, credentials, cache paths and author-only state.
- [x] Use stable pack identity, content-derived release identity and logical file
  identity; display names and version labels are not mutation authority.
- [x] Publish immutable manifests/assets before changing a channel pointer.
- [x] Authenticate channel and release bytes before following content instructions;
  validate redirects, origins, expiry, signatures and saved sequence floors.
- [x] Support explicit publisher enrollment and rotation without silent trust on
  first unattended use. Keep signing keys outside pack sources and distributions.
- [x] Reject downgrade/replay unless the operator explicitly selects rollback;
  snapshot installation remains possible without a channel subscription.
- [x] Keep tool acquisition/version policy independent of pack metadata and cover
  local executable selection with Unix and Windows tests.

## Consumer adapters

- [x] Implement validated consumer/delivery/update-authority recipe values, with
  snapshot defaults and explicit environment/authority capability errors.

- [x] Replace `BuildTarget` and persisted target strings with validated consumer,
  delivery, environment and update-policy recipes; reject invalid combinations.
- [x] Wire complete recipes into CLI selection and every executable consumer adapter;
  bind platform associations and native subscriptions before activation.
- [x] Preserve mrpack hashes, sizes, URLs, side layers and requirements. Separate
  generic format validation from Modrinth hosting-domain eligibility.
- [x] Implement CurseForge manifest ZIP export and independently inspect its
  referenced project/file identities and overrides. Reject unverifiable cross-provider
  substitutions and report content that cannot be represented or distributed.
- [x] Replace lightweight Prism JAR commands with native instance integration;
  preserve launcher components, icons, templates and ordinary snapshot imports.
- [x] Preserve server recipes for vanilla, Fabric, Quilt, Forge and NeoForge,
  historical loader variants, Java requirements and generated launchers.
- [x] Replace full/light naming with explicit dependency delivery. Bundled pack
  content does not imply bundled client game binaries or permission to redistribute.
- [x] Support native release directories/archives and channel output without making
  every consumer depend on an intermediate mrpack.
- [ ] Verify launcher imports and prelaunch status propagation in actual consumers;
  writer/reader agreement within empack is insufficient.

## Removal and wiring

- [x] Remove foreign metadata discovery and index maintenance from addition,
  updates, synchronization and adoption; identify adopted provider content by bytes.
- [x] Resolve removal through logical keys, titles and canonical provider identities;
  remove foreign-record selectors, ownership claims and index rewriting.
- [x] Apply native `sources.exclude` capture to the remaining author commands.
- [x] Remove `BackendDocument`, foreign discovery, index refresh, metadata
  adoption and metadata-based ownership from project commands.
- [x] Remove the remaining packwiz serializer/parser with the light consumer recipes.
- [x] Preserve explicit adoption of verified local/provider/URL content through
  native identity and observation, without consulting foreign metadata.
- [x] Replace old target selectors, templates, source filters, diagnostics,
  release recipes and fixture assumptions as their native consumers are wired.
- [x] Remove packwiz runtime assets, dependency pins, installer interaction options,
  archive projection/verifier branches and obsolete exclusive tests.
- [x] Route author and instance operations through one engine facade; retire
  replaced implementation paths rather than keeping compatibility dispatch.
- [x] Search the complete repository for residual packwiz authority and obsolete
  target/schema claims. Keep third-party format names where semantically required.

## Acceptance

- [x] Author A, publish A, install client/server, change options and configuration,
  publish B with additions/removals/runtime changes, update and inspect exact bytes.
- [x] Interrupt acquisition and each durable publication boundary, restart, recover,
  and establish that no incomplete installation is allowed to launch.
- [x] Test managed rollback with user edits, absent retained content, expired channel
  metadata and a played-world sentinel that must remain unchanged.
- [x] Test wrong keys/signatures, expired/replayed channels, changed manifests,
  revoked publisher keys, cross-pack substitution and explicit trust rotation.
- [x] Test optional groups, side-specific overrides, local seeds, renamed files,
  shared ownership, unexpected directories/links and destination collisions.
- [ ] Run native-platform tests, strict provider/runtime/curated-pack workflows,
  formatting, Clippy, architecture/API checks and consumer import verification.
- [ ] Review and fix the completed implementation, then bind release acceptance to
  one immutable revision and publish `v0.6.0-beta` only after the release gates pass.
