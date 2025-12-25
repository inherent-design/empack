//! Project hosting platform types
//!
//! Canonical definition of project hosting platforms (NOT mod loaders).
//! This module provides the single source of truth for platform enumeration
//! across all empack modules (networking, search, dependency resolution).

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Project hosting platform enumeration
///
/// Represents platforms where projects are downloaded from (NOT mod loaders).
/// - Modrinth: Open platform, no API key required
/// - CurseForge: Requires API key
///
/// # Important Distinction
/// Forge/NeoForge/Fabric/Quilt are mod LOADERS, not platforms.
/// They specify runtime environments, not where projects are hosted.
///
/// # Usage
/// ```
/// use empack_lib::primitives::ProjectPlatform;
///
/// let platform = ProjectPlatform::Modrinth;
/// assert_eq!(platform.rate_limit(), 300);
/// assert_eq!(platform.to_string(), "modrinth");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProjectPlatform {
    /// Modrinth platform (https://modrinth.com)
    Modrinth,
    /// CurseForge platform (https://www.curseforge.com)
    CurseForge,
}

impl ProjectPlatform {
    /// Get the rate limit for this platform (requests per minute)
    ///
    /// Based on official API documentation and production usage patterns:
    /// - Modrinth: 300 req/min (documented limit)
    /// - CurseForge: 60 req/min (conservative, requires API key)
    pub fn rate_limit(&self) -> u32 {
        match self {
            ProjectPlatform::Modrinth => 300,
            ProjectPlatform::CurseForge => 60,
        }
    }

    /// Get the burst size for this platform
    ///
    /// Allows short bursts above sustained rate limit.
    /// Configured as 2x rate limit for both platforms.
    pub fn burst_size(&self) -> u32 {
        match self {
            ProjectPlatform::Modrinth => 600,   // 2x burst tolerance
            ProjectPlatform::CurseForge => 120, // 2x burst tolerance
        }
    }

    /// Get the base URL for this platform's API
    pub fn api_base_url(&self) -> &'static str {
        match self {
            ProjectPlatform::Modrinth => "https://api.modrinth.com",
            ProjectPlatform::CurseForge => "https://api.curseforge.com",
        }
    }

    /// Get the default timeout for requests to this platform
    pub fn default_timeout(&self) -> Duration {
        match self {
            ProjectPlatform::Modrinth => Duration::from_secs(30),
            ProjectPlatform::CurseForge => Duration::from_secs(60),
        }
    }

    /// Check if this platform requires an API key
    pub fn requires_api_key(&self) -> bool {
        match self {
            ProjectPlatform::Modrinth => false,
            ProjectPlatform::CurseForge => true,
        }
    }

    /// Get the environment variable name for the API key
    pub fn api_key_env_var(&self) -> &'static str {
        match self {
            ProjectPlatform::Modrinth => "EMPACK_KEY_MODRINTH", // Optional for higher limits
            ProjectPlatform::CurseForge => "EMPACK_KEY_CURSEFORGE", // Required
        }
    }

    /// Parse platform from string (case-insensitive)
    ///
    /// # Examples
    /// ```
    /// use empack_lib::primitives::ProjectPlatform;
    ///
    /// assert_eq!(ProjectPlatform::from_str("modrinth").unwrap(), ProjectPlatform::Modrinth);
    /// assert_eq!(ProjectPlatform::from_str("CURSEFORGE").unwrap(), ProjectPlatform::CurseForge);
    /// ```
    pub fn from_str(s: &str) -> Result<Self, String> {
        match s.to_lowercase().as_str() {
            "modrinth" => Ok(ProjectPlatform::Modrinth),
            "curseforge" | "curse" => Ok(ProjectPlatform::CurseForge),
            _ => Err(format!("Invalid platform: {}", s)),
        }
    }
}

impl std::fmt::Display for ProjectPlatform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProjectPlatform::Modrinth => write!(f, "modrinth"),
            ProjectPlatform::CurseForge => write!(f, "curseforge"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limits() {
        assert_eq!(ProjectPlatform::Modrinth.rate_limit(), 300);
        assert_eq!(ProjectPlatform::CurseForge.rate_limit(), 60);
    }

    #[test]
    fn test_burst_sizes() {
        assert_eq!(ProjectPlatform::Modrinth.burst_size(), 600);
        assert_eq!(ProjectPlatform::CurseForge.burst_size(), 120);
    }

    #[test]
    fn test_api_base_urls() {
        assert_eq!(
            ProjectPlatform::Modrinth.api_base_url(),
            "https://api.modrinth.com"
        );
        assert_eq!(
            ProjectPlatform::CurseForge.api_base_url(),
            "https://api.curseforge.com"
        );
    }

    #[test]
    fn test_requires_api_key() {
        assert!(!ProjectPlatform::Modrinth.requires_api_key());
        assert!(ProjectPlatform::CurseForge.requires_api_key());
    }

    #[test]
    fn test_api_key_env_vars() {
        assert_eq!(ProjectPlatform::Modrinth.api_key_env_var(), "EMPACK_KEY_MODRINTH");
        assert_eq!(
            ProjectPlatform::CurseForge.api_key_env_var(),
            "EMPACK_KEY_CURSEFORGE"
        );
    }

    #[test]
    fn test_from_str() {
        assert_eq!(
            ProjectPlatform::from_str("modrinth").unwrap(),
            ProjectPlatform::Modrinth
        );
        assert_eq!(
            ProjectPlatform::from_str("MODRINTH").unwrap(),
            ProjectPlatform::Modrinth
        );
        assert_eq!(
            ProjectPlatform::from_str("curseforge").unwrap(),
            ProjectPlatform::CurseForge
        );
        assert_eq!(
            ProjectPlatform::from_str("curse").unwrap(),
            ProjectPlatform::CurseForge
        );
        assert!(ProjectPlatform::from_str("forge").is_err());
    }

    #[test]
    fn test_display() {
        assert_eq!(ProjectPlatform::Modrinth.to_string(), "modrinth");
        assert_eq!(ProjectPlatform::CurseForge.to_string(), "curseforge");
    }

    #[test]
    fn test_serialization() {
        let platform = ProjectPlatform::Modrinth;
        let json = serde_json::to_string(&platform).unwrap();
        assert_eq!(json, "\"Modrinth\"");

        let deserialized: ProjectPlatform = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, platform);
    }

    #[test]
    fn test_equality() {
        assert_eq!(ProjectPlatform::Modrinth, ProjectPlatform::Modrinth);
        assert_ne!(ProjectPlatform::Modrinth, ProjectPlatform::CurseForge);
    }

    #[test]
    fn test_hash() {
        use std::collections::HashSet;

        let mut set = HashSet::new();
        set.insert(ProjectPlatform::Modrinth);
        set.insert(ProjectPlatform::CurseForge);
        set.insert(ProjectPlatform::Modrinth); // Duplicate

        assert_eq!(set.len(), 2);
        assert!(set.contains(&ProjectPlatform::Modrinth));
        assert!(set.contains(&ProjectPlatform::CurseForge));
    }
}
