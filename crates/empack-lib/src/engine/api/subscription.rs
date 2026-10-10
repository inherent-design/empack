//! Explicit publisher enrollment and durable anti-replay observations.
use super::*;
use crate::engine::{
    instance::InstanceRecord,
    layout::ProjectLayout,
    publication::{Publisher, root_key},
    release,
    snapshot::ProjectReadRoot,
    staging::MutableStage,
    verification::{self, VerifiedFileChange},
};
use ed25519_dalek::VerifyingKey;
use empack_core::{
    digest::ContentId,
    files::{FileContent, FilePermissions, FilePlan, ManagedPath},
};
use std::collections::BTreeSet;

pub use crate::engine::instance::subscription::SubscriptionRecord;
/// Enrollment and local key rotation require the ordinary exact-plan execution grant.
pub enum SubscriptionRequest {
    Enroll {
        pack: String,
        channel: String,
        url: String,
        keys: Vec<VerifyingKey>,
    },
    /// Empty keys revoke all update authority without deleting the sequence floor.
    ReplaceKeys { keys: Vec<VerifyingKey> },
    /// Verify metadata using current enrolled keys and persist its floor before acquisition.
    Observe {
        envelope: Vec<u8>,
        now: i64,
        engine: semver::Version,
    },
}
#[derive(Clone)]
pub struct SubscriptionPreview {
    pub plan: PlanId,
    pub record: SubscriptionRecord,
    pub files: FilePlan,
    pub replacement: ReplacementSummary,
}
pub struct SubscriptionReceipt {
    pub plan: PlanId,
    pub record: SubscriptionRecord,
    pub publication: PublicationReceipt,
}
pub(super) struct PreparedSubscription {
    pub(super) view: SubscriptionPreview,
    root: ProjectReadRoot,
    change: VerifiedFileChange,
}
fn encoded_keys(keys: Vec<VerifyingKey>) -> Vec<String> {
    let mut keys: Vec<_> = keys
        .into_iter()
        .map(|key| release::hex(key.as_bytes()))
        .collect();
    keys.sort();
    keys
}

pub(super) async fn prepare(
    target: ProjectTarget,
    request: SubscriptionRequest,
    config: &EngineConfig,
    scope: &mut super::super::runtime::WorkScope,
) -> Result<RetainedOutput<PreparedSubscription>> {
    let ProjectTarget::Existing(selected) = target else {
        anyhow::bail!("Subscription requires an existing instance directory")
    };
    ensure!(selected.is_absolute(), "Instance root must be absolute");
    let state = config.state_root.clone();
    let mut limits = config.snapshot;
    limits.file_bytes = limits.file_bytes.min(release::MAX_RELEASE_BYTES as u64);
    let work = scope.spawn_blocking(
        config.resources.capture,
        config.resources.prepared,
        move |cancel| {
            let root = ProjectReadRoot::open(&selected)?;
            let _guard = RecoveryReader::new(state).enter(&root)?;
            let target = ManagedPath::InstanceSubscription;
            let snapshot = root.capture(
                &[
                    ProjectLayout::path(&target)?,
                    ProjectLayout::path(&ManagedPath::InstanceRecord)?,
                ],
                limits,
                &cancel,
            )?;
            let binding = root_key(&root)?;
            let previous =
                crate::engine::instance::read_optional(&root, &snapshot, &target, &cancel)?
                    .map(|bytes| SubscriptionRecord::decode(&bytes))
                    .transpose()?;
            if let Some(previous) = &previous {
                ensure!(
                    previous.root == binding,
                    "Subscription belongs to another instance root"
                );
            }
            let installed = crate::engine::instance::read_optional(
                &root,
                &snapshot,
                &ManagedPath::InstanceRecord,
                &cancel,
            )?
            .map(|bytes| InstanceRecord::decode(&bytes))
            .transpose()?;
            if let Some(installed) = &installed {
                ensure!(
                    installed.root == binding,
                    "Installed record belongs to another root"
                );
            }
            let record = match request {
                SubscriptionRequest::Enroll {
                    pack,
                    channel,
                    url,
                    keys,
                } => {
                    ensure!(
                        previous.is_none(),
                        "Instance already has a subscription; change its keys explicitly"
                    );
                    ensure!(
                        !keys.is_empty(),
                        "Enrollment requires an explicit publisher key"
                    );
                    SubscriptionRecord {
                        schema: 1,
                        root: binding,
                        pack,
                        channel,
                        url,
                        keys: encoded_keys(keys),
                        floor: None,
                        observed: None,
                    }
                }
                SubscriptionRequest::ReplaceKeys { keys } => {
                    let mut previous = previous.context("Instance has no subscription")?;
                    previous.keys = encoded_keys(keys);
                    previous
                }
                SubscriptionRequest::Observe {
                    envelope,
                    now,
                    engine,
                } => {
                    let mut previous = previous.context("Instance has no subscription")?;
                    let channel = previous.trust()?.channel(
                        &envelope,
                        &previous.channel,
                        now,
                        &engine,
                        previous.floor.as_ref(),
                    )?;
                    previous.floor = Some(channel.floor().clone());
                    previous.observed = Some(String::from_utf8(envelope)?);
                    previous
                }
            };
            ensure!(
                installed
                    .as_ref()
                    .is_none_or(|installed| installed.pack == record.pack),
                "Subscription belongs to another installed pack"
            );
            let bytes = serde_json::to_vec(&record)?;
            SubscriptionRecord::decode(&bytes)?;
            let observed = verification::observed_files_for(&snapshot, [target.clone()])?;
            let desired = BTreeMap::from([(
                target.clone(),
                FileContent {
                    content: ContentId::from_sha256(release::decode_hex(&release::hash(&bytes))?),
                    bytes: bytes.len() as u64,
                    permissions: FilePermissions {
                        readonly: false,
                        executable: false,
                    },
                },
            )]);
            let files = verification::plan_mutation_files(&observed, &desired, &BTreeSet::new())?;
            let mut stage = MutableStage::empty()?;
            if files.expected().contains_key(&target) {
                stage.write(
                    &ProjectLayout::path(&target)?,
                    &mut bytes.as_slice(),
                    bytes.len() as u64,
                    &cancel,
                )?;
            }
            let stage = stage.freeze(limits, &cancel)?;
            let change = VerifiedFileChange::verify_subscription(snapshot, files.clone(), stage)?;
            let view = SubscriptionPreview {
                plan: PlanId(
                    NEXT_PLAN
                        .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                        .map_err(|_| anyhow::anyhow!("Plan identifier exhausted"))?,
                ),
                replacement: project_change::summary(&files)?,
                files,
                record,
            };
            Ok::<_, anyhow::Error>(PreparedSubscription { view, root, change })
        },
    )?;
    scope.accept(work.wait().await?)?.transpose()
}
pub(super) async fn run(
    prepared: RetainedOutput<PreparedSubscription>,
    config: EngineConfig,
    scope: super::super::runtime::WorkScope,
) -> Result<ExecutionOutcome, RuntimeError> {
    let cancellation = scope.cancellation();
    let result: Result<_> = async {
        let work = scope.spawn_blocking(
            config.resources.assembly,
            config.resources.receipt,
            move |cancel| {
                let (prepared, _reservation) = prepared.into_parts();
                let publication = Publisher::open(&config.state_root)?.publish(
                    &prepared.root,
                    prepared.change,
                    &cancel,
                )?;
                Ok::<_, anyhow::Error>(SubscriptionReceipt {
                    plan: prepared.view.plan,
                    record: prepared.view.record,
                    publication,
                })
            },
        )?;
        scope
            .accept_publication(work.wait().await.map_err(PublicationWorkerFailed)?)?
            .transpose()
    }
    .await;
    Ok(match result {
        Ok(receipt) => {
            ExecutionOutcome::Completed(ExecutionReceipt::Subscription(Box::new(receipt)))
        }
        Err(error) => ExecutionOutcome::failed(error, cancellation.is_cancelled()),
    })
}

#[cfg(test)]
mod tests;
