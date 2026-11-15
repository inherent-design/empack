//! One fresh plan for a valid Prism save, never a replay of completed or uncertain effects.
use super::*;
use crate::engine::{api::PrismProfileChanged, instance::InstanceRecord};

pub(super) struct Repreparation {
    selection: Option<[u8; 32]>,
    prepared: Option<InstanceRecord>,
    used: bool,
}

impl Repreparation {
    pub(super) fn used(&self) -> bool {
        self.used
    }

    pub(super) fn inherit_used(&mut self, used: bool) {
        self.used |= used;
    }

    pub(super) async fn capture(session: &dyn Session, root: &Path, prism: bool) -> Result<Self> {
        Ok(Self {
            selection: if prism {
                Some(selection_identity(session, root).await?)
            } else {
                None
            },
            prepared: None,
            used: false,
        })
    }

    pub(super) async fn bind(
        &mut self,
        session: &dyn Session,
        root: &Path,
        record: &InstanceRecord,
    ) -> Result<()> {
        if let Some(selection) = self.selection {
            ensure!(
                selection_identity(session, root).await? == selection,
                "The installed release, options or publisher enrollment changed; prepare the instance again"
            );
            if let Some(expected) = &self.prepared {
                ensure!(
                    record == expected,
                    "The selected instance changed; prepare the instance again"
                );
            } else {
                self.prepared = Some(record.clone());
            }
        }
        Ok(())
    }

    pub(super) fn check_downloads(&self, downloads: &[String]) -> Result<()> {
        // Successful acquisition is cached before profile revalidation. Never
        // grant another transfer allowance if that cache is no longer available.
        ensure!(
            !self.used || downloads.is_empty(),
            "Launch settings changed and more files are needed; prepare the instance again"
        );
        Ok(())
    }

    pub(super) async fn retry(
        &mut self,
        session: &dyn Session,
        root: &Path,
        error: &anyhow::Error,
    ) -> Result<bool> {
        if self.used || self.selection.is_none() || !eligible(error) {
            return Ok(false);
        }
        session.process().check_cancelled()?;
        ensure!(
            Some(selection_identity(session, root).await?) == self.selection,
            "The installed release, options or publisher enrollment changed; prepare the instance again"
        );
        session.process().check_cancelled()?;
        self.used = true;
        session
            .display()
            .progress()
            .message("Launch settings changed; preparing again.");
        Ok(true)
    }

    pub(super) fn explain(&self, error: anyhow::Error) -> anyhow::Error {
        if self.used && eligible(&error) {
            error.context("Launch settings kept changing. Wait for Prism to finish updating, then launch again.")
        } else {
            error
        }
    }
}

fn eligible(error: &anyhow::Error) -> bool {
    if let Some(retained) = error.downcast_ref::<RetainedExecutionError>() {
        return matches!(
            &*retained.outcome,
            OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(cause))
                if cause.is::<PrismProfileChanged>()
        );
    }
    // A direct cause is produced by read-only preparation, before a plan is returned.
    error.is::<PrismProfileChanged>()
}

async fn selection_identity(session: &dyn Session, root: &Path) -> Result<[u8; 32]> {
    let root = root.to_owned();
    initialize::discover(session, move |mut scope| async move {
        let mut resources = operation_resources().capture;
        resources.scratch_bytes = 0;
        let work = scope.spawn_blocking(
            resources,
            ResourceRequest {
                memory_bytes: 32,
                ..Default::default()
            },
            move |cancel| {
                crate::engine::api::prism_selection_identity(
                    &root,
                    SnapshotLimits::default(),
                    &cancel,
                )
            },
        )?;
        scope.accept(work.wait().await?)?.transpose()
    })
    .await
    .map(|value| *value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::{
        process_runtime::Cancellation, session::LiveProcessProvider,
        session_mocks::MockCommandSession,
    };
    use std::{fs, sync::Arc};

    #[test]
    fn only_valid_profile_changes_before_effects_are_eligible() {
        assert!(eligible(&PrismProfileChanged.into()));
        assert!(!eligible(&anyhow::anyhow!(
            "Prism component settings changed"
        )));
        for (outcome, expected) in [
            (
                ExecutionOutcome::FailedBeforePublication(PrismProfileChanged.into()),
                true,
            ),
            (
                ExecutionOutcome::ExecutionUncertain(PrismProfileChanged.into()),
                false,
            ),
            (
                ExecutionOutcome::RecoveryRequired {
                    operation: "retained".into(),
                    cause: PrismProfileChanged.into(),
                },
                false,
            ),
            (ExecutionOutcome::InterruptedBeforePublication, false),
        ] {
            let error = retained_failure(
                Arc::new(OperationOutcome::Completed(outcome)),
                "fixture".into(),
            );
            assert_eq!(eligible(&error), expected, "{error:#}");
        }
        let mut retry = Repreparation {
            selection: Some([0; 32]),
            prepared: None,
            used: false,
        };
        assert!(retry.check_downloads(&["content".into()]).is_ok());
        retry.inherit_used(true);
        retry.inherit_used(false);
        assert!(retry.used());
        assert!(retry.check_downloads(&[]).is_ok());
        assert!(retry.check_downloads(&["content".into()]).is_err());
    }

    #[tokio::test]
    async fn profile_repreparation_is_bounded_and_cancellation_is_terminal() {
        let root = tempfile::tempdir().unwrap();
        let mut session = MockCommandSession::new();
        let mut retry = Repreparation::capture(&session, root.path(), true)
            .await
            .unwrap();
        let error: anyhow::Error = PrismProfileChanged.into();
        assert!(retry.retry(&session, root.path(), &error).await.unwrap());
        assert!(!retry.retry(&session, root.path(), &error).await.unwrap());
        assert_eq!(
            retry.explain(error).to_string(),
            "Launch settings kept changing. Wait for Prism to finish updating, then launch again."
        );
        let mut retry = Repreparation::capture(&session, root.path(), true)
            .await
            .unwrap();
        let cancellation = Cancellation::default();
        cancellation.cancel();
        session.process_provider = LiveProcessProvider::new().with_cancellation(cancellation);
        let error = retry
            .retry(&session, root.path(), &PrismProfileChanged.into())
            .await
            .unwrap_err();
        assert!(error.is::<crate::application::process_runtime::Interrupted>());
        assert!(!retry.used);
    }

    #[tokio::test]
    async fn repreparation_binds_selection_but_leaves_launcher_and_player_files_independent() {
        for changed in [
            "profile",
            "player",
            "record",
            "subscription",
            "other-consumer",
        ] {
            let root = tempfile::tempdir().unwrap();
            let session = MockCommandSession::new();
            fs::create_dir(root.path().join(".empack")).unwrap();
            fs::write(root.path().join(".empack/instance.json"), b"selection").unwrap();
            let mut retry =
                Repreparation::capture(&session, root.path(), changed != "other-consumer")
                    .await
                    .unwrap();
            let path = match changed {
                "profile" => "mmc-pack.json",
                "player" => "options.txt",
                "record" => ".empack/instance.json",
                _ => ".empack/subscription.json",
            };
            fs::write(root.path().join(path), b"changed").unwrap();
            let result = retry
                .retry(&session, root.path(), &PrismProfileChanged.into())
                .await;
            if matches!(changed, "record" | "subscription") {
                assert_eq!(
                    result.unwrap_err().to_string(),
                    "The installed release, options or publisher enrollment changed; prepare the instance again"
                );
            } else {
                assert_eq!(result.unwrap(), changed != "other-consumer");
            }
        }
    }
}
