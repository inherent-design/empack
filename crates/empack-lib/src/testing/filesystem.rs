//! Filesystem-based integration testing utilities
//!
//! Provides systematic test patterns for filesystem operations with:
//! - Automatic temporary directory creation and cleanup
//! - RAII-based resource management
//! - Isolation between test runs
//! - Structured patterns for empack filesystem operations

use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// Temporary directory fixture with automatic cleanup
pub struct TempDirFixture {
    /// The temporary directory (automatically cleaned up on drop)
    pub temp_dir: TempDir,
}

impl TempDirFixture {
    /// Create a new temporary directory fixture
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let temp_dir = tempfile::tempdir()?;
        Ok(Self { temp_dir })
    }

    /// Get the path to the temporary directory
    pub fn path(&self) -> &Path {
        self.temp_dir.path()
    }

    /// Create a subdirectory within the temporary directory
    pub fn create_dir(&self, subdir: &str) -> Result<(), Box<dyn std::error::Error>> {
        let dir_path = self.path().join(subdir);
        fs::create_dir_all(&dir_path)?;
        Ok(())
    }

    /// Write content to a file within the temporary directory
    pub fn write_file(
        &self,
        file_path: &str,
        content: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let full_path = self.path().join(file_path);

        // Ensure parent directory exists
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(&full_path, content)?;
        Ok(())
    }

    /// Read content from a file within the temporary directory
    pub fn read_file(&self, file_path: &str) -> Result<String, Box<dyn std::error::Error>> {
        let full_path = self.path().join(file_path);
        let content = fs::read_to_string(&full_path)?;
        Ok(content)
    }

    /// Check if a file exists within the temporary directory
    pub fn file_exists(&self, file_path: &str) -> bool {
        let full_path = self.path().join(file_path);
        full_path.exists()
    }
}

#[cfg(test)]
mod tests {
    include!("filesystem.test.rs");
}
