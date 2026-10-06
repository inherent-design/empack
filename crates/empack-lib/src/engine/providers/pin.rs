//! File/version selectors without an asserted owner, including version-only dependency edges.
use super::*;
use empack_core::identity::{CurseForgeProjectId, ModrinthProjectId};
use serde_json::Value;

impl ProviderCatalog {
    /// Infer an exact pin's owner from provider evidence, then verify the complete selection.
    pub async fn resolve_pin(
        &self,
        scope: &mut WorkScope,
        pin: PinSelector,
        limits: CatalogLimits,
    ) -> Result<RetainedOutput<ProviderResolution>> {
        Ok(self
            .resolve_pin_budget(scope, pin, limits, transport::RequestBudget::new(limits)?)
            .await?
            .0)
    }
    pub(super) async fn resolve_pin_budget(
        &self,
        scope: &mut WorkScope,
        pin: PinSelector,
        limits: CatalogLimits,
        mut budget: transport::RequestBudget,
    ) -> Result<(RetainedOutput<ProviderResolution>, transport::RequestBudget)> {
        let transport = self.transport.clone();
        let retained = ResourceRequest {
            memory_bytes: limits.response_bytes,
            ..Default::default()
        };
        let worker = scope.spawn(
            ResourceRequest {
                jobs: 1,
                open_files: 1,
                ..retained
            },
            retained,
            move |cancel| async move {
                let bytes = transport.pin(&pin, &mut budget, &cancel).await?;
                Ok::<_, anyhow::Error>((pin, bytes, budget))
            },
        )?;
        let raw = scope.accept(worker.wait().await?)?.transpose()?;
        let retained = parse_resources(raw.1.len() as u64)?;
        let worker = scope.spawn_blocking(
            ResourceRequest {
                jobs: 1,
                ..retained
            },
            retained,
            move |cancel| {
                cancel.check()?;
                let ((pin, bytes, budget), _permit) = raw.into_parts();
                let (pin, bytes) = owned_record(pin, &bytes)?;
                budget.check_deadline()?;
                Ok::<_, anyhow::Error>((pin, bytes, budget))
            },
        )?;
        let record = scope.accept(worker.wait().await?)?.transpose()?;
        let retained = ResourceRequest {
            memory_bytes: limits
                .response_bytes
                .checked_mul(2)
                .ok_or(CatalogError::Limit)?,
            ..Default::default()
        };
        let transport = self.transport.clone();
        let worker = scope.spawn(
            ResourceRequest {
                jobs: 1,
                open_files: 1,
                ..retained
            },
            retained,
            move |cancel| async move {
                let ((pin, file, mut budget), _permit) = record.into_parts();
                let project = transport
                    .project(
                        &ProjectSelector::canonical(pin.project.clone()),
                        &mut budget,
                        &cancel,
                    )
                    .await?;
                Ok::<_, anyhow::Error>((pin, file, project, budget))
            },
        )?;
        let raw = scope.accept(worker.wait().await?)?.transpose()?;
        let retained = parse_resources(
            (raw.1.len() as u64)
                .checked_add(raw.2.len() as u64)
                .ok_or(CatalogError::Limit)?,
        )?;
        let worker = scope.spawn_blocking(
            ResourceRequest {
                jobs: 1,
                ..retained
            },
            retained,
            move |cancel| {
                cancel.check()?;
                let ((pin, file, project, budget), _permit) = raw.into_parts();
                let project =
                    decode_project(&ProjectSelector::canonical(pin.project.clone()), &project)?;
                let resolution = match pin.project {
                    ProviderProjectId::Modrinth(_) => modrinth::selection(project, &pin, &file)?,
                    ProviderProjectId::CurseForge(_) => {
                        curseforge::selection(project, &pin, &file)?
                    }
                };
                cancel.check()?;
                budget.check_deadline()?;
                Ok::<_, anyhow::Error>((resolution, budget))
            },
        )?;
        Ok(split_budget(
            scope.accept(worker.wait().await?)?.transpose()?,
        ))
    }
}
fn owned_record(selection: PinSelector, bytes: &[u8]) -> Result<(ResolvedPin, Vec<u8>)> {
    let value: Value = json(bytes)?;
    match &selection {
        PinSelector::ModrinthVersion(id) => {
            ensure!(
                value.get("id").and_then(Value::as_str) == Some(id.as_str()),
                CatalogError::Identity
            );
            let owner = value
                .get("project_id")
                .and_then(Value::as_str)
                .ok_or(CatalogError::InvalidRecord)?;
            Ok((
                ResolvedPin {
                    project: ProviderProjectId::Modrinth(ModrinthProjectId::parse(owner)?),
                    selection,
                },
                bytes.to_vec(),
            ))
        }
        PinSelector::CurseForgeFile(id) => {
            let files = value
                .get("data")
                .and_then(Value::as_array)
                .ok_or(CatalogError::InvalidRecord)?;
            ensure!(!files.is_empty(), CatalogError::NotFound);
            ensure!(files.len() == 1, CatalogError::Identity);
            let file = &files[0];
            ensure!(
                file.get("id").and_then(Value::as_u64) == Some(id.get())
                    && file.get("gameId").and_then(Value::as_u64) == Some(432),
                CatalogError::Identity
            );
            let owner = file
                .get("modId")
                .and_then(Value::as_u64)
                .ok_or(CatalogError::InvalidRecord)?;
            Ok((
                ResolvedPin {
                    project: ProviderProjectId::CurseForge(CurseForgeProjectId::parse(
                        &owner.to_string(),
                    )?),
                    selection,
                },
                serde_json::to_vec(&serde_json::json!({"data":file}))?,
            ))
        }
    }
}
#[cfg(test)]
mod tests;
