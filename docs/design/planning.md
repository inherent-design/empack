# Observation, planning and authorization

Contract for v0.5.0-alpha.1. Callable types and signatures are defined in the
[pure planners](../../crates/empack-core/src/), [native snapshots](../../crates/empack-lib/src/engine/snapshot.rs) and [operation API](../../crates/empack-lib/src/engine/api.rs). This page specifies their behavior and ownership.

## Snapshots, observations, and revision binding

`ProjectReader` produces a `WorkspaceSnapshot` containing decoded intent, exact
resolution, retained inputs and native observations. Capture binds those observations
to the selected root and recovery state. Semantic constructors validate internal
consistency; they do not prove native facts. Publication re-observes the actual root
and relevant files.

An intended new file depends on its destination still being absent. A cleanup depends on the inspected directory membership. A manifest patch depends on the raw source document revision, even when a concurrent comment-only edit does not affect build semantics.

Use at least two fingerprints: the semantic recipe revision and the actual read-set/document revision. Do not overwrite a comment edit merely because the semantic hash is unchanged. Modification time and size can accelerate observation, but are not final content identity.

Snapshot capture takes empack's project read/mutation coordination lock where needed. Source bytes needed for a long build are copied or retained into immutable staging. Revalidate the read set after capture and before publication. External editors can still race; detect conflicts rather than claiming serializable isolation from programs that do not participate.

The snapshotter refuses ordinary operation on a hot publication journal until recovery resolves it. All engine readers honor that gate, including builds. An unrelated native tool may see mixed files during publication; that limitation remains explicit.

### Snapshot and planning port boundary

Preparation receives cancellation, budgets and read-only storage capabilities.
Those capabilities cannot publish or launch arbitrary tools. `WorkspaceSnapshot` exposes immutable accessors for intent, lock, observations, source revision, and managed-path ownership.

## Requests, planning, and authorization

### Requests express intent, not implementation steps

`InitializeRequest` and `ImportRequest` carry normalized candidates and a
`ProjectReplacementPolicy`; hosts resolve metadata, runtime, layout and conversion
choices before constructing them. `BuildRequest` names targets, archive format and
optional-file choices. Project and cache cleanup use distinct typed requests; neither
accepts arbitrary recursive deletion paths.

An explicit force option maps to a specific replacement request. It does not turn off schema, hash, confinement, identity, or artifact checks. Accepted-version overrides change compatibility intent explicitly; they do not disable validation of unrelated properties.

### Pure plans

`FilePlan` validates expected changes and retained obligations. Prepared operations
keep their executable state private and bind authorization to an opaque `PlanId`;
a caller cannot mutate that state through its display-only preview.

`RemovalSelection` exposes the logical key, title, canonical identity and selected
version. The removal plan separately binds observed metadata and exact managed
destinations. A user string is not a backend removal target.

Removal distinguishes dropping an explicit root from deleting content. Root demotion
retains its exact selection and dependency evidence. Content removal refuses incomplete
retained dependency evidence by default. An explicit `AcknowledgeUnknown` policy can
accept that uncertainty for the selected content; preview and receipt list the
retained selections whose edges remain incomplete. Their coverage is not upgraded.
A known incoming required edge from a retained selection always blocks deletion.
This acknowledgement does not weaken file ownership, content verification, path
confinement or publication checks, and never permits automatic orphan collection.
A whole cycle can be selected explicitly; unrequested content is never inferred to be
removable. Unknown, duplicate or blocked selections reject an AllRequested batch.

A candidate intent update identifies exact logical records to insert, update or remove. Adding the same canonical identity under an alias updates that existing record or asks for an explicit rename. Publication never chooses its manifest key by looking only at the new backend filename.

The prepared footprint describes permitted managed writes/deletes, tools, acquisition origins and dependency closure. If a backend discovers additional required dependencies, they must fit verified closure rules and permitted managed namespaces. A new destructive target, incompatible selection, unknown acquisition origin, or broader tool authority requires re-planning and renewed authorization. Unexpected installed dependencies are proposals, not permission to redefine expected results from whatever the backend happened to write: independently resolve their identity/evidence and amend the plan before publication. A policy may auto-authorize a verified non-destructive dependency-closure amendment, but that amendment still gets a new plan digest and recorded grant.

### Authorization belongs to the plan, not a global boolean

An embedding application is an authority capable of granting execution; the engine is not an authentication server. It must still reject a grant for another plan, unanswered decisions, or a replacement acknowledgment that does not match the planned managed footprint.

Planning and temporary reads precede destructive authorization. Trusted tool execution in staging is an authorized effect even though it does not intentionally write live project files. If executing a tool is needed to determine a plan at all, preparation must report that requirement rather than running arbitrary source-supplied commands under a preview label.

### Partial batches without conflicting document writes

`ContinueIndependent` is not “catch every error and count some successes.” Partition actions into groups only after analyzing dependencies and overlapping writes. Mutually dependent actions belong in one group. A group's failure blocks dependents.

All successful groups contribute to one candidate intent/lock/index update. Recompute and verify the combined candidate once; do not publish several manifest replacements all derived from the same stale base. Failed groups retain their original intent and installed content unless an explicit independent change requires otherwise.

A partial lock identifies unresolved roots; it never claims to be a complete resolution. Strict builds reject unresolved required roots. The final outcome is `PartiallyCompleted`, with successful and blocked groups, even when every actually published file is internally consistent.

### Postconditions are data

A verifier uses fresh candidate observations, not a backend's assertion that these are true. Unsupported postconditions are an implementation error, not a reason to skip verification.

The planned candidate contains desired intent and resolution, required postconditions,
preservation obligations and the allowed managed footprint. Expected results come
from planning, never solely from observed writer output. Verification checks both the intended changes and preservation obligations; an add must not quietly delete an unrelated configuration file while correctly installing its requested mod.
