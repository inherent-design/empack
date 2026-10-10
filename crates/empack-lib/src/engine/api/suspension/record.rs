//! Strict saved request data. No plan ID, native source path or execution grant is encoded.
use super::*;
use crate::engine::{
    packwiz::InstallerInteraction,
    templates::{TemplateLimits, TemplateMode, TemplateOptions},
};
use empack_core::{
    digest::{ContentId, ExpectedDigest},
    model::{DependencyKey, FileSlot},
    path::PathSyntax,
};
use serde::{Deserialize, Serialize};

pub(super) const MAX_RECORD: u64 = 4 << 20;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    pub schema: u32,
    pub fingerprint: [u8; 32],
    pub documents: [Option<[u8; 32]>; 2],
    pub recipe: Recipe,
    pub files: Vec<SavedFile>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SavedFile {
    pub key: Key,
    pub content: String,
    pub readonly: bool,
    pub executable: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(super) enum Key {
    Locked { dependency: String, slot: String },
    Observed { metadata: String },
}
impl From<&AcquisitionKey> for Key {
    fn from(key: &AcquisitionKey) -> Self {
        match key {
            AcquisitionKey::Locked(key) => Self::Locked {
                dependency: key.dependency.as_str().into(),
                slot: key.slot.as_str().into(),
            },
            AcquisitionKey::Observed(path) => Self::Observed {
                metadata: path.as_str().into(),
            },
        }
    }
}
impl Key {
    pub fn parse(&self) -> Result<AcquisitionKey> {
        Ok(match self {
            Self::Locked { dependency, slot } => {
                AcquisitionKey::Locked(crate::engine::mrpack::LockedFileKey {
                    dependency: DependencyKey::parse(dependency)?,
                    slot: FileSlot::parse(slot)?,
                })
            }
            Self::Observed { metadata } => AcquisitionKey::Observed(PortableRelPath::parse(
                metadata,
                PathSyntax::ProjectContent,
            )?),
        })
    }
}
impl SavedFile {
    pub fn id(&self) -> Result<ContentId> {
        let ExpectedDigest::Sha256(hash) = ExpectedDigest::parse("sha256", &self.content)? else {
            unreachable!()
        };
        Ok(ContentId::from_sha256(hash))
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Recipe {
    clean: bool,
    outputs: Vec<(String, String)>,
    archive: String,
    optional: Optional,
    allow_optional_metadata_loss: bool,
    templates: Templates,
    strong_source_required: bool,
    interactive_installer: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "kebab-case", deny_unknown_fields)]
enum Optional {
    Preserve,
    Resolve {
        choices: BTreeMap<String, bool>,
        use_defaults: bool,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Templates {
    modes: BTreeMap<String, String>,
    values: BTreeMap<String, String>,
    input_bytes: u64,
    output_bytes: u64,
    total_bytes: u64,
    entries: usize,
}
impl From<&BuildRequest> for Recipe {
    fn from(request: &BuildRequest) -> Self {
        Self {
            clean: request.clean,
            outputs: request
                .outputs
                .as_slice()
                .iter()
                .map(|output| {
                    (
                        match output.target {
                            BuildTarget::Mrpack => "mrpack",
                            BuildTarget::CurseForge => "curseforge",
                            BuildTarget::Client => "client",
                            BuildTarget::Server => "server",
                            BuildTarget::ClientFull => "client-full",
                            BuildTarget::ServerFull => "server-full",
                        }
                        .into(),
                        output.artifact.as_str().into(),
                    )
                })
                .collect(),
            archive: match request.archive {
                DistributionArchive::Zip => "zip",
                DistributionArchive::TarGz => "tar-gz",
                DistributionArchive::SevenZip => "7z",
            }
            .into(),
            optional: match &request.optional {
                OptionalPolicy::Preserve => Optional::Preserve,
                OptionalPolicy::Resolve {
                    choices,
                    use_defaults,
                } => Optional::Resolve {
                    choices: choices.clone(),
                    use_defaults: *use_defaults,
                },
            },
            allow_optional_metadata_loss: request.mrpack_optional
                == OptionalConversion::AcknowledgedMetadataLoss,
            templates: Templates {
                modes: request
                    .templates
                    .modes
                    .iter()
                    .map(|(path, mode)| {
                        (
                            path.as_str().into(),
                            match mode {
                                TemplateMode::Copy => "copy",
                                TemplateMode::Handlebars => "handlebars",
                                TemplateMode::TextOrBinary => "text-or-binary",
                            }
                            .into(),
                        )
                    })
                    .collect(),
                values: request.templates.values.clone(),
                input_bytes: request.templates.limits.input_bytes,
                output_bytes: request.templates.limits.output_bytes,
                total_bytes: request.templates.limits.total_bytes,
                entries: request.templates.limits.entries,
            },
            strong_source_required: request.evidence == SourceEvidencePolicy::StrongSourceRequired,
            interactive_installer: request.interaction == InstallerInteraction::Interactive,
        }
    }
}
impl Recipe {
    pub fn parse(&self) -> Result<BuildRequest> {
        Ok(BuildRequest {
            clean: self.clean,
            outputs: NonEmpty::new(
                self.outputs
                    .iter()
                    .map(|(target, path)| {
                        Ok(BuildOutput {
                            target: match target.as_str() {
                                "mrpack" => BuildTarget::Mrpack,
                                "curseforge" => BuildTarget::CurseForge,
                                "client" => BuildTarget::Client,
                                "server" => BuildTarget::Server,
                                "client-full" => BuildTarget::ClientFull,
                                "server-full" => BuildTarget::ServerFull,
                                _ => anyhow::bail!("Unsupported saved build target"),
                            },
                            artifact: PortableRelPath::parse(path, PathSyntax::ArtifactName)?,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?,
            )?,
            archive: match self.archive.as_str() {
                "zip" => DistributionArchive::Zip,
                "tar-gz" => DistributionArchive::TarGz,
                "7z" => DistributionArchive::SevenZip,
                _ => anyhow::bail!("Unsupported saved archive format"),
            },
            optional: match &self.optional {
                Optional::Preserve => OptionalPolicy::Preserve,
                Optional::Resolve {
                    choices,
                    use_defaults,
                } => OptionalPolicy::Resolve {
                    choices: choices.clone(),
                    use_defaults: *use_defaults,
                },
            },
            mrpack_optional: if self.allow_optional_metadata_loss {
                OptionalConversion::AcknowledgedMetadataLoss
            } else {
                OptionalConversion::RejectMetadataLoss
            },
            templates: TemplateOptions {
                modes: self
                    .templates
                    .modes
                    .iter()
                    .map(|(path, mode)| {
                        Ok((
                            PortableRelPath::parse(path, PathSyntax::ProjectContent)?,
                            match mode.as_str() {
                                "copy" => TemplateMode::Copy,
                                "handlebars" => TemplateMode::Handlebars,
                                "text-or-binary" => TemplateMode::TextOrBinary,
                                _ => anyhow::bail!("Unsupported saved template mode"),
                            },
                        ))
                    })
                    .collect::<Result<_>>()?,
                values: self.templates.values.clone(),
                limits: TemplateLimits {
                    input_bytes: self.templates.input_bytes,
                    output_bytes: self.templates.output_bytes,
                    total_bytes: self.templates.total_bytes,
                    entries: self.templates.entries,
                },
            },
            evidence: if self.strong_source_required {
                SourceEvidencePolicy::StrongSourceRequired
            } else {
                SourceEvidencePolicy::Compatibility
            },
            interaction: if self.interactive_installer {
                InstallerInteraction::Interactive
            } else {
                InstallerInteraction::Headless
            },
        })
    }
}

/// Conservative encoded-size estimate without cloning request strings or file metadata.
pub(super) fn estimated_bytes(request: &BuildRequest, content: &BuildAcquisitions) -> Result<u64> {
    fn text(value: &str) -> u64 {
        value
            .as_bytes()
            .iter()
            .map(|byte| match byte {
                b'"' | b'\\' => 2,
                0..=31 => 6,
                _ => 1,
            })
            .sum::<u64>()
    }
    let mut total = 4096u64;
    let mut add = |bytes: u64| -> Result<()> {
        total = total
            .checked_add(bytes)
            .context("Pending recipe size overflow")?;
        Ok(())
    };
    for output in request.outputs.as_slice() {
        add(text(output.artifact.as_str()) + 64)?;
    }
    for path in request.templates.modes.keys() {
        add(text(path.as_str()) + 64)?;
    }
    for (key, value) in &request.templates.values {
        add(text(key) + text(value) + 32)?;
    }
    if let OptionalPolicy::Resolve { choices, .. } = &request.optional {
        for key in choices.keys() {
            add(text(key) + 32)?;
        }
    }
    for key in content.locked.keys() {
        add(text(key.dependency.as_str()) + text(key.slot.as_str()) + 256)?;
    }
    for path in content.observed.keys() {
        add(text(path.as_str()) + 256)?;
    }
    Ok(total)
}
