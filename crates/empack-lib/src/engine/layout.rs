//! One mapping from managed logical destinations to portable project-relative paths.
use anyhow::{Result, bail, ensure};
use caseless::Caseless;
use empack_core::{
    files::{ManagedPath, ProjectScaffold},
    model::ContentLayer,
    path::{PathSyntax, PortableRelPath},
};
use std::collections::{BTreeMap, BTreeSet};
use unicode_normalization::UnicodeNormalization;

pub struct ProjectLayout;
impl ProjectLayout {
    pub fn path(target: &ManagedPath) -> Result<PortableRelPath> {
        let value = match target {
            ManagedPath::IntentDocument => "empack.yml".to_owned(),
            ManagedPath::LockDocument => "empack.lock".to_owned(),
            ManagedPath::BackendDocument(path) => format!("pack/{}", path.as_str()),
            ManagedPath::Content { layer, path } => format!(
                "{}/{}",
                match layer {
                    ContentLayer::Common => "pack",
                    ContentLayer::CommonOverride => "overrides/common",
                    ContentLayer::Client => "overrides/client",
                    ContentLayer::Server => "overrides/server",
                },
                path.as_str()
            ),
            ManagedPath::UserTemplate(path) => format!("templates/{}", path.as_str()),
            ManagedPath::Scaffold(scaffold) => match scaffold {
                ProjectScaffold::GitIgnore => ".gitignore",
                ProjectScaffold::ValidationWorkflow => ".github/workflows/validate.yml",
                ProjectScaffold::ReleaseWorkflow => ".github/workflows/release.yml",
            }
            .to_owned(),
            ManagedPath::Artifact(path) => format!("dist/{}", path.as_str()),
        };
        Ok(PortableRelPath::parse(&value, PathSyntax::ProjectContent)?)
    }

    /// Observation assigns backend documents their own role; unrelated root files are not managed.
    pub fn classify(path: &PortableRelPath) -> Result<ManagedPath> {
        let value = path.as_str();
        match value {
            "empack.yml" => return Ok(ManagedPath::IntentDocument),
            "empack.lock" => return Ok(ManagedPath::LockDocument),
            ".gitignore" => return Ok(ManagedPath::Scaffold(ProjectScaffold::GitIgnore)),
            ".github/workflows/validate.yml" => {
                return Ok(ManagedPath::Scaffold(ProjectScaffold::ValidationWorkflow));
            }
            ".github/workflows/release.yml" => {
                return Ok(ManagedPath::Scaffold(ProjectScaffold::ReleaseWorkflow));
            }
            _ => {}
        }
        let relative = |value: &str| {
            PortableRelPath::parse(value, PathSyntax::ProjectContent).map_err(anyhow::Error::from)
        };
        if let Some(value) = value.strip_prefix("pack/") {
            let path = relative(value)?;
            return Ok(
                if matches!(value, "pack.toml" | "index.toml") || value.ends_with(".pw.toml") {
                    ManagedPath::BackendDocument(path)
                } else {
                    ManagedPath::Content {
                        layer: ContentLayer::Common,
                        path,
                    }
                },
            );
        }
        for (prefix, layer) in [
            ("overrides/common/", ContentLayer::CommonOverride),
            ("overrides/client/", ContentLayer::Client),
            ("overrides/server/", ContentLayer::Server),
        ] {
            if let Some(value) = value.strip_prefix(prefix) {
                return Ok(ManagedPath::Content {
                    layer,
                    path: relative(value)?,
                });
            }
        }
        if let Some(value) = value.strip_prefix("templates/") {
            return Ok(ManagedPath::UserTemplate(relative(value)?));
        }
        if let Some(value) = value.strip_prefix("dist/") {
            return Ok(ManagedPath::Artifact(relative(value)?));
        }
        bail!("Path is outside managed project namespaces: {value}")
    }
}

pub(super) fn collision_key(path: &str) -> String {
    path.chars().nfd().default_case_fold().nfd().collect()
}

/// Conservative portable collision policy: canonical Unicode caseless matching.
/// Preserve original names; a collision is an error, never a rename or overwrite.
#[derive(Default)]
pub(super) struct CollisionIndex {
    spelling: BTreeMap<String, String>,
    files: BTreeSet<String>,
}
impl CollisionIndex {
    pub fn insert_file(&mut self, path: &PortableRelPath) -> Result<()> {
        self.insert(path, true)
    }
    pub fn insert_directory(&mut self, path: &PortableRelPath) -> Result<()> {
        self.insert(path, false)
    }
    fn insert(&mut self, path: &PortableRelPath, file: bool) -> Result<()> {
        let mut prefix = String::new();
        let mut parts = path.components().peekable();
        while let Some(part) = parts.next() {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            let key = collision_key(&prefix);
            ensure!(
                !self.files.contains(&key),
                "File destination collides with another file or ancestor: {prefix}"
            );
            if let Some(previous) = self.spelling.get(&key) {
                ensure!(
                    *previous == prefix,
                    "Portable filename collision: {previous} and {prefix}"
                );
                ensure!(
                    !file || parts.peek().is_some(),
                    "File destination replaces an ancestor directory: {prefix}"
                );
            }
            self.spelling.insert(key.clone(), prefix.clone());
            if file && parts.peek().is_none() {
                self.files.insert(key);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn path(input: &str) -> PortableRelPath {
        PortableRelPath::parse(input, PathSyntax::ProjectContent).unwrap()
    }
    #[test]
    fn collision_policy_preserves_spelling_and_rejects_aliases_on_every_host() {
        for (left, right) in [
            ("mods/A.jar", "mods/a.jar"),
            ("Straße/x", "STRASSE/y"),
            ("é", "e\u{301}"),
            ("dir", "dir/file"),
            ("dir/file", "dir"),
            ("same", "same"),
        ] {
            let mut index = CollisionIndex::default();
            index.insert_file(&path(left)).unwrap();
            assert!(index.insert_file(&path(right)).is_err(), "{left} / {right}");
        }
        let mut index = CollisionIndex::default();
        index.insert_file(&path("mods/a.jar")).unwrap();
        index.insert_file(&path("mods/b.jar")).unwrap();
    }
    #[test]
    fn layout_excludes_unrelated_root_files_and_roundtrips_managed_roles() {
        for input in [".git/config", "notes.txt", "overrides/unknown/a"] {
            assert!(ProjectLayout::classify(&path(input)).is_err());
        }
        for input in [
            "empack.yml",
            "empack.lock",
            ".gitignore",
            ".github/workflows/validate.yml",
            ".github/workflows/release.yml",
            "pack/pack.toml",
            "pack/mods/a.pw.toml",
            "pack/mods/a.jar",
            "overrides/common/config/a",
            "overrides/client/config/a",
            "overrides/server/config/a",
            "templates/server/start.sh",
            "dist/a.zip",
        ] {
            assert_eq!(
                ProjectLayout::path(&ProjectLayout::classify(&path(input)).unwrap()).unwrap(),
                path(input)
            );
        }
    }
}
