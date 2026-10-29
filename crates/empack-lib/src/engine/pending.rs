//! Shared presentation over existing continuation records; never execution authority.
use super::{
    continuation_store::{self as store, Kind},
    documents::DocumentCodec,
    project::ProjectReader,
    publication::RecoveryReader,
    snapshot::SnapshotLimits,
};
use crate::application::process_runtime::Cancellation;
use anyhow::{Context, Result, ensure};
use empack_core::model::{AcquisitionSpec, ExpectedContent, ResolvedPin, ResolvedProject};
use serde::Serialize;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PendingKind {
    Build,
    Import,
    Sync,
    Instance,
}
/// Exact provider identity for presentation; it does not authorize acquisition.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PendingProviderSelection {
    pub provider: &'static str,
    pub project: String,
    pub selection: String,
}
impl From<&ResolvedPin> for PendingProviderSelection {
    fn from(pin: &ResolvedPin) -> Self {
        use empack_core::identity::{PinSelector, ProviderProjectId};
        Self {
            provider: match pin.project {
                ProviderProjectId::Modrinth(_) => "modrinth",
                ProviderProjectId::CurseForge(_) => "curseforge",
            },
            project: pin.project.to_string(),
            selection: match &pin.selection {
                PinSelector::ModrinthVersion(id) => id.to_string(),
                PinSelector::CurseForgeFile(id) => id.to_string(),
            },
        }
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct PendingInput {
    pub key: String,
    pub name: Option<String>,
    pub provider_selection: Option<PendingProviderSelection>,
    pub filename: Option<String>,
    pub digests: Vec<(String, String)>,
    pub bytes: Option<u64>,
    pub download_page: Option<String>,
    pub supplied_candidate: Option<PathBuf>,
    /// Retained bytes are reverified on continuation; metadata is never a completion proof.
    pub verification: String,
    pub needed: Option<bool>,
    #[serde(skip)]
    pub(crate) expected: Option<ExpectedContent>,
    #[serde(skip)]
    pub(crate) pin: Option<ResolvedPin>,
}
#[derive(Debug, Clone, Serialize)]
pub struct PendingOperation {
    pub id: String,
    pub kind: PendingKind,
    pub root: PathBuf,
    pub state: String,
    pub inputs: Vec<PendingInput>,
    pub requirements_complete: bool,
    pub diagnostic: Option<String>,
    pub next: Vec<String>,
}
#[derive(Debug, Clone, Serialize)]
pub struct PendingReport {
    pub schema: u32,
    pub root: PathBuf,
    pub operations: Vec<PendingOperation>,
}
pub(crate) fn operation_id(kind: &str, name: &str, content: &[u8; 32]) -> String {
    super::release::hash(format!("{kind}\0{name}\0{}", super::release::hex(content)).as_bytes())
}
impl PendingReport {
    pub fn select(&self, id: &str) -> Result<&PendingOperation> {
        ensure!(
            !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_hexdigit()),
            "Pending ID must be a hexadecimal identity or unique prefix"
        );
        let matching: Vec<_> = self
            .operations
            .iter()
            .filter(|op| op.id.starts_with(id))
            .collect();
        ensure!(
            !matching.is_empty(),
            "No pending operation matches this ID in the selected root"
        );
        ensure!(
            matching.len() == 1,
            "Pending ID is ambiguous; supply more characters"
        );
        Ok(matching[0])
    }
    pub fn text(&self) -> String {
        let mut lines = vec![format!("Pending inputs in {}", self.root.display())];
        for op in &self.operations {
            lines.push(format!("{} {:?}: {}", &op.id[..12], op.kind, op.state));
            if let Some(message) = &op.diagnostic {
                lines.push(message.clone());
            }
            for input in &op.inputs {
                lines.push(format!(
                    "  {}: {}; filename {}; bytes {}; {}; digests {}",
                    input.key,
                    input.name.as_deref().unwrap_or("unknown name"),
                    input.filename.as_deref().unwrap_or("unknown"),
                    input.bytes.map_or("unknown".into(), |n| n.to_string()),
                    input.verification,
                    if input.digests.is_empty() {
                        "unknown".into()
                    } else {
                        input
                            .digests
                            .iter()
                            .map(|(kind, value)| format!("{kind}:{value}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    }
                ));
                lines.push(format!(
                    "    provider {}; permitted page {}; candidate {}",
                    input.provider_selection.as_ref().map_or_else(
                        || "unknown".into(),
                        |selected| format!(
                            "{}:{}@{}",
                            selected.provider, selected.project, selected.selection
                        )
                    ),
                    input
                        .download_page
                        .as_deref()
                        .unwrap_or("unknown (resolve explicitly with --open-downloads)"),
                    input
                        .supplied_candidate
                        .as_ref()
                        .map_or("unknown".into(), |p| p.display().to_string())
                ));
            }
            lines.push(format!("Next in this root: empack {}", op.next.join(" ")));
        }
        if self.operations.is_empty() {
            lines.push("No saved pending operations".into());
        }
        lines.push("Metadata only; continuation revalidates recipes, trust and supplied bytes before fresh approval.".into());
        lines.join("\n")
    }
}
fn empty(id: String, kind: PendingKind, root: &Path) -> PendingOperation {
    PendingOperation {
        next: vec!["pending".into(), "resume".into(), id.clone()],
        id,
        kind,
        root: root.into(),
        state: "unverified".into(),
        inputs: Vec::new(),
        requirements_complete: false,
        diagnostic: None,
    }
}
fn input(
    key: String,
    name: Option<String>,
    filename: Option<String>,
    expected: Option<ExpectedContent>,
    pin: Option<ResolvedPin>,
    retained: bool,
    needed: Option<bool>,
) -> PendingInput {
    let mut digests = expected
        .as_ref()
        .and_then(|e| e.digests.as_ref())
        .map_or_else(Vec::new, |set| {
            set.values()
                .iter()
                .map(|d| (d.algorithm().name().into(), d.hex()))
                .collect()
        });
    if let Some(id) = expected
        .as_ref()
        .and_then(|e| e.accepted_observation.as_ref())
    {
        digests.push(("selected-sha256".into(), super::release::hex(id.bytes())));
    }
    PendingInput {
        key,
        name,
        filename,
        provider_selection: pin.as_ref().map(PendingProviderSelection::from),
        digests,
        bytes: expected.as_ref().and_then(|e| e.size),
        download_page: None,
        supplied_candidate: None,
        verification: if retained {
            "retained; verification required"
        } else {
            "not supplied; verification required"
        }
        .into(),
        needed,
        expected,
        pin,
    }
}
pub(crate) fn locked_selector(dependency: &str, slot: &str) -> String {
    let encode = |s: &str| {
        percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string()
    };
    format!("locked:{}:{}", encode(dependency), encode(slot))
}
fn locked_inputs(
    op: &mut PendingOperation,
    project: &ResolvedProject,
    retained: &BTreeSet<String>,
) {
    for (key, dependency) in &project.lock().dependencies {
        for file in dependency.files.as_slice() {
            let pin = match &file.acquisition {
                AcquisitionSpec::Local(_) | AcquisitionSpec::Embedded { .. } => continue,
                AcquisitionSpec::Provider { pin, .. } => Some(pin.clone()),
                AcquisitionSpec::Manual { pin, .. } => pin.clone(),
                AcquisitionSpec::ProviderArchiveMember { archive, .. } => Some(archive.pin.clone()),
                AcquisitionSpec::Url(_) => None,
            };
            let selector = if op.kind == PendingKind::Build {
                locked_selector(key.as_str(), file.slot.as_str())
            } else {
                format!("{}/{}", key.as_str(), file.slot.as_str())
            };
            let filename = file
                .placements
                .as_slice()
                .first()
                .and_then(|p| p.destination.relative().as_str().rsplit('/').next())
                .map(str::to_owned);
            op.inputs.push(input(
                selector.clone(),
                Some(dependency.title.clone()),
                filename,
                Some(file.expected.clone()),
                pin,
                retained.contains(&selector),
                None,
            ));
        }
    }
}
fn details(
    op: &mut PendingOperation,
    bytes: &[u8],
    state: &Path,
    cancel: &Cancellation,
) -> Result<()> {
    let record: serde_json::Value =
        serde_json::from_slice(bytes).context("Invalid pending record")?;
    let supported = match op.kind {
        PendingKind::Build => matches!(record["schema"].as_u64(), Some(4 | 5)),
        PendingKind::Sync => record["schema"].as_u64() == Some(2),
        _ => record["schema"].as_u64() == Some(1),
    };
    ensure!(
        supported,
        "Unsupported saved record schema; use the original workflow continuation or explicitly discard this ID"
    );
    let kind = match op.kind {
        PendingKind::Instance => Kind::Instance,
        PendingKind::Import => Kind::Import,
        _ => Kind::Synchronization,
    };
    let binding = store::bind_for(kind, &op.root, cancel)?.1;
    let current = if op.kind == PendingKind::Build {
        serde_json::from_value::<Vec<Option<[u8; 32]>>>(record["documents"].clone())?
            == binding.documents
    } else {
        serde_json::from_value::<store::Binding>(record["binding"].clone())? == binding
    };
    if !current {
        op.state = "stale".into();
        op.diagnostic = Some(
            "Saved target identity or documents changed; continuation will reject this record"
                .into(),
        );
        return Ok(());
    }
    match op.kind {
        PendingKind::Build => {
            let workspace = ProjectReader::new(RecoveryReader::new(state.to_owned())).capture(
                &op.root,
                &[],
                SnapshotLimits::default(),
                cancel,
            )?;
            let project = workspace.require_resolved()?;
            let retained = record["files"]
                .as_array()
                .context("Missing saved build files")?
                .iter()
                .map(|file| {
                    Ok(locked_selector(
                        file["key"]["dependency"]
                            .as_str()
                            .context("Missing dependency key")?,
                        file["key"]["slot"].as_str().context("Missing file slot")?,
                    ))
                })
                .collect::<Result<BTreeSet<_>>>()?;
            locked_inputs(op, &project, &retained);
            op.diagnostic = Some("Exact unresolved build requirements are recomputed from the saved output recipes during continuation; listed lock inputs may not all be needed".into());
        }
        PendingKind::Sync => {
            if record["migration"]
                .as_bool()
                .context("Missing synchronization mode")?
            {
                op.diagnostic = Some("Runtime migration: continuation applies the retained candidate intent only after checking the original author documents and obtaining new approval".into());
            }
            let intent = DocumentCodec.decode_intent(
                record["intent"]
                    .as_str()
                    .context("Missing saved intent")?
                    .as_bytes(),
                "saved synchronization",
            )?;
            let project = DocumentCodec.decode_lock(
                record["lock"]
                    .as_str()
                    .context("Missing saved lock")?
                    .as_bytes(),
                &intent,
                "saved synchronization",
            )?;
            let retained = record["files"]
                .as_array()
                .context("Missing saved files")?
                .iter()
                .map(|file| {
                    Ok(format!(
                        "{}/{}",
                        file["dependency"].as_str().context("Missing dependency")?,
                        file["slot"].as_str().context("Missing slot")?
                    ))
                })
                .collect::<Result<BTreeSet<_>>>()?;
            locked_inputs(op, &project, &retained);
            op.requirements_complete = true;
        }
        PendingKind::Instance => {
            let release = super::release::DecodedRelease::decode(
                record["recipe"]["payload"]
                    .as_str()
                    .context("Missing release payload")?
                    .as_bytes(),
            )?;
            let manual: BTreeSet<String> = serde_json::from_value(record["manual"].clone())?;
            let retained: Vec<super::release::ReleaseFile> =
                serde_json::from_value(record["files"].clone())?;
            for file in &release.document().files {
                if !manual.contains(&file.key) && !retained.iter().any(|f| f.key == file.key) {
                    continue;
                }
                let expected = file.expected()?;
                let mut row = input(
                    file.key.clone(),
                    None,
                    file.destination.rsplit('/').next().map(str::to_owned),
                    Some(expected),
                    None,
                    !manual.contains(&file.key),
                    Some(manual.contains(&file.key)),
                );
                let selection = match &file.source {
                    super::release::ReleaseSource::Provider {
                        provider,
                        project,
                        selection,
                        ..
                    } => Some((provider, project, selection)),
                    super::release::ReleaseSource::Manual {
                        selection: Some(s), ..
                    } => Some((&s.provider, &s.project, &s.selection)),
                    _ => None,
                };
                if let Some((provider, project, selection)) = selection {
                    use empack_core::identity::{
                        CurseForgeProjectId, ModrinthProjectId, ProviderProjectId,
                    };
                    let project = match provider {
                        super::release::ReleaseProvider::Modrinth => {
                            ProviderProjectId::Modrinth(ModrinthProjectId::parse(project)?)
                        }
                        super::release::ReleaseProvider::CurseForge => {
                            ProviderProjectId::CurseForge(CurseForgeProjectId::parse(project)?)
                        }
                    };
                    row.pin = Some(ResolvedPin {
                        selection: project.parse_pin(selection)?,
                        project,
                    });
                    row.provider_selection = row.pin.as_ref().map(PendingProviderSelection::from);
                }
                op.inputs.push(row);
            }
            op.requirements_complete = true;
        }
        PendingKind::Import => {
            let files = record["files"]
                .as_object()
                .context("Missing retained import associations")?;
            for (key, content) in files {
                let hash: [u8; 32] = serde_json::from_value(content.clone())?;
                let mut row = input(key.clone(), None, None, None, None, true, None);
                row.digests
                    .push(("retained-sha256".into(), super::release::hex(&hash)));
                op.inputs.push(row);
            }
            op.diagnostic = Some("Saved imports retain the archive and supplied associations, not missing dependency metadata; normal continuation reparses source/provider facts. Use the original exact selectors with --file; unknown facts are not completion evidence".into());
        }
    }
    ensure!(
        op.inputs.len() <= 10_000,
        "Pending presentation exceeds input count limit"
    );
    Ok(())
}
/// Only existing owned control records are read. No network, caches, locks or directories are created.
pub fn inspect(state: &Path, target: &Path, cancel: &Cancellation) -> Result<PendingReport> {
    let mut report = PendingReport {
        schema: 1,
        root: target.into(),
        operations: Vec::new(),
    };
    if target.try_exists()?
        && let Some((id, bytes)) = super::api::inspect_pending_build(state, target, cancel)?
    {
        let mut op = empty(id, PendingKind::Build, target);
        if let Err(error) = details(&mut op, &bytes, state, cancel) {
            op.state = "unavailable".into();
            op.diagnostic = Some(error.to_string());
        }
        report.operations.push(op);
    }
    for (kind, public) in [
        (Kind::Import, PendingKind::Import),
        (Kind::Synchronization, PendingKind::Sync),
        (Kind::Instance, PendingKind::Instance),
    ] {
        let Some(selected) = store::observe(state, kind, target, cancel)? else {
            continue;
        };
        let mut op = empty(selected.identity(), public, target);
        let result = (|| {
            let (observed, bytes) = store::inspect_opaque(state, kind, target, 4 << 20, cancel)?
                .context("Pending record disappeared")?;
            ensure!(
                observed.identity() == op.id,
                "Pending record changed during inspection"
            );
            details(&mut op, &bytes, state, cancel)
        })();
        if let Err(error) = result {
            op.state = "unavailable".into();
            op.diagnostic = Some(error.to_string());
        }
        report.operations.push(op);
    }
    cancel.check()?;
    Ok(report)
}

#[cfg(test)]
mod tests;
