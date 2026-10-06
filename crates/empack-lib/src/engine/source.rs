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

/// Persisted build traversal policy. Recovery repeats these captured rules, never live rules.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PackCaptureFilter {
    rules: String,
    required: Vec<String>,
}
impl PackCaptureFilter {
    pub(super) fn new(rules: Vec<u8>, required: &[PortableRelPath]) -> Result<Self> {
        let value = Self {
            rules: String::from_utf8(rules)?,
            required: required
                .iter()
                .map(|path| path.as_str().to_owned())
                .collect(),
        };
        value.matcher()?;
        Ok(value)
    }
    pub(super) fn matcher(&self) -> Result<SourceFilter> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use empack_core::path::PathSyntax;
    #[test]
    fn excluded_pack_contents_do_not_hide_the_control_root() {
        let policy = PackCaptureFilter::new(b"**\n".to_vec(), &[]).unwrap();
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
