# Engine API

The engine exposes prepared, authorized operations and retained outcomes. Public
clients cannot directly publish caller-constructed file changes.

## Requests

| Request | Input | Required result |
| --- | --- | --- |
| Initialize / Import | Authored values or normalized archive | Complete verified project, no early reset |
| Add / Remove | Logical selectors, sources and explicit policy | Exact ownership changes and coherent intent/lock |
| Synchronize | Intent and retained exact resolution | Satisfied intent without implicit upgrades |
| UpdateDependencies | Selected author roots | Deliberately refreshed compatible resolution |
| Adopt | Explicit observed content and identity evidence | Verified document changes without hidden installation |
| Build | Consumer recipes and choices | Verified artifacts from one captured resolution |
| InstallInstance | Exact release, destination, side and choices | Completed instance and durable ownership record |
| UpdateInstance | Instance and explicit release/subscription | Complete verified transition and retained previous release |
| RepairInstance | Last completed release and choices | Restored required content without advancing release |
| RollbackInstance | Retained completed release | Managed rollback with user-change conflicts preserved |
| Inspect / Recover | Native root and operation identity | Read-only classification or approved finish/restore |
| Clean | Explicit owned storage category | Scoped retirement preserving active leases and recovery |

CLI normalization produces these requests. Launcher integration uses the instance
requests; it never executes an alternate mutation path. Author `update` and instance
`update` have separate input types and do not share implicit version-selection policy.

## Lifecycle

```text
Engine.prepare(Request)
  -> DecisionRequired | NeedsInput | PreparedOperation
PreparedOperation.authorize(ExecutionGrant)
  -> ApprovedOperation
Engine.start(ApprovedOperation)
  -> OperationHandle
OperationHandle.wait()
  -> ExecutionOutcome
```

The request, prepared plan, grant and execution carry operation and attempt identity.
Private proof values cannot be decoded from project JSON or constructed by toggling
public fields. Replanning after changed inputs invalidates the previous grant.
The owning runtime survives a dropped UI handle until work retires or reaches a
durable recoverable boundary.

## Narrow ports

| Port | Capability |
| --- | --- |
| ProviderCatalog | Resolve selectors, exact pins, file roles and required evidence |
| ReleaseCatalog | Acquire bounded channel/release bytes; no local mutation |
| TrustVerifier | Authenticate exact envelopes against enrolled trust and sequence state |
| RootReader | Capture root-bound observations and immutable content leases |
| ContentAcquirer | Verify exact requested bytes through shared transport policy |
| ConsumerProjector | Normalize a complete inventory into a representable recipe |
| RuntimePreparer | Verify official assets and bounded installer outcomes |
| InstanceCoordinator | Bind launch/update ownership to one native instance |
| Publisher | Internal verified publication and recovery |
| DecisionHost | Present typed choices and return revision-bound answers |

Services do not expose unrestricted raw HTTP or mutable documents to command
handlers. A test adapter must declare weakened guarantees rather than inheriting
success defaults for locking, verification or root confinement.

## Decisions and continuation

Decisions cover selector ambiguity, explicit file roles/placements, optional groups,
consumer conversion, local conflicts, publisher trust and manual acquisition.
Answers bind to the relevant revision and input identity. Headless execution fails
with named unresolved decisions instead of selecting an arbitrary candidate.

Continuation retains the exact request, selected release/resolution, assertions,
verified associations and root/read-set binding. It is not replayable authorization.
Resume validates saved content, detects staleness and obtains a new grant. Preview
never deletes stale continuation or updates subscription state.

Instance preparation exposes missing logical file keys, exact SHA-256 addresses and
byte counts in `InstancePreview.manual`. A provider can also discover restricted
content during approved acquisition; `ExecutionInput.instance_requirements()`
reports those obligations. Build obligations use `ExecutionInput.requirements()`.
The single-consumer continuation retains verified inputs.
`Engine::resume_instance_files` accepts explicit key-to-file associations, verifies
the bytes and captured base, and produces a new plan requiring fresh approval.
An empty or incomplete response remains `NeedsInput`; it does not silently retry a
known restricted download or publish the available subset.

## Outcomes and diagnostics

| Outcome | Meaning |
| --- | --- |
| Completed | Verified postconditions and durable completion receipt |
| PartiallyCompleted | Explicit independent author groups or scoped maintenance; receipt names effects |
| NeedsInput | No activation; exact unresolved input remains resumable |
| FailedBeforePublication | Live managed state unchanged |
| RecoveryRequired | Live effects may exist; operation identity and recovery route are retained |
| Cancelled | Work retired; outcome states whether recovery is required |

Public diagnostics carry stable code, phase, affected logical object, expected and
observed values, effect classification and recovery action. Internal contextual
error chains remain available but cannot determine status by string matching.
Secrets, authenticated URLs and credentials are redacted. Cache-only admission or
storage failure does not fail otherwise valid authoritative work; cancellation,
changed staged bytes and integrity failures still propagate.

## Host responsibilities

The executable owns Tokio, Ctrl-C, process exit and terminal restoration. Sessions
carry invocation paths, user configuration and narrow services. Independent embedded
sessions must not inherit the first session's display or error configuration.
Progress is bounded and lossy; completion is retained independently of subscribers.

`LaunchInstanceRequest` carries an absolute host-selected program and native
arguments. `LaunchInstancePreview` identifies the completed release and executable.
An exact-plan grant must separately permit `run_runtime`; allowing installers does
not permit runtime launch. `LaunchInstanceReceipt` retains the process exit status.
The engine owns process supervision and the instance lease independently of the
caller's handle. Release and channel documents cannot construct launch requests.

Unconfirmed runtime retirement produces `runtime-recovery-required` in the execution
phase with required recovery. It is distinct from uncertain file publication and
ordinary cancellation. Hosts route it to stopped-process confirmation and
`instance recover-runtime --acknowledge-stopped`; replaying a publication journal
does not clear runtime ownership evidence.
