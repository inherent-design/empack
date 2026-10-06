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
        if matches!(
            relative.as_str(),
            "pack.toml" | "index.toml" | ".packwizignore"
        ) {
            return false;
        }
        // Match a traversal, not just the leaf: a negated child cannot resurrect a directory
        // that the walker already pruned. The matcher alone gives a leaf whitelist precedence.
        for (index, _) in relative.as_str().match_indices('/') {
            if self
                .matcher
                .matched(&relative.as_str()[..index], true)
                .is_ignore()
            {
                return false;
            }
        }
        !self
            .matcher
            .matched(relative.as_str(), directory)
            .is_ignore()
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
        let Some(relative) = path.as_str().strip_prefix("pack/") else {
            return true;
        };
        if matches!(relative, "pack.toml" | "index.toml" | ".packwizignore") {
            return true;
        }
        // Explicit locked local/archive sources cannot disappear behind an ignore rule.
        if self.required.iter().any(|required| {
            required == path.as_str()
                || required
                    .strip_prefix(path.as_str())
                    .is_some_and(|suffix| suffix.starts_with('/'))
                || path
                    .as_str()
                    .strip_prefix(required)
                    .is_some_and(|suffix| suffix.starts_with('/'))
        }) {
            return true;
        }
        let relative =
            PortableRelPath::parse(relative, empack_core::path::PathSyntax::ProjectContent)
                .expect("suffix of a validated portable path");
        matcher.includes(&relative, directory)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use empack_core::path::PathSyntax;
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
