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
    include!("resolved_project.test.rs");
}
