# Observation, planning and authorization

Target contract for v0.5.0-alpha.1. Code blocks are design sketches unless the
[implementation ledger](implementation.md) identifies a compiled API.

## 7. Snapshots, observations, and revision binding

A snapshot is more than a manifest plus a set of filenames. Cross-crate value types such as `WorkspaceSnapshot` expose a checked constructor (`validate_parts(SnapshotParts)`) for the trusted snapshotter and test fixtures. That constructor validates internal consistency, not the truth of native observations. Authority-bearing stages and publication proofs remain separately private inside `empack-lib`; the publisher re-observes the actual root and files.

```rust
pub struct ObservedProject {
    pub installed: Vec<InstalledRecord>,
    pub files: BTreeMap<ManagedPath, FileObservation>,
    pub unmanaged: Vec<UnmanagedObservation>,
    pub backend_documents: BackendDocumentSet,
}

pub struct InstalledRecord {
    pub identity: Option<ProviderProjectId>,
    pub provider_file: Option<ProviderFileId>,
    pub metadata_key: BackendMetadataKey,
    pub kind: ContentKind,
    pub placements: NonEmpty<ObservedPlacement>,
    pub ownership: ObservedOwnership,
}

pub struct SourceRevision {
    pub project: ProjectInstanceId,
    pub semantic: SemanticRevision,
    pub read_set: ReadSetDigest,
}

pub enum ReadExpectation {
    File { path: ManagedPath, content: ContentId, kind: FileKind },
    Absent { path: ManagedPath },
    DirectoryMembers { path: ManagedDirectory, names: MembershipDigest },
}
```

An intended new file depends on its destination still being absent. A cleanup depends on the inspected directory membership. A manifest patch depends on the raw source document revision, even when a concurrent comment-only edit does not affect build semantics.

Use at least two fingerprints: the semantic recipe revision and the actual read-set/document revision. Do not overwrite a comment edit merely because the semantic hash is unchanged. Modification time and size can accelerate observation, but are not final content identity.

Snapshot capture takes empack's project read/mutation coordination lock where needed. Source bytes needed for a long build are copied or retained into immutable staging. Revalidate the read set after capture and before publication. External editors can still race; detect conflicts rather than claiming serializable isolation from programs that do not participate.

The snapshotter refuses ordinary operation on a hot publication journal until recovery resolves it. All engine readers honor that gate, including builds. An unrelated native tool may see mixed files during publication; that limitation remains explicit.

### 7.1 Snapshot and planning port boundary

```rust
pub trait ProjectReader: Send + Sync {
    fn snapshot<'a>(
        &'a self,
        target: &'a ProjectTarget,
        policy: SnapshotPolicy,
        context: &'a ReadContext,
    ) -> PortFuture<'a, WorkspaceSnapshot, SnapshotError>;
}
```

`ReadContext` contains cancellation, budgets, and read-only storage access. It has no publisher, mutation lease, or general process launcher. `WorkspaceSnapshot` exposes immutable accessors for intent, lock, observations, source revision, and managed-path ownership.


## 8. Requests, planning, and authorization

### 8.1 Requests express intent, not implementation steps

```rust
pub enum Request {
    Initialize(InitializeRequest),
    Import(ImportRequest),
    Add(AddRequest),
    Remove(RemoveRequest),
    Sync(SyncRequest),
    Update(UpdateRequest),
    Build(BuildRequest),
    Clean(CleanRequest),
    AdoptObserved(AdoptObservedRequest),
}

pub struct AddRequest {
    pub inputs: NonEmpty<AddInput>,
    pub replacement: ReplacementPolicy,
    pub batch: BatchPolicy,
}

pub enum AddInput {
    Provider { selector: ProjectSelector, pin: Option<PinSelector> },
    DirectDownload { locator: DownloadLocator, kind: Option<ContentKind> },
    Local { source: UserSelectedFile, kind: Option<ContentKind> },
}

pub enum ReplacementPolicy {
    RejectExisting,
    UpdateSameIdentity,
    ReplaceSelected { selection: DependencySelection },
}

pub enum BatchPolicy {
    AllRequested,             // All candidate work must verify before publication
    ContinueIndependent,      // Publish verified independent groups, report partial
}
```

`InitializeRequest` and `ImportRequest` include a `ReplacementPolicy`, metadata/runtime overrides, layout options, and conversion policy. `BuildRequest` includes exact targets, archive format, optional-file choices, and a build-input policy. `CleanRequest` selects managed build/cache categories, never accepts arbitrary recursive deletion paths.

An explicit force option maps to a specific replacement request. It does not turn off schema, hash, confinement, identity, or artifact checks. Accepted-version overrides change compatibility intent explicitly; they do not disable validation of unrelated properties.

### 8.2 Pure plans

```rust
pub fn plan(
    snapshot: &WorkspaceSnapshot,
    request: &ResolvedRequest,
    policy: &PlanningPolicy,
) -> Result<Plan, PlanError>;

pub struct Plan {
    id: PlanId,
    base: SourceRevision,
    groups: Vec<ActionGroup>,
    expected: ExpectedProject,
    footprint: EffectFootprint,
    decisions: Vec<DecisionRequirement>,
}

pub struct ActionGroup {
    pub id: GroupId,
    pub prerequisites: Vec<GroupId>,
    pub actions: NonEmpty<PlannedAction>,
    pub postconditions: Vec<Postcondition>,
}

pub enum PlannedAction {
    EnsureProvider(ProviderSelection),
    EnsureContent(ResolvedFileRef),
    RemoveInstalled(RemovalSelection),
    ApplyIntent(IntentDelta),
    AdoptObserved(AdoptionSelection),
    BuildProjection(BuildProjection),
    RemoveManagedFile(ManagedRemoval),
    RemoveCacheObjects(CacheEvictionSelection),
}
```

`Plan` fields are private with read-only accessors. Only planners construct valid plans. Planner decisions cannot be mutated after the authorization digest is computed.

`RemovalSelection` contains the selected logical key, canonical identity, observed metadata key, expected version/content, and exact managed destinations. A user string is not a backend removal target.

Removal distinguishes dropping an explicit root from deleting content. Root demotion
retains its exact selection and dependency evidence. Content removal requires complete
retained dependency evidence and no incoming required edge from a retained selection.
A whole cycle can be selected explicitly; unrequested content is never inferred to be
removable. Unknown, duplicate or blocked selections reject an AllRequested batch.

`IntentDelta` identifies exact logical records to insert/update/rename/delete. Adding the same canonical identity under an alias updates that existing record or asks for an explicit rename. Publication never chooses its manifest key by looking only at the new backend filename.

`EffectFootprint` describes permitted managed writes/deletes, tools, acquisition origins, and the declared backend closure policy. If a backend discovers additional required dependencies, they must fit verified closure rules and permitted managed namespaces. A new destructive target, incompatible selection, unknown acquisition origin, or broader tool authority requires re-planning and renewed authorization. Unexpected installed dependencies are proposals, not permission to redefine expected results from whatever the backend happened to write: independently resolve their identity/evidence and amend the plan before publication. A policy may auto-authorize a verified non-destructive dependency-closure amendment, but that amendment still gets a new plan digest and recorded grant.

### 8.3 Authorization belongs to the plan, not a global boolean

```rust
pub struct PreparedOperation { /* plan + retained immutable preparation inputs */ }
pub struct ApprovedOperation { /* private, consumed by Engine::start */ }

pub struct ExecutionGrant {
    pub plan: PlanId,
    pub decisions: DecisionSet,
    pub permission: ExecutionPermission,
}

pub enum ExecutionPermission {
    NormalProjectChanges,
    ReplaceManagedProject { acknowledged: ReplacementSummaryDigest },
}

impl PreparedOperation {
    pub fn view(&self) -> PreparedView<'_>;
    pub fn authorize(self, grant: ExecutionGrant)
        -> Result<ApprovedOperation, AuthorizationError>;
}
```

An embedding application is an authority capable of granting execution; the engine is not an authentication server. It must still reject a grant for another plan, unanswered decisions, or a replacement acknowledgment that does not match the planned managed footprint.

Planning and temporary reads precede destructive authorization. Trusted tool execution in staging is an authorized effect even though it does not intentionally write live project files. If executing a tool is needed to determine a plan at all, preparation must report that requirement rather than running arbitrary source-supplied commands under a preview label.

### 8.4 Partial batches without conflicting document writes

`ContinueIndependent` is not “catch every error and count some successes.” Partition actions into groups only after analyzing dependencies and overlapping writes. Mutually dependent actions belong in one group. A group's failure blocks dependents.

All successful groups contribute to one candidate intent/lock/index update. Recompute and verify the combined candidate once; do not publish several manifest replacements all derived from the same stale base. Failed groups retain their original intent and installed content unless an explicit independent change requires otherwise.

A partial lock identifies unresolved roots; it never claims to be a complete resolution. Strict builds reject unresolved required roots. The final outcome is `PartiallyCompleted`, with successful and blocked groups, even when every actually published file is internally consistent.

### 8.5 Postconditions are data

```rust
pub enum Postcondition {
    IdentitySatisfied { key: DependencyKey, identity: ResolvedIdentity },
    PinSatisfied { key: DependencyKey, pin: ResolvedPin },
    ContentAt { destination: ManagedPath, content: ContentId },
    LogicalRecordAbsent { key: DependencyKey },
    ManagedFileAbsent { path: ManagedPath },
    RequirementsPreserved { owner: ContentOwner, requirements: Requirements },
    InventorySatisfied { inventory: InventoryId },
    NoUnexpectedManagedEffects,
}
```

A verifier uses fresh candidate observations, not a backend's assertion that these are true. Unsupported postconditions are an implementation error, not a reason to skip verification.

`ExpectedProject` contains the desired logical intent/resolution, the required postconditions, retained-original obligations for unaffected content, and the allowed managed footprint. It is produced by the planner, never synthesized solely from actual backend output. Verification checks both the intended changes and preservation obligations; an add must not quietly delete an unrelated configuration file while correctly installing its requested mod.
