# Engine API and operation traces

Target contract for v0.5.0-alpha.1. Code blocks are design sketches unless the
[implementation ledger](implementation.md) identifies a compiled API.

The compiled build entry point is `empack_lib::engine::api::Engine`. Its `preview`
and `prepare` accept an absolute existing-project selection and a `BuildRequest`.
Both only capture and plan. The preview includes exact artifact destinations,
runtime, missing content, network/tool requirements and the complete requested
options. It has no conversion into an executable operation. A ready preparation
can be consumed with an `ExecutionGrant` naming its opaque in-process `PlanId`;
the originating engine then admits `start`.

The owned driver acquires captured archives and remote content, prepares the exact
runtime, verifies every requested artifact and publishes their union. Missing
manual/provider acquisitions remain `NeedsInput`; the build API does not invent a
provider result or download association. Persistent content lookup and the APIs for
other operation kinds remain completion work. The broader interface below remains
the target for those operations.

Preparation, acquired content and retained receipts own explicit host admission
estimates. Those estimates are separate from enforced stream/snapshot byte limits.
Abandoned preparations retire before their registry entries disappear. Publication
receipts survive cancellation after commitment; a runtime-level worker failure or
`ExecutionUncertain` requires recovery assessment and must not be presented as proof
that no files changed. Native publication errors retain their recovery operation ID.
The compiled usage example is tested with Rust documentation tests in
[`api.rs`](../../crates/empack-lib/src/engine/api.rs).

## 17. Public engine API and application wiring

### 17.1 Public entry points

```rust
pub struct Engine { /* narrow services composed internally */ }

pub struct PrepareOptions {
    pub network: NetworkPermission,
    pub cache: CacheAccess,
    pub limits: PreparationLimits,
}

pub enum Preparation {
    Ready(PreparedOperation),
    NeedsInput(PreparationContinuation),
    Blocked(PreparationReport),
}

impl Engine {
    pub async fn preview(
        &self, target: ProjectTarget, request: Request, options: PreviewOptions,
    ) -> Result<PreviewReport, EngineError>;

    pub async fn prepare(
        &self, target: ProjectTarget, request: Request, options: PrepareOptions,
    ) -> Result<Preparation, EngineError>;

    pub fn start(&self, operation: ApprovedOperation)
        -> Result<OperationHandle, AdmissionError>;

    pub async fn resume(
        &self, target: ProjectTarget, input: ContinuationInput,
        decisions: DecisionSet, options: PrepareOptions,
    ) -> Result<ResumeAssessment, EngineError>;

    pub async fn inspect_recovery(
        &self, target: &ProjectTarget,
    ) -> Result<Option<RecoveryReport>, EngineError>;

    pub async fn recover(
        &self, target: ProjectTarget, decision: ApprovedRecovery,
    ) -> Result<RecoveryOutcome, EngineError>;

    pub async fn shutdown(&self, policy: ShutdownPolicy)
        -> Result<ShutdownReport, EngineError>;
}
```

`ProjectTarget` is `Existing` or `NewAt(UserSelectedDirectory)`. Both must resolve into host-bound root capabilities; creation is also a publication effect. For a nonexistent destination, bind and lock the existing parent plus validated child name and its expected absence until creation. Do not create the destination or a persistent project registration during preview; use a proposed in-memory identity until execution is authorized. Source/destination path selection belongs at the public boundary, not inside imported content.

`preview` uses the same resolver and pure planner as execution, but composes read-only cache/storage capabilities and no publisher or mutating backend capability. It returns plan confidence, required decisions, and unresolved blockers. Temporary downloads are bounded and removed. It does not serialize a durable operation as a side effect.

`prepare` can use the explicitly granted cache policy. It never changes live project/artifact content. Required verification that can be done before tools is completed here; required backend verification occurs on staging before publication.

`PreparationContinuation` is an in-memory continuation by default. Persisting it requires an explicit authorized operation or host action. A preview cannot obtain that writer by converting its report.

Host-only cache maintenance does not require a configured pack or a fabricated project snapshot. Give it a narrow `preview_cache_cleanup(CacheCleanRequest) -> CacheCleanupPlan` and `prepare_cache_cleanup(CacheCleanRequest) -> PreparedCacheCleanup` entry point. The latter is authorized and started through a maintenance handle; the store validates ownership, leases, expected identities, and verified eviction before mutation. It reuses admission, cancellation, read-only preview, receipts, and recovery primitives, but has no project writer. Its `CacheCleanupReceipt` reports reclaimed, retained, and failed objects. A combined `clean all` aggregates project and host-maintenance outcomes explicitly; it does not claim an atomic transaction across them. This is a distinct domain plan, not a second permissive deletion implementation.

### 17.2 Operation handles and outcomes

```rust
pub struct OperationHandle { /* observation + cancellation, not task resources */ }

impl OperationHandle {
    pub fn id(&self) -> OperationId;
    pub fn status(&self) -> Arc<OperationStatus>;
    pub fn subscribe(&self) -> ProgressSubscription;
    pub fn cancel(&self, reason: CancelReason);
    pub async fn wait(&mut self) -> Arc<OperationOutcome>;
}

pub enum OperationOutcome {
    Completed(OperationReceipt),
    PartiallyCompleted(PartialReceipt),
    NeedsInput(ContinuationReceipt),
    FailedBeforePublication(FailureReport),
    InterruptedBeforePublication(InterruptionReport),
    RecoveryRequired(RecoveryRequired),
}

pub struct OperationReceipt {
    pub operation: OperationId,
    pub plan: PlanId,
    pub before: SourceRevision,
    pub after: SourceRevision,
    pub applied_groups: Vec<GroupId>,
    pub artifacts: Vec<ArtifactReceipt>,
    pub verification: VerificationSummary,
    pub warnings: Vec<Diagnostic>,
}
```

The handle does not own task reservations or the only journal reference. Dropping it requests cancellation according to its configured policy, but driver retirement is engine-owned. `wait` returns a retained terminal outcome even if a notification was missed.

`PartiallyCompleted` includes a committed receipt for applied groups and separate blocked/failed groups. `NeedsInput` is not falsely labeled success or generic backend failure. When some groups were already published, use `PartiallyCompleted` with pending-input details rather than an outcome that implies no publication. `RecoveryRequired` distinguishes effects that may already be visible from failure before publication.

Errors use stable diagnostic codes and structured causes. A CLI adapter may preserve existing numeric exit classes, including cancellation, without making library users parse text. Include file/record locations, provider identity, operation ID, and a next safe action. Redact credentials by construction.

### 17.3 Composition root

This is illustrative wiring, not a proposed public service-locator interface:

```rust
pub fn assemble(config: HostConfig) -> Result<Engine, SetupError> {
    let limits = Arc::new(ResourceGovernor::new(config.resources)?);
    let runtime = Arc::new(OperationRuntime::new(limits.clone()));
    let roots = Arc::new(NativeRootFactory::new(config.filesystem_policy)?);
    let host_state = Arc::new(HostStateStore::open(config.state_root)?);
    let content = Arc::new(FileContentStore::open(config.cache_root, limits.clone())?);

    let transport = Arc::new(PolicyTransport::new(
        config.network, config.credentials, limits.clone(),
    )?);
    let acquisition = Arc::new(VerifiedAcquisition::new(
        transport.clone(), roots.scratch_factory(),
    ));
    let catalog = Arc::new(ProviderRegistry::new(vec![
        Arc::new(ModrinthCatalog::new(transport.clone())),
        Arc::new(CurseForgeCatalog::new(transport.clone())),
    ]));

    let processes = Arc::new(OwnedProcessRunner::new(runtime.clone()));
    let tools = Arc::new(LazyToolResolver::new(
        acquisition.clone(), processes.clone(), config.tools,
    ));
    let backend = Arc::new(PackwizAdapter::new(tools.clone(), processes.clone()));
    let codecs = Arc::new(ProjectCodecs::new());
    let journals = Arc::new(JournalStore::new(host_state.clone()));
    let reader = Arc::new(ProjectSnapshotter::new(
        roots.project_reader(), codecs.clone(), journals.reader(),
    ));

    let preparation = PreparationService::new(
        reader, catalog, acquisition.clone(), codecs.clone(),
        ImportRegistry::standard(),
    );
    let staging = StagingService::new(
        roots.stage_factory(), acquisition, backend, RuntimeAdapters::new(tools, processes),
        codecs,
    );
    let verification = VerificationService::new(ArtifactReaders::standard());
    let publisher = Publisher::new(roots.publisher_fs(), journals, host_state);
    let access = AccessScopes::new(content); // Only Engine can derive per-call capabilities.

    Ok(Engine::compose(runtime, access, preparation, staging, verification, publisher))
}
```

Only `assemble` sees all dependencies. `PreparationService` does not receive `Publisher` or a capability factory that can grant itself cache writes. `Engine::preview` passes read-only per-call acquisition authority; `Engine::prepare` derives only the cache authority the caller allowed. Root factories are projected into scratch, staging, reader, and publisher interfaces, and snapshotting receives only the journal reader. A snapshotter therefore cannot delete stale recovery state. `PackwizAdapter` does not receive project-document publication. `VerificationService` does not trust a backend-provided expected inventory.

The constructor names are proposed concrete adapters. In implementation, prefer grouped typed configuration objects when a constructor grows too large, but do not introduce `services: Arc<Everything>` to shorten the signature.

### 17.4 CLI usage

```rust
async fn run_change(
    engine: &Engine,
    args: ChangeArgs,
    ui: &mut dyn UserInterface,
) -> Result<ExitClass, CliError> {
    let (target, request) = translate_args(args.clone())?;

    if args.dry_run {
        let report = engine.preview(target, request, args.preview_options()).await?;
        ui.show_preview(&report)?;
        return Ok(ExitClass::from_preview(&report));
    }

    let prepared = match engine.prepare(target, request, args.prepare_options()).await? {
        Preparation::Ready(value) => value,
        Preparation::NeedsInput(value) => {
            return ui.handle_preparation_input(engine, value).await;
        }
        Preparation::Blocked(report) => {
            ui.show_preparation_failure(&report)?;
            return Ok(ExitClass::from_preparation(&report));
        }
    };

    let grant = ui.authorize(prepared.view(), args.interaction_policy()).await?;
    let approved = prepared.authorize(grant)?;
    let mut operation = engine.start(approved)?;
    ui.observe(operation.subscribe())?;
    let outcome = operation.wait().await;
    ui.show_outcome(&outcome)?;
    Ok(ExitClass::from_outcome(&outcome))
}
```

This `UserInterface` is an illustrative host interface; an object-safe implementation uses the same boxed-future convention for its async operations. It receives no live filesystem or backend capability.

The only CLI-specific dry-run branch chooses the preview API. There are no dry-run booleans threaded through destructive helpers. Preview cannot accidentally call a downloader that also installs a JAR because acquisition and installation are different interfaces.

### 17.5 Embedding usage and configuration

An embedding application creates one engine, prepares typed requests, answers structured decisions, starts operations, and consumes status/receipts. It may run preparation for several projects concurrently; publication is serialized per host-bound project instance.

Configuration parsing produces `HostConfig` plus field provenance. Define CLI/environment/dotenv precedence through one adapter. Help/version can use a minimal parsing path that does not bootstrap tools or unnecessarily read malformed project configuration. Workdir resolution has one invocation-root rule. [E3](https://github.com/inherent-design/empack/blob/50c121f/docs/usage.md)

No operation reads global CLI flags. Providers and planners receive explicit per-operation policy. Display/logging state is instance-owned or explicitly shared by the embedding host, not hidden mutable globals.


## 18. End-to-end usage traces

### 18.1 Direct ZIP/JAR add, including preview

1. CLI creates `AddInput::DirectDownload` with optional content kind.
2. Preparation acquires into bounded scratch, using read-only cache under preview. It computes content evidence and probes archive/JAR type.
3. Provider identification produces a canonical record, a choice, or `Unknown`. Unknown content can become tracked local/URL intent only through the applicable acceptance policy.
4. The planner selects the logical key and exact placements. Existing same-identity aliases are updated, not duplicated.
5. Preview renders that plan and drops scratch leases. No backend or live project writer was available.
6. Execution materializes a stage, verifies installed identity/content/requirements, freezes it, and publishes a verified change.

**Regression prevented:** a helper cannot download-and-install before a later dry-run check because its acquisition port has no installation authority.

### 18.2 Forced import into an existing pack

1. Read and snapshot the current project without resetting it.
2. Parse the archive into `ImportedProject`; preserve provenance, optionality, and layers.
3. Resolve provider references and verify required URL/embedded bytes. Unsupported conversion or wrong digest blocks preparation while the old pack remains intact.
4. Construct the replacement in staging, including metadata overrides, datapack-folder choices, backend options, and generated default templates where authorized.
5. The approval names the managed replacement footprint. User-owned unrelated content is excluded.
6. Verify the complete replacement, then publish with durable recovery data.

**Regression prevented:** the importer and its caller cannot disagree about whether preflight already happened; the publisher consumes only a verified candidate.

### 18.3 Remove by alias, title, or installed stem

1. Resolve user selection against intent and observations. An ambiguous title/stem returns a choice.
2. Produce `RemovalSelection` binding one logical key to its canonical identity, installed metadata key, and expected content.
3. Check dependency evidence. Demote a root still needed by another root or report the explicit dependent-removal requirement.
4. Modify the stage and exact logical record, then verify absence/retention postconditions.
5. Publish only the planned managed file changes. A local record pointing to a directory fails before any removal.

**Regression prevented:** strings from different identity namespaces never reach a raw backend remove operation interchangeably.

### 18.4 Build all targets and reject silent omission

1. Capture satisfied exact build input and all preserved observed content.
2. Project inventories for selected targets and expand shared prerequisites.
3. Build an intermediate once in this invocation, in staging. Do not reuse a name/version-matched old archive.
4. An exporter returns exit 0 but omits a required provider file. `ArtifactReader` detects a missing inventory item, and verification fails.
5. Discard failed candidate artifacts; keep the prior published distribution.

**Regression prevented:** every source kind contributes to inventory verification; no URL-only verification exception exists.

### 18.5 Restricted acquisition and resume

1. Preparation or staged backend execution produces `PendingAcquisition` with expected identity/evidence.
2. Host optionally opens the provider page and watches configured read-only download roots.
3. Verified candidate bytes satisfy the pending item; ambiguous or evidence-poor matches require explicit association.
4. If suspended, retain validated content and a bounded continuation description, releasing active execution permits.
5. Resume assesses the current source revision. Stale state is reported without deletion. Valid state re-enters the normal plan/stage/verify/publish flow.

**Regression prevented:** continuation is not a path-authoritative second build pipeline or a filename-guessing cache insertion path.

### 18.6 Concurrent edit during a long acquisition

1. Prepare plan A against revision R and release the project lock while acquiring bytes.
2. A user edits the manifest or changes relevant source content.
3. A stages and verifies its candidate, then requests publication.
4. Publisher detects read-set mismatch and returns conflict before live changes.
5. Reprepare against the new revision, reusing validated content if applicable. Any changed destructive footprint requires a new grant.

**Regression prevented:** a long-running plan cannot overwrite a later user edit solely because it still holds an old manifest in memory.
