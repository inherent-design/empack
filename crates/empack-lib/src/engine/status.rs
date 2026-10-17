//! Bounded metadata inspection. Reports carry no execution or publisher authority.
use super::{
    documents::DocumentCodec,
    native,
    publication::{Publisher, RecoveryReader},
    snapshot::ProjectReadRoot,
};
use crate::application::process_runtime::Cancellation;
use anyhow::{Context, Result, ensure};
use empack_core::path::{PathSyntax, PortableRelPath};
use serde::{Deserialize, Serialize};
use std::{
    io::{self, Read},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkspaceContext {
    Author,
    Publisher,
    Instance,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecordState {
    Missing,
    Valid,
    Stale,
    Invalid,
    Unavailable,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusFinding {
    pub code: String,
    pub message: String,
    /// Arguments relative to the selected workspace; no interpolated shell command.
    pub next: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextStatus {
    pub context: WorkspaceContext,
    pub state: RecordState,
    pub name: Option<String>,
    pub version: Option<String>,
    pub minecraft: Option<String>,
    pub loader: Option<String>,
    /// Exact release ID for instances, raw lock-document SHA-256 for authors.
    pub exact: Option<String>,
    pub lock: Option<RecordState>,
    pub game_directory: Option<String>,
    pub side: Option<String>,
    pub update_owner: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceStatus {
    pub schema: u32,
    pub root: PathBuf,
    pub selected: Option<WorkspaceContext>,
    pub contexts: Vec<ContextStatus>,
    pub findings: Vec<StatusFinding>,
    /// Status checks metadata, not the installed payload bytes or runtime compatibility.
    pub content_verified: bool,
}
impl WorkspaceStatus {
    fn finding(&mut self, code: &str, message: &str, next: &[&str]) {
        self.findings.push(StatusFinding {
            code: code.into(),
            message: message.into(),
            next: next.iter().map(|s| (*s).into()).collect(),
        });
    }
    pub fn text(&self) -> String {
        let mut lines = vec![format!("Workspace: {}", self.root.display())];
        for context in &self.contexts {
            if self
                .selected
                .is_some_and(|selected| selected != context.context)
            {
                continue;
            }
            lines.push(format!("{:?}: {:?}", context.context, context.state));
            if let Some(name) = &context.name {
                lines.push(format!(
                    "Pack: {name} {}",
                    context
                        .version
                        .as_deref()
                        .unwrap_or("(version unavailable)")
                ));
            }
            if let Some(exact) = &context.exact {
                let label = if context.context == WorkspaceContext::Author {
                    "Lock document SHA-256"
                } else {
                    "Exact release"
                };
                lines.push(format!("{label}: {exact}"));
            }
            if let Some(lock) = context.lock {
                lines.push(format!("Resolution lock: {lock:?}"));
            }
            if let Some(game) = &context.game_directory {
                lines.push(format!("Game directory: {game}"));
            }
            if let Some(mc) = &context.minecraft {
                lines.push(format!(
                    "Minecraft: {mc}; loader: {}",
                    context.loader.as_deref().unwrap_or("unknown")
                ));
            }
            lines.push(format!("Updates: {}", context.update_owner));
        }
        for finding in &self.findings {
            lines.push(format!("{}: {}", finding.code, finding.message));
            if !finding.next.is_empty() {
                lines.push(format!(
                    "Next in this workspace: empack {}",
                    finding.next.join(" ")
                ));
            }
        }
        lines.push("Metadata inspection only; content bytes have not been verified.".into());
        lines.join("\n")
    }
}
fn empty(context: WorkspaceContext) -> ContextStatus {
    ContextStatus {
        context,
        state: RecordState::Missing,
        name: None,
        version: None,
        minecraft: None,
        loader: None,
        exact: None,
        lock: None,
        game_directory: None,
        side: None,
        update_owner: "unknown".into(),
    }
}
fn missing(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<io::Error>()
        .is_some_and(|e| e.kind() == io::ErrorKind::NotFound)
}
fn present(root: &ProjectReadRoot, name: &str) -> Result<bool> {
    let path = PortableRelPath::parse(name, PathSyntax::ProjectContent)?;
    let (parent, leaf) = match native::parent(&root.directory, &path) {
        Ok(v) => v,
        Err(e) if missing(&e) => return Ok(false),
        Err(e) => return Err(e),
    };
    match parent.symlink_metadata(&leaf) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}
/// Read only the selected regular document, never recurse into a directory-valued record.
fn read(root: &ProjectReadRoot, name: &str, cancel: &Cancellation) -> Result<Option<Vec<u8>>> {
    cancel.check()?;
    let path = PortableRelPath::parse(name, PathSyntax::ProjectContent)?;
    let (parent, leaf) = match native::parent(&root.directory, &path) {
        Ok(v) => v,
        Err(e) if missing(&e) => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut file = match native::open_file(&parent, &leaf) {
        Ok(v) => v,
        Err(e) if missing(&e) => return Ok(None),
        Err(e) => return Err(e),
    };
    let metadata = file.metadata()?;
    let limit = super::documents::MAX_DOCUMENT_BYTES as u64;
    ensure!(
        metadata.is_file() && metadata.len() <= limit,
        "Status document must be a bounded regular file"
    );
    let mut bytes = Vec::new();
    (&mut file).take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "Status document grew beyond limit"
    );
    let after = file.metadata()?;
    ensure!(
        metadata.len() == after.len() && metadata.modified()? == after.modified()?,
        "Status document changed during inspection"
    );
    let reopened = native::open_file(&parent, &leaf)?;
    ensure!(
        native::identity(&file)? == native::identity(&reopened)?,
        "Status document was replaced during inspection"
    );
    cancel.check()?;
    Ok(Some(bytes))
}
fn author(root: &ProjectReadRoot, cancel: &Cancellation) -> Result<ContextStatus> {
    let mut result = empty(WorkspaceContext::Author);
    let Some(bytes) = read(root, "empack.yml", cancel)? else {
        return Ok(result);
    };
    let bindings = read(root, "empack.bindings.yml", cancel)?;
    let intent = DocumentCodec.decode_authoring(&bytes, bindings.as_deref(), "empack.yml")?;
    result.state = RecordState::Valid;
    result.name = Some(intent.intent().metadata.name.clone());
    result.version = Some(intent.intent().metadata.version.clone());
    result.minecraft = Some(intent.intent().runtime.minecraft.as_str().into());
    result.loader = Some(format!("{:?}", intent.intent().runtime.loader));
    result.update_owner =
        "author: update selects dependencies; sync applies recorded intent".into();
    result.lock = Some(match read(root, "empack.lock", cancel)? {
        None => RecordState::Missing,
        Some(bytes) => match DocumentCodec.decode_prior_lock(&bytes, "empack.lock") {
            Err(_) => RecordState::Invalid,
            Ok(lock) => {
                if lock.bind(&intent).is_ok() {
                    result.exact = Some(super::release::hash(&bytes));
                    RecordState::Valid
                } else {
                    RecordState::Stale
                }
            }
        },
    });
    Ok(result)
}
fn instance(root: &ProjectReadRoot, cancel: &Cancellation) -> Result<ContextStatus> {
    let record = super::instance::InstanceRecord::decode(
        &read(root, ".empack/instance.json", cancel)?.context("No completed instance exists")?,
    )?;
    ensure!(
        record.root == super::publication::root_key(root)?,
        "Instance belongs to another root"
    );
    let release = super::release::DecodedRelease::decode(
        &read(
            root,
            &format!(".empack/releases/{}.json", record.release),
            cancel,
        )?
        .context("Retained release descriptor is missing")?,
    )?;
    ensure!(
        release.id() == record.release && release.document().pack == record.pack,
        "Retained release differs from instance record"
    );
    let mut result = empty(WorkspaceContext::Instance);
    result.state = RecordState::Valid;
    result.name = Some(record.pack);
    result.version = Some(release.document().version.clone());
    result.exact = Some(record.release);
    result.minecraft = Some(release.document().runtime.minecraft.clone());
    result.loader = Some(format!("{:?}", release.document().runtime.loader));
    result.game_directory = Some(record.layout.directory().into());
    result.side = Some(format!("{:?}", record.side).to_lowercase());
    result.update_owner = "snapshot: install an explicitly selected exact release".into();
    Ok(result)
}
/// Inspect selected native control records without network access, writes or payload scans.
/// Findings distinguish invalid/unavailable state from an empty workspace. Context ambiguity
/// is reported explicitly; consumers must not infer mutation authority from this result.
pub fn inspect_workspace(
    path: &Path,
    state: &Path,
    context: Option<WorkspaceContext>,
    cancel: &Cancellation,
) -> Result<WorkspaceStatus> {
    ensure!(
        path.is_absolute() && state.is_absolute(),
        "Status requires absolute workspace and state paths"
    );
    let root = ProjectReadRoot::open(path)?;
    let mut report = WorkspaceStatus {
        schema: 1,
        root: path.into(),
        selected: context,
        contexts: Vec::new(),
        findings: Vec::new(),
        content_verified: false,
    };
    if let Some(publisher) = Publisher::open_existing(state)? {
        if let Some(recovery) = publisher.inspect_recovery(&root)? {
            report.finding(
                "publication-recovery-required",
                &format!(
                    "Interrupted publication {} must be inspected before reading live documents",
                    recovery.operation
                ),
                &["recover", "inspect"],
            );
            return Ok(report);
        }
        if publisher.observe_runtime(&root)?.is_some() {
            report.finding("runtime-retirement-unconfirmed", "Managed runtime is active or its retirement is unconfirmed; stop all related processes before recovery", &["instance", "inspect"]);
        }
    }
    let _guard = RecoveryReader::new(state.into()).enter(&root)?;
    for (kind, markers) in [
        (
            WorkspaceContext::Author,
            &["empack.yml", "empack.bindings.yml", "empack.lock"][..],
        ),
        (
            WorkspaceContext::Instance,
            &[".empack/instance.json", ".empack/subscription.json"][..],
        ),
        (
            WorkspaceContext::Publisher,
            &["dist/releases", "dist/channels"][..],
        ),
    ] {
        let detected = markers.iter().try_fold(false, |found, name| {
            present(&root, name).map(|p| found || p)
        });
        let found = match detected {
            Ok(found) => found,
            Err(_) => {
                report.finding(
                    "context-unavailable",
                    "A context marker cannot be safely inspected",
                    &[],
                );
                true
            }
        };
        if !found && context != Some(kind) {
            continue;
        }
        let inspected = if !found {
            Ok(empty(kind))
        } else {
            match kind {
                WorkspaceContext::Author => author(&root, cancel),
                WorkspaceContext::Instance => instance(&root, cancel),
                WorkspaceContext::Publisher => publisher(&root, cancel),
            }
        };
        let mut value = inspected.unwrap_or_else(|_| {
            let mut value = empty(kind);
            value.state = RecordState::Invalid;
            value
        });
        if kind == WorkspaceContext::Instance && found {
            match read(&root, ".empack/subscription.json", cancel) {
                Ok(Some(bytes)) => {
                    match super::instance::subscription::SubscriptionRecord::decode(&bytes) {
                        Ok(subscription)
                            if subscription.root == super::publication::root_key(&root)? =>
                        {
                            value.update_owner = if subscription.keys.is_empty() {
                                "publisher subscription revoked".into()
                            } else {
                                format!(
                                    "publisher channel {} (authenticate before applying)",
                                    subscription.channel
                                )
                            };
                        }
                        _ => report.finding(
                            "subscription-invalid",
                            "Publisher enrollment is invalid or belongs to another root",
                            &[],
                        ),
                    }
                }
                Err(_) => report.finding(
                    "subscription-unavailable",
                    "Publisher enrollment cannot be safely read",
                    &[],
                ),
                Ok(None) => {}
            }
        }
        if value.state != RecordState::Valid {
            report.finding(
                "context-not-ready",
                &format!("{kind:?} records are {:?}", value.state),
                &[],
            );
        }
        if value.lock.is_some_and(|lock| lock != RecordState::Valid) {
            report.finding(
                "lock-not-current",
                "Author intent has no valid matching resolution; preview synchronization",
                &["--dry-run", "sync"],
            );
        }
        report.contexts.push(value);
    }
    if context.is_none() {
        if report.contexts.len() == 1 {
            report.selected = Some(report.contexts[0].context);
        } else if report.contexts.len() > 1 {
            report.finding("context-ambiguous", "Multiple workspace contexts found; select status --context author, publisher or instance", &[]);
        } else {
            report.finding(
                "workspace-empty",
                "No author, publisher or instance records found",
                &["init", "--help"],
            );
        }
    }
    for (kind, name, command) in [
        (
            super::continuation_store::Kind::Import,
            "import",
            &["init", "--continue"][..],
        ),
        (
            super::continuation_store::Kind::Synchronization,
            "sync",
            &["sync", "--continue"][..],
        ),
        (
            super::continuation_store::Kind::Instance,
            "instance",
            &["instance", "continue"][..],
        ),
    ] {
        match super::continuation_store::observe(state, kind, path, cancel) {
            Ok(Some(_)) => report.finding("pending-input", &format!("Retained {name} input exists; resume will validate it and request fresh approval"), command),
            Ok(None) => {},
            Err(_) => report.finding("pending-unavailable", &format!("Retained {name} state cannot be inspected"), &[]),
        }
    }
    match super::api::inspect_saved_build_presence(state, path, cancel) {
        Ok(true) => report.finding(
            "pending-input",
            "Retained build input exists; resume will validate it and request fresh approval",
            &["build", "--continue"],
        ),
        Ok(false) => {}
        Err(_) => report.finding(
            "pending-unavailable",
            "Retained build state cannot be inspected",
            &[],
        ),
    }
    root.check_binding()?;
    cancel.check()?;
    Ok(report)
}
fn publisher(root: &ProjectReadRoot, cancel: &Cancellation) -> Result<ContextStatus> {
    use cap_fs_ext::DirExt;
    let mut result = empty(WorkspaceContext::Publisher);
    // Inspect control directory shape only. A status report does not authenticate hosted releases.
    for name in ["dist/releases", "dist/channels"] {
        cancel.check()?;
        let path = PortableRelPath::parse(name, PathSyntax::ProjectContent)?;
        let (parent, leaf) = match native::parent(&root.directory, &path) {
            Ok(v) => v,
            Err(e) if missing(&e) => continue,
            Err(e) => return Err(e),
        };
        match parent.open_dir_nofollow(&leaf) {
            Ok(directory) => {
                native::directory_identity(&directory)?;
                // Recognized storage is not evidence of a valid signed release.
                result.state = RecordState::Unavailable;
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    result.update_owner = "publisher workspace: immutable releases and channels; contents not authenticated by status".into();
    Ok(result)
}

#[cfg(test)]
mod tests;
