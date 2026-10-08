use super::*;
use crate::engine::addition::{DirectFileInput, DirectFileSource, FileEvidence, FileKindPolicy};
use empack_core::{
    model::{ContentLayer, DependencyKey, Placement},
    path::{InstallDestination, PathSyntax, PortableRelPath},
};

/// Existing paths and explicit path syntax never fall through to provider search.
pub(super) fn classify(
    invocation: &Path,
    value: &str,
    provider_url: bool,
) -> Result<Option<DirectFileSource>> {
    if provider_url {
        return Ok(None);
    }
    if value.contains("://") {
        crate::engine::documents::validate_download_url(value)?;
        return Ok(Some(DirectFileSource::Download {
            origins: NonEmpty::new(vec![value.into()])?,
            alternatives: NonEmpty::new(vec![value.into()])?,
        }));
    }
    let path = absolute(invocation, Path::new(value));
    let explicit = Path::new(value).is_absolute()
        || value.contains('/')
        || value.contains('\\')
        || matches!(
            Path::new(value)
                .extension()
                .and_then(|value| value.to_str()),
            Some("jar" | "zip" | "mrpack")
        );
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) => {
            ensure!(
                metadata.file_type().is_file(),
                "Direct dependency input must be a regular file"
            );
            Ok(Some(DirectFileSource::Local(path)))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !explicit => Ok(None),
        Err(error) => Err(error).context("Cannot read direct dependency input"),
    }
}
pub(super) fn input(
    session: &dyn Session,
    current: &ResolvedProject,
    value: &str,
    source: DirectFileSource,
    selected: Option<&CliProjectType>,
) -> Result<DirectFileInput> {
    let filename = match &source {
        DirectFileSource::Local(path) => path
            .file_name()
            .and_then(|name| name.to_str())
            .context("Dependency filename is not UTF-8")?
            .to_owned(),
        _ => {
            let url = reqwest::Url::parse(value)?;
            let name = url
                .path_segments()
                .and_then(|mut parts| parts.next_back())
                .context("Download URL needs a filename")?;
            percent_encoding::percent_decode_str(name)
                .decode_utf8()?
                .into_owned()
        }
    };
    // URL encoding must not smuggle a different destination or platform-specific component.
    PortableRelPath::parse(&filename, PathSyntax::ArtifactName)?;
    let kind = match selected {
        Some(selected) => kind(selected),
        None if filename.to_ascii_lowercase().ends_with(".jar") => ContentKind::Mod,
        None => {
            ensure!(
                !session.config().app_config().yes && session.interactive().can_choose(),
                crate::application::cli::CommandInputRequired(
                    "Direct archive kind is ambiguous; supply --type"
                )
            );
            let kinds = [
                ContentKind::Mod,
                ContentKind::ResourcePack,
                ContentKind::ShaderPack,
                ContentKind::DataPack,
                ContentKind::World,
            ];
            let labels = ["Mod", "Resource pack", "Shader pack", "Data pack", "World"];
            let index = session
                .interactive()
                .select("Direct archive content type", &labels)?;
            *kinds
                .get(index)
                .context("Content type selection is out of range")?
        }
    };
    let folder = current
        .intent()
        .layout
        .get(&kind)
        .map(|folder| folder.as_str())
        .or(match kind {
            ContentKind::Mod => Some("mods"),
            ContentKind::ResourcePack => Some("resourcepacks"),
            ContentKind::ShaderPack => Some("shaderpacks"),
            _ => None,
        })
        .context("This content kind requires a configured destination folder")?;
    let requirements = if matches!(kind, ContentKind::ResourcePack | ContentKind::ShaderPack) {
        Requirements {
            client: Requirement::Required,
            server: Requirement::Unsupported,
        }
    } else {
        required()
    };
    let key = Path::new(&filename)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .context("Dependency needs a file stem")?;
    Ok(DirectFileInput {
        key: DependencyKey::parse(key)?,
        title: filename.clone(),
        source,
        evidence: FileEvidence::AcceptObserved,
        kind,
        kind_policy: FileKindPolicy::RequireRecognized,
        requirements: requirements.clone(),
        placements: NonEmpty::new(vec![Placement {
            destination: InstallDestination::parse(&format!("{folder}/{filename}"))?,
            layer: ContentLayer::Common,
            requirements,
        }])?,
    })
}
