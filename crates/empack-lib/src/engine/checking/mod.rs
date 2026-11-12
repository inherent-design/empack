//! Read-only project evidence. A check result is never preparation or a native grant.
use super::{
    diagnostics::{Diagnostic, DiagnosticPhase},
    project::{ProjectReader, WorkspaceSnapshot},
    providers::{ClosureLimits, ProviderCatalog},
    publication::RecoveryReader,
    resources::ResourceRequest,
    runtime::{WorkProgress, WorkScope},
    snapshot::{NativeSnapshot, ProjectReadRoot, SnapshotLimits},
};
use crate::application::process_runtime::Cancellation;
use anyhow::{Context, Result, ensure};
use empack_core::{
    digest::DigestSet,
    model::*,
    path::{PathSyntax, PortableRelPath},
};
use serde::Serialize;
use std::{collections::BTreeMap, path::PathBuf};

mod local;
mod online;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CheckScope {
    Documents,
    Recorded,
    Online,
}
#[derive(Clone)]
pub struct CheckRequest {
    pub project: PathBuf,
    pub state: PathBuf,
    pub scope: CheckScope,
    pub files: SnapshotLimits,
    pub providers: ClosureLimits,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CheckState {
    Passed,
    Failed,
    Unverified,
    Reference,
}
#[derive(Debug, Serialize)]
pub struct CheckFinding {
    pub subject: String,
    pub state: CheckState,
    pub required: bool,
    pub message: String,
    pub diagnostic: Option<Diagnostic>,
}
#[derive(Debug, Serialize)]
pub struct CheckReport {
    pub schema: u32,
    pub scope: CheckScope,
    pub pack: Option<String>,
    pub author_schema: Option<u64>,
    pub authoring: Option<super::documents::AuthoringInspection>,
    pub lock: &'static str,
    pub verified_files: usize,
    pub references: usize,
    pub findings: Vec<CheckFinding>,
    pub complete: bool,
}
pub struct ProjectCheck {
    pub report: CheckReport,
    pub result: Result<()>,
}
struct CheckedContent {
    digests: DigestSet,
    bytes: u64,
}
struct Inspection {
    report: CheckReport,
    failure: Option<anyhow::Error>,
    root: ProjectReadRoot,
    documents: Vec<NativeSnapshot>,
    project: Option<ResolvedProject>,
    workspace: Option<WorkspaceSnapshot>,
    observed: BTreeMap<(DependencyKey, FileSlot), CheckedContent>,
}
impl Inspection {
    fn finding(
        &mut self,
        subject: impl Into<String>,
        state: CheckState,
        message: impl Into<String>,
    ) {
        self.report.findings.push(CheckFinding {
            subject: subject.into(),
            state,
            required: false,
            message: message.into(),
            diagnostic: None,
        });
    }
    fn passed(&mut self, subject: impl Into<String>, message: impl Into<String>) {
        self.report.findings.push(CheckFinding {
            subject: subject.into(),
            state: CheckState::Passed,
            required: true,
            message: message.into(),
            diagnostic: None,
        });
    }
    fn failed(&mut self, subject: impl Into<String>, error: anyhow::Error) {
        self.report.findings.push(CheckFinding {
            subject: subject.into(),
            state: CheckState::Failed,
            required: true,
            message: format!("{error:#}").chars().take(8192).collect(),
            diagnostic: Some(Diagnostic::from_error(
                &error,
                DiagnosticPhase::Verification,
            )),
        });
        if self.failure.is_none() {
            self.failure = Some(error);
        }
    }
}
/// Inspect documents independently, then applicable recorded bytes and optional fresh exact
/// metadata. Temporary archive verification may use owned scratch; no cache/state is written.
pub async fn check_project(
    scope: &mut WorkScope,
    request: CheckRequest,
    catalog: &ProviderCatalog,
) -> Result<ProjectCheck> {
    ensure!(
        request.project.is_absolute() && request.state.is_absolute(),
        "Check requires absolute project/state paths"
    );
    let reporter = scope.progress_reporter();
    let local_request = request.clone();
    let work = scope.spawn_blocking(
        ResourceRequest {
            jobs: 1,
            memory_bytes: 96 << 20,
            open_files: 16,
            ..Default::default()
        },
        ResourceRequest {
            memory_bytes: 64 << 20,
            open_files: 4,
            ..Default::default()
        },
        move |cancel| local::inspect(&local_request, &cancel, &reporter),
    )?;
    let (mut inspection, _retained) = scope.accept(work.wait().await?)?.transpose()?.into_parts();
    if request.scope == CheckScope::Online
        && inspection.project.is_some()
        && let Err(error) =
            online::inspect(scope, &mut inspection, catalog, request.providers).await
    {
        inspection.failed("Fresh provider evidence", error);
    }
    // Reacquire read coordination and check all captured inputs after network work.
    scope.report_progress(WorkProgress::RevalidatingInputs);
    let checked = scope.spawn_blocking(
        ResourceRequest {
            jobs: 1,
            memory_bytes: 16 << 20,
            open_files: 8,
            ..Default::default()
        },
        ResourceRequest::default(),
        move |cancel| {
            let revalidated = (|| {
                let recovery = RecoveryReader::new(request.state);
                let _guard = recovery.enter(&inspection.root)?;
                for snapshot in &inspection.documents {
                    inspection.root.revalidate(snapshot, &cancel)?;
                }
                if let Some(workspace) = &inspection.workspace {
                    workspace
                        .root()
                        .revalidate(workspace.observations(), &cancel)?;
                }
                Ok::<_, anyhow::Error>(())
            })();
            if let Err(error) = revalidated {
                inspection.failed("Final input check", error);
            }
            inspection.report.complete = inspection.failure.is_none();
            Ok::<_, anyhow::Error>(ProjectCheck {
                report: inspection.report,
                result: inspection.failure.map_or(Ok(()), Err),
            })
        },
    )?;
    let (result, _result_resources) = scope
        .accept(checked.wait().await?)?
        .transpose()?
        .into_parts();
    Ok(result)
}

#[cfg(test)]
mod tests;
