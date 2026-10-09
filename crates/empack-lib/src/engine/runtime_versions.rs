//! Pure interpretation of official Forge version coordinates.
use anyhow::Result;
use semver::Version;

const LEGACY_FORGE_1710_SUFFIX_START: &str = "10.13.2.1300";

/// Parse a version string into a semver::Version, normalizing 2-component
/// strings like "1.20" to "1.20.0" since Minecraft uses both forms.
/// Legacy Forge 4+ component numeric versions are mapped into prerelease
/// identifiers so semver can still compare them numerically.
pub(crate) fn parse_version(s: &str) -> Option<Version> {
    let normalized = if s.matches('.').count() == 1 {
        format!("{s}.0")
    } else {
        s.to_string()
    };

    Version::parse(&normalized).ok().or_else(|| {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() > 3
            && parts
                .iter()
                .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
        {
            Version::parse(&format!(
                "{}.{}.{}-{}",
                parts[0],
                parts[1],
                parts[2],
                parts[3..].join(".")
            ))
            .ok()
        } else {
            None
        }
    })
}

/// Sort version strings in descending order (newest first) using semver.
/// Unparseable versions sort to the end.
pub(crate) fn sort_versions_desc(versions: &mut [String]) {
    versions.sort_by(|a, b| match (parse_version(a), parse_version(b)) {
        (Some(va), Some(vb)) => vb.cmp(&va),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.cmp(b),
    });
}

/// Canonicalize Forge loader versions for legacy 1.7.10 metadata.
///
/// Forge maven metadata switches from raw versions like `10.13.2.1291`
/// to suffixed versions like `10.13.2.1300-1.7.10`. Empack stores the raw
/// Forge loader version internally, so late legacy values are normalized
/// back to that form here.
pub(crate) fn canonicalize_forge_loader_version(mc_version: &str, loader_version: &str) -> String {
    if mc_version == "1.7.10" {
        loader_version
            .strip_suffix("-1.7.10")
            .unwrap_or(loader_version)
            .to_string()
    } else {
        loader_version.to_string()
    }
}

/// Returns true when a Forge 1.7.10 loader version requires the repeated-MC
/// legacy coordinate form introduced at 10.13.2.1300.
pub(crate) fn uses_legacy_forge_coordinate(mc_version: &str, loader_version: &str) -> bool {
    if mc_version != "1.7.10" {
        return false;
    }

    let Some(version) = parse_version(&canonicalize_forge_loader_version(
        mc_version,
        loader_version,
    )) else {
        return false;
    };

    let threshold =
        parse_version(LEGACY_FORGE_1710_SUFFIX_START).expect("legacy Forge boundary should parse");
    version >= threshold
}

/// Filter Forge versions by Minecraft version from maven-metadata.xml
///
/// Forge maven-metadata.xml structure:
/// - Version format: "{mc_version}-{forge_version}"
/// - Examples: "1.21.11-61.0.3", "1.20.1-47.4.13", "1.7.10-10.13.4.1614"
///
/// Strategy:
/// - Extract all versions matching "{mc_version}-" prefix
/// - Return sorted newest first (semantic versioning on forge version component)
///
/// Artifact URL construction (legacy vs modern):
/// - Late MC 1.7.10 Forge (starting at 10.13.2.1300): `forge-{mc}-{forge}-{mc}-installer.jar`
/// - Earlier MC 1.7.10 Forge and newer MC versions: `forge-{mc}-{forge}-installer.jar`
///
/// Examples:
/// - MC "1.20.1" → All versions starting with "1.20.1-" (e.g., "47.4.13", "47.4.10", ...)
/// - MC "1.16.4" → All versions starting with "1.16.4-" (e.g., "35.1.37", "35.1.36", ..., "35.0.0")
/// - MC "1.7.10" → Raw versions until `10.13.2.1291`, then suffixed metadata entries
///   starting at `10.13.2.1300-1.7.10` which are normalized back to raw versions here
pub(crate) fn filter_forge_versions_by_minecraft(
    all_versions: &[String],
    mc_version: &str,
) -> Result<Vec<String>> {
    // Forge supports very old MC versions (back to 1.1), no minimum check needed

    // Normalize MC version (handle "1.21" → "1.21.0" for consistency)
    let normalized_version = if mc_version.matches('.').count() == 1 {
        format!("{}.0", mc_version)
    } else {
        mc_version.to_string()
    };

    // Extract forge version component from full version string
    // Input: "1.20.1-47.4.13" → Output: "47.4.13"
    let extract_forge_version = |full_version: &str, prefix: &str| -> Option<String> {
        full_version.strip_prefix(prefix).map(|v| v.to_string())
    };

    // Collect all matching versions (try both normalized and original)
    let mut matching_versions: Vec<String> = Vec::new();
    let normalized_prefix = format!("{}-", normalized_version);
    let original_prefix = format!("{}-", mc_version);

    for version in all_versions {
        // Try normalized prefix (e.g., "1.21.0-")
        if let Some(forge_ver) = extract_forge_version(version, &normalized_prefix) {
            let forge_ver = canonicalize_forge_loader_version(mc_version, &forge_ver);
            if !matching_versions.contains(&forge_ver) {
                matching_versions.push(forge_ver);
            }
        }

        // Also try original prefix if different (e.g., "1.21-")
        if mc_version != normalized_version
            && let Some(forge_ver) = extract_forge_version(version, &original_prefix)
        {
            let forge_ver = canonicalize_forge_loader_version(mc_version, &forge_ver);
            if !matching_versions.contains(&forge_ver) {
                matching_versions.push(forge_ver);
            }
        }
    }

    sort_versions_desc(&mut matching_versions);

    Ok(matching_versions)
}

#[cfg(test)]
mod tests;
