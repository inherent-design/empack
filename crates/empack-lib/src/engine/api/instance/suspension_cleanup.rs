//! Read-only pending-state selection, followed by conditional explicit cleanup.
use super::*;
use crate::engine::continuation_store::{self as store, Kind};
pub struct PendingInstanceCleanup {
    owner: Arc<()>,
    selected: store::Cleanup,
}
impl Engine {
    /// Invalid and stale records can be selected without decoding or deleting them.
    pub async fn observe_pending_instance(
        &self,
        root: PathBuf,
    ) -> Result<Option<PendingInstanceCleanup>> {
        let state = self.config.state_root.clone();
        let owner = self.owner.clone();
        let (sender, receiver) = oneshot::channel();
        let mut handle = self
            .preparations
            .start_ephemeral(move |mut scope| async move {
                let result = async {
                    let work = scope.spawn_blocking(
                        ResourceRequest {
                            jobs: 1,
                            memory_bytes: 256 << 10,
                            open_files: 6,
                            ..Default::default()
                        },
                        ResourceRequest::default(),
                        move |cancel| store::observe(&state, Kind::Instance, &root, &cancel),
                    )?;
                    Ok::<_, anyhow::Error>(
                        scope
                            .accept(work.wait().await?)?
                            .transpose()?
                            .into_parts()
                            .0
                            .map(|selected| PendingInstanceCleanup { owner, selected }),
                    )
                }
                .await;
                let _ = sender.send(result);
                Ok(())
            })?;
        let outcome = handle.wait().await;
        if let OperationOutcome::Failed(error) = &*outcome {
            return Err(error.clone().into());
        }
        receiver
            .await
            .context("Pending instance observation was not retained")?
    }
    pub async fn discard_pending_instance(&self, selected: PendingInstanceCleanup) -> Result<bool> {
        ensure!(
            Arc::ptr_eq(&self.owner, &selected.owner),
            "Pending instance observation belongs to another engine"
        );
        let (sender, receiver) = oneshot::channel();
        let mut handle = self.preparations.start_ephemeral(move |scope| async move {
            let result = async {
                let work = scope.spawn_blocking(
                    ResourceRequest {
                        jobs: 1,
                        memory_bytes: 256 << 10,
                        open_files: 6,
                        ..Default::default()
                    },
                    ResourceRequest::default(),
                    move |cancel| store::discard(&selected.selected, &cancel),
                )?;
                Ok::<_, anyhow::Error>(
                    scope
                        .accept_publication(work.wait().await?)?
                        .transpose()?
                        .into_parts()
                        .0,
                )
            }
            .await;
            let _ = sender.send(result);
            Ok(())
        })?;
        let outcome = handle.wait().await;
        if let OperationOutcome::Failed(error) = &*outcome {
            return Err(error.clone().into());
        }
        receiver
            .await
            .context("Pending instance cleanup was not retained")?
    }
}
