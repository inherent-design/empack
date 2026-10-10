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

/// Author-declared source rules. Never consult host/global Git configuration.
pub struct SourceFilter {
    matcher: Gitignore,
}
impl SourceFilter {
    /// Native author policy has no foreign control names or implicit archive exclusions.
    pub fn author(excludes: &[String]) -> Result<Self> {
        let mut builder = GitignoreBuilder::new("");
        for line in [".git/**", ".DS_Store"]
            .into_iter()
            .chain(excludes.iter().map(String::as_str))
        {
            anyhow::ensure!(
                !line.chars().any(char::is_control),
                "Source pattern contains a control character"
            );
            builder.add_line(None, line)?;
        }
        Ok(Self {
            matcher: builder.build()?,
        })
    }
    /// Apply the captured author policy to a source-layer relative path.
    pub fn includes(&self, relative: &PortableRelPath, directory: bool) -> bool {
        self.includes_native(std::path::Path::new(relative.as_str()), directory)
    }
    fn includes_native(&self, relative: &std::path::Path, directory: bool) -> bool {
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
    excludes: Vec<String>,
    required: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    template_outputs: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    managed_only: bool,
}
impl CaptureFilter {
    pub(super) fn author(excludes: &[String], required: &[PortableRelPath]) -> Result<Self> {
        let value = Self {
            excludes: excludes.to_vec(),
            required: required
                .iter()
                .map(|path| path.as_str().to_owned())
                .collect(),
            template_outputs: None,
            managed_only: false,
        };
        value.matcher()?;
        Ok(value)
    }
    /// Capture exact native mutation paths and portable aliases, without foreign discovery.
    pub(super) fn native_mutation(required: &[PortableRelPath]) -> Result<Self> {
        let mut value = Self::author(&[], required)?;
        value.managed_only = true;
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
            excludes: vec![],
            required: vec![],
            template_outputs: Some(outputs.into_iter().collect()),
            managed_only: false,
        };
        value.matcher()?;
        Ok(value)
    }
    pub(super) fn matcher(&self) -> Result<SourceFilter> {
        if let Some(outputs) = &self.template_outputs {
            anyhow::ensure!(
                self.excludes.is_empty() && self.required.is_empty() && !self.managed_only,
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
        SourceFilter::author(&self.excludes)
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
        if self.managed_only {
            let spelling: Option<Vec<_>> = path
                .components()
                .map(|part| match part {
                    std::path::Component::Normal(value) => value.to_str(),
                    _ => None,
                })
                .collect();
            return spelling.is_some_and(|parts| {
                let path = parts.join("/");
                matches!(path.as_str(), "empack.yml" | "empack.lock" | "pack")
                    || self
                        .required
                        .iter()
                        .any(|required| template_overlap(&path, required))
            });
        }
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
        if self.required.iter().any(|required| {
            let required = std::path::Path::new(required);
            required.starts_with(path) || path.starts_with(required)
        }) {
            return true;
        }
        for base in [
            "pack",
            "overrides/common",
            "overrides/client",
            "overrides/server",
        ] {
            if let Ok(relative) = path.strip_prefix(base) {
                return relative.as_os_str().is_empty()
                    || matcher.includes_native(relative, directory);
            }
        }
        true
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
                // Most captured paths share literal prefixes. Comparing those does not
                // require allocating and normalizing both components for every seed.
                if a == b {
                    continue;
                }
                if a.is_ascii() && b.is_ascii() {
                    return a.eq_ignore_ascii_case(b);
                }
                let folded = |value: &str| {
                    value
                        .chars()
                        .nfd()
                        .default_case_fold()
                        .nfd()
                        .collect::<String>()
                };
                return folded(a) == folded(b);
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
    fn capture_overlap_preserves_component_and_unicode_alias_boundaries() {
        for (left, right, expected) in [
            ("pack/config", "pack/config/a.toml", true),
            ("pack/config/a.toml", "pack/config", true),
            ("pack/config/a.toml", "pack/config/a.toml", true),
            ("pack/config/a.toml", "pack/config/b.toml", false),
            ("pack/config", "pack/configuration/a.toml", false),
            // An aliased ancestor is itself an observation obligation, even when
            // the remaining path differs from the selected descendant.
            ("pack/CONFIG/unrelated", "pack/config/a.toml", true),
            (
                "pack/caf\u{00e9}/unrelated",
                "pack/cafe\u{0301}/a.toml",
                true,
            ),
            ("pack/stra\u{00df}e/unrelated", "pack/STRASSE/a.toml", true),
            ("pack/\u{212a}/unrelated", "pack/k/a.toml", true),
            ("pack/caf\u{00e9}/a.toml", "pack/caf\u{00e9}/b.toml", false),
            ("pack/caf\u{00e9}/a.toml", "pack/cafe/a.toml", false),
        ] {
            assert_eq!(template_overlap(left, right), expected, "{left}, {right}");
            assert_eq!(template_overlap(right, left), expected, "{right}, {left}");
        }
    }
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
    fn excluded_sources_keep_roots_and_explicit_obligations() {
        let required =
            [
                PortableRelPath::parse("pack/mods/required.jar", PathSyntax::ProjectContent)
                    .unwrap(),
            ];
        let policy = CaptureFilter::author(&["**".into()], &required).unwrap();
        let encoded = serde_json::to_vec(&policy).unwrap();
        let policy: CaptureFilter = serde_json::from_slice(&encoded).unwrap();
        let matcher = policy.matcher().unwrap();
        for name in ["pack", "overrides/client", "pack/mods/required.jar"] {
            assert!(policy.includes_native(&matcher, std::path::Path::new(name), false));
        }
        for name in [
            "pack/other",
            "pack/.packwizignore",
            "pack/pack.toml",
            "overrides/client/private",
        ] {
            assert!(!policy.includes_native(&matcher, std::path::Path::new(name), false));
        }
    }
    #[test]
    fn author_rules_have_no_foreign_controls_or_archive_exclusions() {
        let filter = SourceFilter::author(&[
            "config/private/".into(),
            "!config/private/rescue.toml".into(),
            "*.secret".into(),
            "!config/keep.secret".into(),
        ])
        .unwrap();
        let includes = |name| {
            filter.includes(
                &PortableRelPath::parse(name, PathSyntax::ProjectContent).unwrap(),
                false,
            )
        };
        for name in [
            ".git/config",
            ".DS_Store",
            "config/private/rescue.toml",
            "config/a.secret",
        ] {
            assert!(!includes(name), "{name}");
        }
        for name in [
            "archive.zip",
            "nested/pack.mrpack",
            "pack.toml",
            "index.toml",
            ".packwizignore",
            "mods/entry.pw.toml",
            "config/keep.secret",
        ] {
            assert!(includes(name), "{name}");
        }
        assert!(SourceFilter::author(&["a\nb".into()]).is_err());
    }
}
