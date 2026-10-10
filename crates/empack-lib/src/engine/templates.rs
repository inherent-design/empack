//! Output-language helpers shared by captured builds and project scaffolding.
mod encoding;

pub(crate) fn register_helpers(handlebars: &mut handlebars::Handlebars<'_>) {
    handlebars::handlebars_helper!(shell_quote: |value: str| {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    });
    handlebars::handlebars_helper!(ini_quote: |value: str| encoding::ini_value(value));
    handlebars::handlebars_helper!(properties_value: |value: str| encoding::properties_value(value));
    handlebars.register_helper("shell_quote", Box::new(shell_quote));
    handlebars.register_helper("ini_quote", Box::new(ini_quote));
    handlebars.register_helper("properties_value", Box::new(properties_value));
}

use super::{
    content::{AcquiredContent, InitialObservation, SourceEvidencePolicy, verify_stream},
    layout::CollisionIndex,
    project::WorkspaceSnapshot,
    snapshot::Observation,
};
use crate::application::process_runtime::Cancellation;
use anyhow::{Context, Result, ensure};
use empack_core::{
    files::FilePermissions,
    model::{ContentLayer, ExpectedContent, LoaderKind},
    path::{PathSyntax, PortableRelPath},
    projection::BuildTarget,
};
use std::{
    collections::BTreeMap,
    io::{self, Read, Write},
};

/// Rendering is explicit; the output filename does not select a quoting language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateMode {
    /// Exact bytes, including UTF-8 containing template-like syntax.
    Copy,
    /// Require UTF-8 and expand expressions with explicit output-language helpers.
    Handlebars,
    /// Preserve the project convention: expand UTF-8, copy non-text bytes unchanged.
    TextOrBinary,
}
#[derive(Debug, Clone, Copy)]
pub struct TemplateLimits {
    pub input_bytes: u64,
    pub output_bytes: u64,
    pub total_bytes: u64,
    pub entries: usize,
}
impl Default for TemplateLimits {
    fn default() -> Self {
        Self {
            input_bytes: 16 << 20,
            output_bytes: 32 << 20,
            total_bytes: 128 << 20,
            entries: 10_000,
        }
    }
}
#[derive(Clone, Default)]
pub struct TemplateOptions {
    /// Exact captured input paths; unknown entries are errors, not ignored configuration.
    pub modes: BTreeMap<PortableRelPath, TemplateMode>,
    /// Additional or deliberately overridden values. Credentials should not be build metadata.
    pub values: BTreeMap<String, String>,
    pub limits: TemplateLimits,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateInput {
    pub source: PortableRelPath,
    pub destination: PortableRelPath,
    pub layer: ContentLayer,
    pub mode: TemplateMode,
}
pub struct RenderedTemplate {
    pub input: TemplateInput,
    /// Explicit common input replaced by this side's template, if any.
    pub replaces: Option<PortableRelPath>,
    pub content: AcquiredContent,
    pub permissions: FilePermissions,
}
/// An all-or-error preparation result. It carries no project or distribution writer.
#[derive(Default)]
pub struct RenderedTemplates {
    files: BTreeMap<PortableRelPath, RenderedTemplate>,
}
impl RenderedTemplates {
    pub fn files(&self) -> &BTreeMap<PortableRelPath, RenderedTemplate> {
        &self.files
    }
}

/// One source-to-output interpretation, shared by build rendering and missing-template seeds.
pub(super) fn template_address(
    relative: &str,
) -> Result<Option<(ContentLayer, PortableRelPath, TemplateMode)>> {
    let Some((layer, name)) = relative.split_once('/') else {
        return Ok(None);
    };
    let layer = match layer {
        "common" => ContentLayer::Common,
        "client" => ContentLayer::Client,
        "server" => ContentLayer::Server,
        _ => return Ok(None),
    };
    let (name, mode) = match name.strip_suffix(".template") {
        Some(name) => (name, TemplateMode::Handlebars),
        None => (name, TemplateMode::TextOrBinary),
    };
    Ok(Some((
        layer,
        PortableRelPath::parse(name, PathSyntax::ArchiveMember)?,
        mode,
    )))
}

/// Render a standalone target from its captured common and side inputs. Nested paths,
/// user scripts and binary content remain inputs, never rewritten project files.
/// Call from an admitted worker: parsing and local file verification are synchronous.
pub fn prepare_templates(
    workspace: &WorkspaceSnapshot,
    target: BuildTarget,
    options: &TemplateOptions,
    cancel: &Cancellation,
) -> Result<RenderedTemplates> {
    let side = match target {
        BuildTarget::Client | BuildTarget::ClientFull => ContentLayer::Client,
        BuildTarget::Server | BuildTarget::ServerFull => ContentLayer::Server,
        BuildTarget::Mrpack | BuildTarget::CurseForge => {
            anyhow::bail!("Reference archives have no standalone template projection")
        }
    };
    cancel.check()?;
    let project = workspace.require_resolved()?;
    let root = PortableRelPath::parse("templates", PathSyntax::ProjectContent)?;
    ensure!(
        matches!(
            workspace.observations().entries().get(&root),
            Some(Observation::Directory { .. } | Observation::Absent)
        ),
        "Template root was not captured"
    );
    let mut layers = [BTreeMap::new(), BTreeMap::new()];
    let mut all_inputs = std::collections::BTreeSet::new();
    for (path, observed) in workspace.observations().entries() {
        cancel.check()?;
        if !matches!(observed, Observation::File(_)) {
            continue;
        }
        let Some(relative) = path.as_str().strip_prefix("templates/") else {
            continue;
        };
        let Some((layer, destination, mode)) = template_address(relative)? else {
            continue;
        };
        all_inputs.insert(path.clone());
        if layer != ContentLayer::Common && layer != side {
            continue;
        }
        let input = TemplateInput {
            source: path.clone(),
            destination: destination.clone(),
            layer,
            mode: options.modes.get(path).copied().unwrap_or(mode),
        };
        let index = usize::from(layer != ContentLayer::Common);
        ensure!(
            layers[index].insert(destination, input).is_none(),
            "Two templates produce the same destination in one layer"
        );
    }
    ensure!(
        options.modes.keys().all(|path| all_inputs.contains(path)),
        "Template mode names an uncaptured input"
    );
    for layer in &layers {
        let mut collisions = CollisionIndex::default();
        for path in layer.keys() {
            collisions.insert_file(path)?;
        }
    }
    let [common, side] = layers;
    let mut selected: BTreeMap<_, _> = common
        .into_iter()
        .map(|(key, input)| (key, (input, None)))
        .collect();
    for (destination, input) in side {
        let previous = selected.remove(&destination).map(|(input, _)| input.source);
        selected.insert(destination, (input, previous));
    }
    ensure!(
        selected.len() <= options.limits.entries,
        "Template count exceeds limit"
    );
    let mut collisions = CollisionIndex::default();
    for path in selected.keys() {
        collisions.insert_file(path)?;
    }
    let mut values = template_values(&project, target);
    values.extend(options.values.clone());
    let mut renderer = handlebars::Handlebars::new();
    renderer.set_strict_mode(true);
    renderer.register_escape_fn(handlebars::no_escape);
    register_helpers(&mut renderer);
    let mut total = 0u64;
    let mut files = BTreeMap::new();
    for (destination, (input, replaces)) in selected {
        cancel.check()?;
        let Some(Observation::File(observed)) =
            workspace.observations().entries().get(&input.source)
        else {
            anyhow::bail!("Template is not a captured regular file");
        };
        ensure!(
            observed.bytes <= options.limits.input_bytes,
            "Template input exceeds limit"
        );
        let remaining = options
            .limits
            .total_bytes
            .checked_sub(total)
            .context("Template output total exceeds limit")?;
        let maximum = options.limits.output_bytes.min(remaining);
        let (source, permissions) = workspace.acquire_file(
            &input.source,
            None,
            SourceEvidencePolicy::Compatibility,
            cancel,
        )?;
        let content = if input.mode == TemplateMode::Copy {
            ensure!(
                source.lease().len() <= maximum,
                "Template copy exceeds output limit"
            );
            source
        } else {
            let mut bytes = Vec::new();
            source.lease().open().read_to_end(&mut bytes)?;
            match std::str::from_utf8(&bytes) {
                Ok(text) => {
                    let mut output = BoundedOutput {
                        bytes: Vec::new(),
                        maximum,
                        cancel,
                    };
                    renderer
                        .render_template_to_write(text, &values, &mut output)
                        .with_context(|| {
                            format!("Template rendering failed: {}", input.source.as_str())
                        })?;
                    verify_stream(
                        &mut output.bytes.as_slice(),
                        &ExpectedContent {
                            digests: None,
                            size: Some(output.bytes.len() as u64),
                            accepted_observation: None,
                        },
                        maximum,
                        SourceEvidencePolicy::Compatibility,
                        InitialObservation::Accepted,
                        cancel,
                    )?
                }
                Err(_) if input.mode == TemplateMode::TextOrBinary => {
                    ensure!(
                        source.lease().len() <= maximum,
                        "Binary template exceeds output limit"
                    );
                    source
                }
                Err(_) => {
                    anyhow::bail!("Explicit template is not UTF-8: {}", input.source.as_str())
                }
            }
        };
        total = total
            .checked_add(content.lease().len())
            .context("Template output length overflow")?;
        files.insert(
            destination,
            RenderedTemplate {
                input,
                replaces,
                content,
                permissions,
            },
        );
    }
    cancel.check()?;
    Ok(RenderedTemplates { files })
}
/// Metadata has one interpretation for user templates and embedded defaults.
fn template_values(
    project: &empack_core::model::ResolvedProject,
    target: BuildTarget,
) -> BTreeMap<String, String> {
    let metadata = &project.intent().metadata;
    let runtime = &project.lock().runtime;
    let loader = match runtime.loader {
        LoaderKind::Vanilla => "vanilla",
        LoaderKind::Fabric => "fabric",
        LoaderKind::Quilt => "quilt",
        LoaderKind::Forge => "forge",
        LoaderKind::NeoForge => "neoforge",
    };
    let safe_name: String = metadata
        .name
        .to_lowercase()
        .chars()
        .map(|ch| if ch.is_alphanumeric() { ch } else { '-' })
        .collect();
    let values: BTreeMap<String, String> = [
        (
            "BOOTSTRAP",
            if matches!(target, BuildTarget::Client | BuildTarget::Server) {
                "true"
            } else {
                ""
            }
            .into(),
        ),
        ("BOOTSTRAP_COMMAND", "\"$INST_JAVA\" -jar packwiz-installer-bootstrap.jar --bootstrap-no-update --bootstrap-main-jar packwiz-installer.jar -s client pack/pack.toml".into()),
        ("NAME", metadata.name.clone()),
        ("VERSION", metadata.version.clone()),
        ("AUTHOR", metadata.author.clone().unwrap_or_default()),
        (
            "DESCRIPTION",
            metadata.description.clone().unwrap_or_default(),
        ),
        ("SAFE_NAME", safe_name),
        ("MC_VERSION", runtime.minecraft.as_str().to_owned()),
        ("MODLOADER_NAME", loader.to_owned()),
        (
            "MODLOADER_VERSION",
            runtime
                .loader_version
                .as_ref()
                .map(|value| value.as_str().to_owned())
                .unwrap_or_default(),
        ),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value))
    .collect();
    values
}

pub(super) fn render_default(
    project: &empack_core::model::ResolvedProject,
    target: BuildTarget,
    source: &str,
    overrides: impl IntoIterator<Item = (String, String)>,
    maximum: u64,
    cancel: &Cancellation,
) -> Result<Vec<u8>> {
    cancel.check()?;
    let mut values = template_values(project, target);
    values.extend(overrides);
    let mut renderer = handlebars::Handlebars::new();
    renderer.set_strict_mode(true);
    renderer.register_escape_fn(handlebars::no_escape);
    register_helpers(&mut renderer);
    let mut output = BoundedOutput {
        bytes: Vec::new(),
        maximum,
        cancel,
    };
    renderer.render_template_to_write(source, &values, &mut output)?;
    cancel.check()?;
    Ok(output.bytes)
}

struct BoundedOutput<'a> {
    bytes: Vec<u8>,
    maximum: u64,
    cancel: &'a Cancellation,
}
impl Write for BoundedOutput<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.cancel.check().map_err(io::Error::other)?;
        let next = (self.bytes.len() as u64)
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| io::Error::other("Template output length overflow"))?;
        if next > self.maximum {
            return Err(io::Error::other("Template output exceeds limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
