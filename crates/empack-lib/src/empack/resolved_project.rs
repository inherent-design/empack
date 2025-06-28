#[derive(Debug, Clone, PartialEq)]
pub enum Platform {
    Modrinth,
    CurseForge,
}

#[derive(Debug, Clone)]
pub struct ResolvedProject {
    pub project_id: String,  // Modrinth/CurseForge project ID
    pub name: String,        // API-provided name for confidence comparison
    pub platform: Platform,  // Which API found this
    pub download_count: u64, // For confidence scoring
}

impl ResolvedProject {
    pub fn new(project_id: String, name: String, platform: Platform, download_count: u64) -> Self {
        Self {
            project_id,
            name,
            platform,
            download_count,
        }
    }

    /// Calculate confidence score based on download count
    pub fn confidence_score(&self) -> u8 {
        // Logarithmic scale: more downloads = higher confidence
        match self.download_count {
            0..=100 => 10,
            101..=1_000 => 20,
            1_001..=10_000 => 40,
            10_001..=100_000 => 60,
            100_001..=1_000_000 => 80,
            _ => 95, // 1M+ downloads
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolved_project_creation() {
        let project = ResolvedProject::new(
            "create-abc123".to_string(),
            "Create".to_string(),
            Platform::Modrinth,
            150_000,
        );

        assert_eq!(project.project_id, "create-abc123");
        assert_eq!(project.name, "Create");
        assert_eq!(project.platform, Platform::Modrinth);
        assert_eq!(project.download_count, 150_000);
    }

    #[test]
    fn test_confidence_scoring() {
        let low_downloads = ResolvedProject::new(
            "test".to_string(),
            "Test".to_string(),
            Platform::Modrinth,
            50,
        );
        let high_downloads = ResolvedProject::new(
            "test".to_string(),
            "Test".to_string(),
            Platform::Modrinth,
            2_000_000,
        );

        assert_eq!(low_downloads.confidence_score(), 10);
        assert_eq!(high_downloads.confidence_score(), 95);
    }

    #[test]
    fn test_platform_comparison() {
        let modrinth_project = ResolvedProject::new(
            "test".to_string(),
            "Test".to_string(),
            Platform::Modrinth,
            1000,
        );
        let curseforge_project = ResolvedProject::new(
            "test".to_string(),
            "Test".to_string(),
            Platform::CurseForge,
            1000,
        );

        assert_ne!(modrinth_project.platform, curseforge_project.platform);
        assert_eq!(modrinth_project.platform, Platform::Modrinth);
        assert_eq!(curseforge_project.platform, Platform::CurseForge);
    }
}
