//! Fixture infrastructure for E2E tests
//!
//! This module provides utilities for loading real API response fixtures
//! captured from Modrinth and CurseForge APIs to ground our tests in reality.

use anyhow::Result;
use serde_json::Value;
use std::path::Path;

/// Load a JSON fixture from the fixtures directory
pub fn load_fixture(name: &str) -> Result<String> {
    let fixture_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join("api_responses")
        .join(name);

    std::fs::read_to_string(fixture_path)
        .map_err(|e| anyhow::anyhow!("Failed to load fixture '{}': {}", name, e))
}

/// Load a JSON fixture and parse it as serde_json::Value
pub fn load_fixture_json(name: &str) -> Result<Value> {
    let content = load_fixture(name)?;
    serde_json::from_str(&content)
        .map_err(|e| anyhow::anyhow!("Failed to parse fixture '{}' as JSON: {}", name, e))
}

/// Get the base URL for mockito server
#[cfg(test)]
pub fn mockito_url(server: &mockito::Server) -> String {
    server.url()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_sodium_fixture() {
        let fixture = load_fixture("modrinth_search_sodium.json").unwrap();
        assert!(fixture.contains("sodium"));
        assert!(fixture.contains("AANobbMI"));
    }

    #[test]
    fn test_load_jei_fixture() {
        let fixture = load_fixture("modrinth_search_jei.json").unwrap();
        assert!(fixture.contains("jei"));
    }

    #[test]
    fn test_load_fixture_json() {
        let json = load_fixture_json("modrinth_search_sodium.json").unwrap();
        assert!(json["hits"].is_array());
        assert!(json["hits"].as_array().unwrap().len() > 0);
    }
}
