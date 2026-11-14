//! Installed evidence is checked without planning a repair, acquiring bytes or changing ownership.
use super::*;
use crate::engine::{
    checking::{CheckFinding, CheckState},
    runtime::WorkProgress,
};

#[derive(Debug, Serialize)]
pub struct InstanceCheckReport {
    pub schema: u32,
    pub scope: &'static str,
    pub pack: Option<String>,
    pub version: Option<String>,
    pub release: Option<String>,
    pub verified_files: usize,
    pub verified_overrides: usize,
    pub player_owned_entries: usize,
    pub findings: Vec<CheckFinding>,
    pub complete: bool,
}
pub struct InstanceCheck {
    pub report: InstanceCheckReport,
    pub result: Result<()>,
}
impl InstanceCheck {
    fn failed(&mut self, subject: impl Into<String>, error: anyhow::Error) {
        self.report
            .findings
            .push(CheckFinding::failure(subject, &error));
        if self.result.is_ok() {
            self.result = Err(error);
        }
    }
    fn finding(
        &mut self,
        subject: impl Into<String>,
        state: CheckState,
        message: impl Into<String>,
    ) {
        self.report.findings.push(CheckFinding {
            subject: subject.into(),
            state,
            required: state == CheckState::Passed,
            message: message.into(),
            diagnostic: None,
        });
    }
}
/// Retain the bounded report under the caller's existing work/resource lifetime.
pub async fn check_scoped(
    scope: &mut crate::engine::runtime::WorkScope,
    root: std::path::PathBuf,
    recovery: RecoveryReader,
    limits: SnapshotLimits,
) -> Result<crate::engine::runtime::RetainedOutput<InstanceCheck>> {
    let progress = scope.progress_reporter();
    let worker = scope.spawn_blocking(
        crate::engine::resources::ResourceRequest {
            jobs: 1,
            memory_bytes: 96 << 20,
            open_files: 16,
            ..Default::default()
        },
        crate::engine::resources::ResourceRequest {
            memory_bytes: 64 << 20,
            ..Default::default()
        },
        move |cancel| check(&root, recovery, limits, &cancel, &progress),
    )?;
    scope.accept(worker.wait().await?).map_err(Into::into)
}
pub fn check(
    selected_root: &Path,
    recovery: RecoveryReader,
    limits: SnapshotLimits,
    cancel: &Cancellation,
    progress: &impl Fn(WorkProgress),
) -> InstanceCheck {
    let mut check = InstanceCheck {
        report: InstanceCheckReport {
            schema: 1,
            scope: "instance",
            pack: None,
            version: None,
            release: None,
            verified_files: 0,
            verified_overrides: 0,
            player_owned_entries: 0,
            findings: vec![],
            complete: false,
        },
        result: Ok(()),
    };
    if let Err(error) = inspect(
        selected_root,
        recovery,
        limits,
        cancel,
        progress,
        &mut check,
    ) {
        check.failed("Installed state", error);
    }
    check.report.complete = check.result.is_ok();
    check
}
fn inspect(
    selected_root: &Path,
    recovery: RecoveryReader,
    limits: SnapshotLimits,
    cancel: &Cancellation,
    progress: &impl Fn(WorkProgress),
    check: &mut InstanceCheck,
) -> Result<()> {
    ensure!(
        selected_root.is_absolute(),
        "Instance check requires an absolute selected directory"
    );
    progress(WorkProgress::CheckingProjectFile {
        path: "completed instance".into(),
        index: 0,
        total: 0,
    });
    let CompletedInstance {
        root,
        mut snapshot,
        record,
        release,
    } = capture_completed(selected_root, None, &recovery, limits, cancel)?
        .context("No completed instance exists; select the installed game directory")?;
    let _guard = recovery.enter(&root)?;
    root.revalidate(&snapshot, cancel)?;
    check.report.pack = Some(record.pack.clone());
    check.report.version = Some(release.document().version.clone());
    check.report.release = Some(record.release.clone());
    let version = semver::Version::parse(crate::engine::compatibility_version())?;
    // This checks the local descriptor and engine requirement, not publisher authenticity.
    super::super::release::trust::SelectedSnapshot::select(
        release.bytes(),
        release.id(),
        &version,
    )?;
    check.finding(
        "Completed release",
        CheckState::Passed,
        "Release identity, native root and engine requirement agree",
    );
    if recovery.has_runtime_evidence(&root)? {
        check.failed("Runtime state",anyhow::anyhow!("Managed runtime is active or its retirement is unconfirmed; stop it and recheck. Inspect stopped-runtime evidence before acknowledging it"));
        return Ok(());
    }
    let marker = match record.layout {
        InstanceLayout::Game => ManagedPath::InstanceLayoutMarker,
        InstanceLayout::Prism => ManagedPath::PrismLayoutMarker,
    };
    let mut controls = vec![
        ProjectLayout::path(&marker)?,
        ProjectLayout::path(&ManagedPath::InstanceSubscription)?,
    ];
    if record.layout == InstanceLayout::Prism {
        match root.directory.symlink_metadata("minecraft") {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
            Ok(_) => anyhow::bail!(
                "Prism layout requires minecraft/ to be absent; installed content uses .minecraft/"
            ),
        }
        controls.push(path("minecraft")?);
    }
    snapshot = snapshot.merge(root.capture(&controls, document_limits(limits), cancel)?)?;
    let marker_result = (|| {
        ensure!(
            read_optional(&root, &snapshot, &marker, cancel)?.as_deref()
                == Some(record.layout.marker()),
            "Instance layout marker is missing or changed; review instance repair"
        );
        Ok(())
    })();
    match marker_result {
        Ok(()) => check.finding(
            "Layout",
            CheckState::Passed,
            "Installed game directory and layout marker agree",
        ),
        Err(error) => check.failed("Layout", error),
    }
    let subscription_result = (|| {
        let bytes = read_optional(&root, &snapshot, &ManagedPath::InstanceSubscription, cancel)?;
        if let Some(bytes) = bytes {
            let subscription = subscription::SubscriptionRecord::decode(&bytes)?;
            ensure!(
                subscription.root == record.root && subscription.pack == record.pack,
                "Subscription belongs to another instance or pack"
            );
            if release.document().require_subscription {
                subscription.trust()?;
            }
            if subscription.observed.is_some() {
                subscription.authenticated_channel(&version)?;
            }
        } else {
            ensure!(
                !release.document().require_subscription,
                "Installed release requires publisher enrollment; use instance subscribe"
            );
        }
        Ok(())
    })();
    match subscription_result {
        Ok(()) => check.finding(
            "Subscription state",
            CheckState::Passed,
            "Saved enrollment and observation are consistent; no fresh channel was requested",
        ),
        Err(error) => check.failed("Subscription state", error),
    }
    let profile = if record.layout == InstanceLayout::Prism {
        let observed = root.capture(
            &[ProjectLayout::path(&ManagedPath::PrismProfile)?],
            document_limits(limits),
            cancel,
        )?;
        let bytes = read_optional(&root, &observed, &ManagedPath::PrismProfile, cancel)?
            .context("Prism runtime profile is missing")?;
        Some(ReadOnlyPrismProfile {
            identity: super::super::prism::ProfileIdentity::capture(
                &bytes,
                release.document().runtime.resolution()?,
            )?,
            limits: document_limits(limits),
        })
    } else {
        None
    };
    let inventory = project_files(&release, record.side, record.layout, &record.choices)?;
    validate_consumer_inventory(record.layout, &inventory)?;
    let overrides: BTreeMap<_, _> = record
        .local_overrides
        .iter()
        .map(|value| Ok((record.layout.target(path(&value.destination)?), value)))
        .collect::<Result<_>>()?;
    ensure!(
        overrides
            .keys()
            .all(|target| inventory.contains_key(target)),
        "Local override is outside the selected completed inventory"
    );
    let mut owned = BTreeMap::new();
    for (target, file) in inventory {
        let world = file.world_root.is_some()
            || record.initialized_worlds.iter().any(|group| {
                super::super::release::within_world(&file.destination, group)
                    || super::super::layout::collision_key(&file.destination)
                        == super::super::layout::collision_key(group)
            });
        if world || file.policy == FilePolicy::Seed {
            ensure!(
                !overrides.contains_key(&target),
                "A saved local override cannot claim player-owned seed/world content"
            );
            check.report.player_owned_entries += 1;
            check.finding(file.destination,CheckState::PlayerOwned,"Seed/world content belongs to the player and was not traversed or compared with publisher bytes");
        } else {
            owned.insert(target, file);
        }
    }
    if !owned.is_empty() {
        let scopes = owned
            .keys()
            .map(ProjectLayout::path)
            .collect::<Result<Vec<_>>>()?;
        snapshot = snapshot.merge(root.capture(&scopes, limits, cancel)?)?;
    }
    let observed = verification::observed_files_for(&snapshot, owned.keys().cloned())?;
    let total = owned.len();
    for (index, (target, file)) in owned.iter().enumerate() {
        cancel.check()?;
        progress(WorkProgress::CheckingProjectFile {
            path: ProjectLayout::path(target)?.as_str().into(),
            index: index + 1,
            total,
        });
        let result = (|| {
            let current = observed
                .get(target)
                .context("Instance file has no captured observation")?;
            if let Some(value) = overrides.get(target) {
                let baseline = InstalledFile {
                    baseline: file.content()?,
                    policy: empack_core::instance::FilePolicy::Managed,
                };
                let incoming = DesiredFile {
                    content: baseline.baseline.clone(),
                    policy: baseline.policy,
                };
                ensure!(
                    matches!(
                        conflicts::retained(value, &baseline, current, Some(&incoming))?,
                        Some(FileDecision::Preserve)
                    ),
                    "Accepted local override changed or is missing; review instance repair and your local changes"
                );
            } else {
                let expected = file.content()?;
                ensure!(
                    matches!(current,ObservedPath::File(actual) if actual.equivalent(&expected,verification::native_capabilities())),
                    "Managed file is missing or differs in content/permissions; review instance repair before replacing or preserving changes"
                );
                verify_current_file(&root, target, file, cancel)?;
            }
            Ok(())
        })();
        match result {
            Ok(_) => {
                if overrides.contains_key(target) {
                    check.report.verified_overrides += 1;
                } else {
                    check.report.verified_files += 1;
                }
                check.finding(
                    &file.destination,
                    CheckState::Passed,
                    if overrides.contains_key(target) {
                        "Content and attributes match the accepted local override"
                    } else {
                        "Content, attributes and original assertions match the completed release"
                    },
                );
            }
            Err(error) => check.failed(&file.destination, error),
        }
    }
    progress(WorkProgress::RevalidatingInputs);
    root.revalidate(&snapshot, cancel)?;
    if let Some(profile) = profile {
        profile.revalidate(&root, cancel)?;
        check.finding(
            "Prism runtime",
            CheckState::Passed,
            "Effective game and loader match the completed release",
        );
    }
    ensure!(
        !recovery.has_runtime_evidence(&root)?,
        "Runtime ownership appeared during the check; stop it and recheck without clearing its evidence"
    );
    check.finding(
        "Runtime state",
        CheckState::Passed,
        "No managed runtime marker was observed before or after file checks",
    );
    Ok(())
}

#[cfg(test)]
mod tests;
