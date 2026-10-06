//! Source inclusion is explicit and independent of an exporter's reported inventory.
use anyhow::Result;
use empack_core::{
    model::ContentLayer,
    path::{InstallDestination, PortableRelPath},
};
/// One captured game file, before ownership is reconciled against exact locked placements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceEntry {
    pub path: PortableRelPath,
    pub destination: InstallDestination,
    pub layer: ContentLayer,
}
use ignore::gitignore::{Gitignore, GitignoreBuilder};

/// Packwiz-compatible pack-root ignore rules. Never consult host/global Git configuration.
pub struct SourceFilter {
    matcher: Gitignore,
}
impl SourceFilter {
    /// Rules are captured bytes, not a path that the matcher may reopen later.
    pub fn parse(document: &[u8]) -> Result<Self> {
        let text = std::str::from_utf8(document)?;
        let mut builder = GitignoreBuilder::new("");
        for line in [
            ".git/**",
            ".gitattributes",
            ".gitignore",
            ".DS_Store",
            "/*.zip",
            "*.mrpack",
            "packwiz.exe",
            "packwiz",
        ]
        .into_iter()
        .chain(text.lines())
        {
            builder.add_line(None, line)?;
        }
        Ok(Self {
            matcher: builder.build()?,
        })
    }
    /// Backend control documents are never game files. Metadata membership is checked separately.
    pub fn includes(&self, relative: &PortableRelPath, directory: bool) -> bool {
        self.includes_native(std::path::Path::new(relative.as_str()), directory)
    }
    fn includes_native(&self, relative: &std::path::Path, directory: bool) -> bool {
        if ["pack.toml", "index.toml", ".packwizignore"]
            .iter()
            .any(|name| relative == std::path::Path::new(name))
        {
            return false;
        }
        // Match each traversed ancestor before the leaf. Native names are allowed here only
        // for exclusion; included objects must still pass portable-path validation.
        if relative.ancestors().skip(1).any(|parent| {
            !parent.as_os_str().is_empty() && self.matcher.matched(parent, true).is_ignore()
        }) {
            return false;
        }
        !self.matcher.matched(relative, directory).is_ignore()
    }
}

/// Persisted traversal policy. Recovery repeats captured rules and seed outputs, never live policy.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CaptureFilter {
    rules: String,
    required: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    template_outputs: Option<Vec<String>>,
}
impl CaptureFilter {
    pub(super) fn new(rules: Vec<u8>, required: &[PortableRelPath]) -> Result<Self> {
        let value = Self {
            rules: String::from_utf8(rules)?,
            required: required
                .iter()
                .map(|path| path.as_str().to_owned())
                .collect(),
            template_outputs: None,
        };
        value.matcher()?;
        Ok(value)
    }
    /// Observe only entries capable of changing whether a default seed may be added.
    pub(super) fn template_seeds(paths: &[PortableRelPath]) -> Result<Self> {
        let mut outputs = std::collections::BTreeSet::new();
        for path in paths {
            let relative = path
                .as_str()
                .strip_prefix("templates/")
                .ok_or_else(|| anyhow::anyhow!("Seed outside templates"))?;
            let (layer, _) = relative
                .split_once('/')
                .ok_or_else(|| anyhow::anyhow!("Seed lacks template layer"))?;
            let (_, destination, _) = super::templates::template_address(relative)?
                .ok_or_else(|| anyhow::anyhow!("Unknown seed layer"))?;
            for layer in ["common", layer] {
                outputs.insert(format!("templates/{layer}/{}", destination.as_str()));
            }
        }
        let value = Self {
            rules: String::new(),
            required: vec![],
            template_outputs: Some(outputs.into_iter().collect()),
        };
        value.matcher()?;
        Ok(value)
    }
    pub(super) fn matcher(&self) -> Result<SourceFilter> {
        if let Some(outputs) = &self.template_outputs {
            anyhow::ensure!(
                self.rules.is_empty() && self.required.is_empty(),
                "Mixed snapshot policies"
            );
            for output in outputs {
                let path =
                    PortableRelPath::parse(output, empack_core::path::PathSyntax::ProjectContent)?;
                anyhow::ensure!(
                    path.as_str().starts_with("templates/"),
                    "Template observation outside namespace"
                );
            }
        }
        for path in &self.required {
            PortableRelPath::parse(path, empack_core::path::PathSyntax::ProjectContent)?;
        }
        SourceFilter::parse(self.rules.as_bytes())
    }
    pub(super) fn includes(
        &self,
        matcher: &SourceFilter,
        path: &PortableRelPath,
        directory: bool,
    ) -> bool {
        self.includes_native(matcher, std::path::Path::new(path.as_str()), directory)
    }
    pub(super) fn includes_native(
        &self,
        matcher: &SourceFilter,
        path: &std::path::Path,
        directory: bool,
    ) -> bool {
        if let Some(outputs) = &self.template_outputs {
            let parts: Option<Vec<_>> = path
                .components()
                .map(|part| match part {
                    std::path::Component::Normal(value) => value.to_str(),
                    _ => None,
                })
                .collect();
            return parts.is_some_and(|parts| {
                let path = parts.join("/");
                let output = if directory {
                    path.as_str()
                } else {
                    path.strip_suffix(".template").unwrap_or(&path)
                };
                outputs.iter().any(|seed| template_overlap(output, seed))
            });
        }
        let Ok(relative) = path.strip_prefix("pack") else {
            return true;
        };
        if relative.as_os_str().is_empty() {
            return true;
        }
        if ["pack.toml", "index.toml", ".packwizignore"]
            .iter()
            .any(|name| relative == std::path::Path::new(name))
        {
            return true;
        }
        // Explicit locked local/archive sources cannot disappear behind an ignore rule.
        if self.required.iter().any(|required| {
            let required = std::path::Path::new(required);
            required.starts_with(path) || path.starts_with(required)
        }) {
            return true;
        }
        matcher.includes_native(relative, directory)
    }
}

fn template_overlap(path: &str, seed: &str) -> bool {
    use caseless::Caseless;
    use unicode_normalization::UnicodeNormalization;
    // Component comparison includes portable case/normalization aliases before opening bytes.
    let mut left = path.split('/');
    let mut right = seed.split('/');
    loop {
        match (left.next(), right.next()) {
            (Some(a), Some(b)) => {
                let folded = |value: &str| {
                    value
                        .chars()
                        .nfd()
                        .default_case_fold()
                        .nfd()
                        .collect::<String>()
                };
                if folded(a) != folded(b) {
                    return false;
                }
                if a != b {
                    return true;
                }
            }
            _ => return true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use empack_core::path::PathSyntax;
    #[test]
    fn template_capture_selects_rendered_collisions_before_opening_user_bytes() {
        let seeds = [PortableRelPath::parse(
            "templates/client/nested/instance.cfg.template",
            PathSyntax::ProjectContent,
        )
        .unwrap()];
        let filter = CaptureFilter::template_seeds(&seeds).unwrap();
        let encoded = serde_json::to_vec(&filter).unwrap();
        let filter: CaptureFilter = serde_json::from_slice(&encoded).unwrap();
        let matcher = filter.matcher().unwrap();
        let native = std::path::Path::new("templates/client/nested").join("instance.cfg");
        assert!(filter.includes_native(&matcher, &native, false));
        for name in [
            "templates/client",
            "templates/common",
            "templates/client/nested",
            "templates/client/nested/instance.cfg",
            "templates/client/nested/INSTANCE.cfg.template",
            "templates/common/nested/instance.cfg",
            "templates/client/NESTED/other",
        ] {
            assert!(
                filter.includes_native(&matcher, std::path::Path::new(name), false),
                "{name}"
            );
        }
        for name in [
            "templates/client/unrelated.bin",
            "templates/common/unrelated:note",
            "templates/client/nested/other",
            "templates/server/nested/instance.cfg",
            "pack/instance.cfg",
        ] {
            assert!(
                !filter.includes_native(&matcher, std::path::Path::new(name), false),
                "{name}"
            );
        }
    }
    #[test]
    fn excluded_pack_contents_do_not_hide_the_control_root() {
        let policy = CaptureFilter::new(b"**\n".to_vec(), &[]).unwrap();
        let matcher = policy.matcher().unwrap();
        for name in [
            "pack",
            "pack/.packwizignore",
            "pack/pack.toml",
            "pack/index.toml",
        ] {
            assert!(
                policy.includes_native(&matcher, std::path::Path::new(name), name == "pack"),
                "{name}"
            );
        }
        assert!(!policy.includes_native(&matcher, std::path::Path::new("pack/other"), false));
    }
    #[test]
    fn source_rules_preserve_defaults_negation_and_ignored_parent_semantics() {
        let filter = SourceFilter::parse(b"!keep.zip\nconfig/private/\n!config/private/rescue.toml\n*.secret\n!config/keep.secret\n").unwrap();
        let includes = |name| {
            filter.includes(
                &PortableRelPath::parse(name, PathSyntax::ProjectContent).unwrap(),
                false,
            )
        };
        for path in [
            "archive.zip",
            "nested/pack.mrpack",
            ".git/config",
            ".DS_Store",
            "config/private/rescue.toml",
            "config/a.secret",
            "pack.toml",
            "index.toml",
            ".packwizignore",
        ] {
            assert!(!includes(path), "{path}");
        }
        for path in [
            "keep.zip",
            "resourcepacks/assets.zip",
            "config/keep.secret",
            "mods/entry.pw.toml",
            "config/new.toml",
        ] {
            assert!(includes(path), "{path}");
        }
        assert!(SourceFilter::parse(&[0xff]).is_err());
    }
}
