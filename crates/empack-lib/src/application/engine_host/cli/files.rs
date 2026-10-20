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
        .content_folder(kind)
        .context("This content kind requires a configured destination folder")?;
    let requirements = if matches!(kind, ContentKind::ResourcePack | ContentKind::ShaderPack) {
        Requirements {
            client: Requirement::Required,
            server: Requirement::Unsupported,
        }
    } else {
        required()
    };
    // A file already inside the checkout remains an authored source. External inputs
    // are copied into managed project storage so the manifest stays portable.
    let source = if let DirectFileSource::Local(path) = source {
        let (_, project) = project_path(session)?;
        if let Ok(relative) = path.strip_prefix(&project) {
            let source = portable_source(relative)?;
            DirectFileSource::TrackedLocal { path, source }
        } else {
            DirectFileSource::Local(path)
        }
    } else {
        source
    };
    let key = Path::new(&filename)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .context("Dependency needs a file stem")?;
    Ok(DirectFileInput {
        role: crate::engine::addition::FileInputRole::Primary,
        key: DependencyKey::parse(key)?,
        title: filename.clone(),
        source,
        evidence: FileEvidence::AcceptObserved,
        kind,
        kind_policy: FileKindPolicy::RequireRecognized,
        requirements: requirements.clone(),
        placements: NonEmpty::new(vec![Placement {
            destination: InstallDestination::parse(&format!(
                "{folder}/{}",
                if kind == ContentKind::World {
                    key
                } else {
                    &filename
                }
            ))?,
            layer: ContentLayer::Common,
            requirements,
        }])?,
    })
}

/// Native separators are syntax, while every component still obeys portable file rules.
fn portable_source(relative: &Path) -> Result<PortableRelPath> {
    let components = relative
        .components()
        .map(|component| match component {
            std::path::Component::Normal(name) => {
                name.to_str().context("Local source path is not UTF-8")
            }
            _ => anyhow::bail!("Local source path must contain only relative file components"),
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(PortableRelPath::parse(
        &components.join("/"),
        PathSyntax::ProjectContent,
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_source_components_preserve_portable_spelling_and_reject_traversal() {
        let source = PathBuf::from("vendor").join("mods").join("custom.jar");
        assert_eq!(
            portable_source(&source).unwrap().as_str(),
            "vendor/mods/custom.jar"
        );
        for path in [
            PathBuf::new(),
            PathBuf::from("../outside.jar"),
            PathBuf::from("vendor").join("..").join("outside.jar"),
            std::env::temp_dir().join("outside.jar"),
            PathBuf::from("vendor").join("CON.jar"),
        ] {
            assert!(portable_source(&path).is_err(), "{path:?}");
        }
        #[cfg(unix)]
        assert!(portable_source(Path::new(r"vendor/back\slash.jar")).is_err());
    }
}
