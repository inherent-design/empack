//! Exact in-memory input continuation; neither old approval nor changed observations survive.
use super::*;
impl Engine {
    pub async fn resume_instance_files(
        &self,
        pending: PreparationContinuation,
        files: BTreeMap<String, PathBuf>,
    ) -> Result<Preparation> {
        ensure!(
            Arc::ptr_eq(&self.owner, &pending.prepared.owner),
            "Continuation belongs to another engine"
        );
        ensure!(
            matches!(&**pending.prepared.data, PreparedKind::Instance(_)),
            "Continuation is not an instance preparation"
        );
        let prior = (*pending.prepared.data).map(|kind| match kind {
            PreparedKind::Instance(value) => *value,
            _ => unreachable!(),
        });
        let config = self.config.clone();
        let access = self
            .catalog
            .as_ref()
            .map(|(catalog, _)| catalog.availability())
            .unwrap_or_default();
        let owner = self.owner.clone();
        let (sender, receiver) = oneshot::channel();
        let mut operation = self
            .preparations
            .start_ephemeral(move |mut scope| async move {
                let result = async {
                    let (resources, _) =
                        project_change::resources(prior.instance.bytes()?, &config)?;
                    let work = scope.spawn_blocking(
                        resources,
                        ResourceRequest::default(),
                        move |cancel| {
                            let (mut prior, reservation) = prior.into_parts();
                            prior.content =
                                prior
                                    .instance
                                    .resume_files(&prior.content, &files, &cancel)?;
                            prior.downloads = prior
                                .instance
                                .needed()
                                .filter(|file| !prior.content.contains_key(&file.key))
                                .map(|file| prior.instance.download(file))
                                .collect::<Result<_>>()?;
                            prior.view.downloads = prior
                                .downloads
                                .iter()
                                .map(|file| file.key.clone())
                                .collect();
                            let mut manual = manual_inputs(&prior.downloads, access)?;
                            for known in &prior.view.manual {
                                if !prior.content.contains_key(&known.key)
                                    && !manual.iter().any(|need| need.key == known.key)
                                {
                                    manual.push(known.clone());
                                }
                            }
                            prior.view.manual = manual;
                            prior.view.plan = PlanId(
                                NEXT_PLAN
                                    .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| {
                                        id.checked_add(1)
                                    })
                                    .map_err(|_| anyhow::anyhow!("Plan identifier exhausted"))?,
                            );
                            Ok::<_, anyhow::Error>(RetainedOutput::from_parts(prior, reservation))
                        },
                    )?;
                    let data = scope
                        .accept(work.wait().await?)?
                        .transpose()?
                        .into_parts()
                        .0
                        .map(|value| PreparedKind::Instance(Box::new(value)));
                    Ok::<_, anyhow::Error>(
                        PreparedOperation {
                            owner,
                            view: Box::new(data.view()),
                            data: Box::new(data),
                        }
                        .classify(),
                    )
                }
                .await;
                let _ = sender.send(result);
                Ok(())
            })?;
        let outcome = operation.wait().await;
        if let OperationOutcome::Failed(error) = &*outcome {
            return Err(error.clone().into());
        }
        receiver
            .await
            .context("Instance continuation result was not retained")?
    }
}
