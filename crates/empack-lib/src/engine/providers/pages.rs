//! Browser destinations derived from verified provider selections, never from download locators.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadPage(String);
impl DownloadPage {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl ProviderCatalog {
    /// Resolve ownership of the exact selection before deriving its public download page.
    /// This is read-only network discovery; opening a browser needs separate host approval.
    pub async fn download_page(
        &self,
        scope: &mut WorkScope,
        pin: ResolvedPin,
        limits: CatalogLimits,
    ) -> Result<RetainedOutput<DownloadPage>> {
        self.resolve_exact(scope, pin, limits)
            .await?
            .map(|resolution| page(&resolution))
            .transpose()
    }
}
fn page(resolution: &ProviderResolution) -> Result<DownloadPage> {
    let project = &resolution.project;
    ensure!(project.id == resolution.pin.project, CatalogError::Identity);
    // These characters are safe both as a URL segment and as a Windows start argument.
    // Reject encoding, query syntax and shell syntax instead of passing it to an opener.
    ensure!(
        !project.slug.is_empty()
            && project.slug.len() <= 256
            && project.slug != "."
            && project.slug != ".."
            && project
                .slug
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_.+".contains(&byte)),
        CatalogError::InvalidRecord
    );
    let kind = project.kinds.as_slice()[0];
    let url = match (&project.id, &resolution.pin.selection) {
        (ProviderProjectId::Modrinth(_), PinSelector::ModrinthVersion(version)) => {
            let kind = match kind {
                ContentKind::Mod => "mod",
                ContentKind::ResourcePack => "resourcepack",
                ContentKind::ShaderPack => "shader",
                ContentKind::DataPack => "datapack",
                _ => return Err(CatalogError::UnsupportedKind.into()),
            };
            format!(
                "https://modrinth.com/{kind}/{}/version/{version}",
                project.slug
            )
        }
        (ProviderProjectId::CurseForge(_), PinSelector::CurseForgeFile(file)) => {
            let kind = match kind {
                ContentKind::Mod => "mc-mods",
                ContentKind::ResourcePack => "texture-packs",
                ContentKind::ShaderPack => "shaders",
                ContentKind::DataPack => "data-packs",
                ContentKind::World => "worlds",
                _ => return Err(CatalogError::UnsupportedKind.into()),
            };
            format!(
                "https://www.curseforge.com/minecraft/{kind}/{}/files/{file}",
                project.slug
            )
        }
        _ => return Err(CatalogError::Identity.into()),
    };
    Ok(DownloadPage(url))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::providers::tests::{
        bytes, cf_file, cf_project, mr_project, mr_version, pin,
    };
    use empack_core::model::ProviderKind;
    #[test]
    fn browser_pages_keep_provider_kind_and_exact_selection() {
        let mut mr = super::super::modrinth::selection(
            super::super::modrinth::project(&bytes(&mr_project())).unwrap(),
            &pin(ProviderKind::Modrinth),
            &bytes(&mr_version()),
        )
        .unwrap();
        assert_eq!(
            page(&mr).unwrap().as_str(),
            "https://modrinth.com/mod/sodium/version/abcdefgh"
        );
        let cf = super::super::curseforge::selection(
            super::super::curseforge::project(&bytes(&cf_project()), false).unwrap(),
            &pin(ProviderKind::CurseForge),
            &bytes(&cf_file()),
        )
        .unwrap();
        assert_eq!(
            page(&cf).unwrap().as_str(),
            "https://www.curseforge.com/minecraft/mc-mods/sodium/files/456"
        );
        for slug in [
            "../other",
            "x%20y",
            "x&start",
            "x?token=secret",
            "x\nother",
            ".",
            "",
        ] {
            mr.project.slug = slug.into();
            assert!(page(&mr).is_err(), "{slug:?}");
        }
    }
}
