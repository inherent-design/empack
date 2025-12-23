//! Fixture infrastructure for E2E tests
//!
//! This module provides utilities for loading VCR cassettes containing
//! real API response fixtures captured from Modrinth and CurseForge APIs.

use anyhow::Result;
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::Value;

/// VCR cassette structure matching our recorded HTTP interactions
#[derive(Debug, Deserialize)]
pub struct VcrCassette {
    pub name: String,
    pub request: VcrRequest,
    pub response: VcrResponse,
}

#[derive(Debug, Deserialize)]
pub struct VcrRequest {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub query: Value,
    #[serde(default)]
    pub headers: Value,
}

#[derive(Debug, Deserialize)]
pub struct VcrResponse {
    pub status: u16,
    #[serde(default)]
    pub headers: Value,
    pub body: Value,
}

/// Load a VCR cassette and extract the response body as a typed value
pub fn load_vcr_response<T>(cassette_path: &str) -> Result<T>
where
    T: DeserializeOwned,
{
    let cassette_content = std::fs::read_to_string(cassette_path)
        .map_err(|e| anyhow::anyhow!("Failed to load VCR cassette '{}': {}", cassette_path, e))?;

    let vcr: VcrCassette = serde_json::from_str(&cassette_content)
        .map_err(|e| anyhow::anyhow!("Failed to parse VCR cassette '{}': {}", cassette_path, e))?;

    serde_json::from_value(vcr.response.body)
        .map_err(|e| anyhow::anyhow!("Failed to deserialize response body from '{}': {}", cassette_path, e))
}

/// Load a VCR cassette's response body as raw JSON string (for mockito)
pub fn load_vcr_body_string(cassette_path: &str) -> Result<String> {
    let cassette_content = std::fs::read_to_string(cassette_path)
        .map_err(|e| anyhow::anyhow!("Failed to load VCR cassette '{}': {}", cassette_path, e))?;

    let vcr: VcrCassette = serde_json::from_str(&cassette_content)
        .map_err(|e| anyhow::anyhow!("Failed to parse VCR cassette '{}': {}", cassette_path, e))?;

    serde_json::to_string(&vcr.response.body)
        .map_err(|e| anyhow::anyhow!("Failed to serialize response body from '{}': {}", cassette_path, e))
}

/// Legacy function - deprecated, use load_vcr_body_string instead
#[deprecated(note = "Use load_vcr_body_string instead")]
pub fn load_fixture(name: &str) -> Result<String> {
    // Map old fixture names to new VCR cassette paths
    let cassette_path = match name {
        "modrinth_search_sodium.json" => {
            format!("{}/fixtures/cassettes/modrinth/search_sodium.json", env!("CARGO_MANIFEST_DIR"))
        }
        "modrinth_search_jei.json" => {
            // Note: This cassette doesn't exist yet, would need to be created
            return Err(anyhow::anyhow!("JEI cassette not yet created - use VCR recording"))
        }
        _ => return Err(anyhow::anyhow!("Unknown fixture '{}'", name))
    };

    load_vcr_body_string(&cassette_path)
}

/// Legacy function - deprecated, use load_vcr_response instead
#[deprecated(note = "Use load_vcr_response instead")]
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
    fn test_load_vcr_cassette() {
        let cassette_path = format!(
            "{}/fixtures/cassettes/modrinth/search_sodium.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let json: Value = load_vcr_response(&cassette_path).unwrap();
        assert!(json["hits"].is_array());
        assert!(json["hits"].as_array().unwrap().len() > 0);

        // Verify it contains sodium project data
        let hits = json["hits"].as_array().unwrap();
        let first_hit = &hits[0];
        assert_eq!(first_hit["project_id"].as_str().unwrap(), "AANobbMI");
        assert_eq!(first_hit["slug"].as_str().unwrap(), "sodium");
    }

    #[test]
    fn test_load_vcr_body_string() {
        let cassette_path = format!(
            "{}/fixtures/cassettes/modrinth/search_sodium.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let body_str = load_vcr_body_string(&cassette_path).unwrap();
        assert!(body_str.contains("sodium"));
        assert!(body_str.contains("AANobbMI"));
    }

    #[test]
    #[allow(deprecated)]
    fn test_legacy_load_fixture() {
        // Test that legacy function still works via compatibility layer
        let fixture = load_fixture("modrinth_search_sodium.json").unwrap();
        assert!(fixture.contains("sodium"));
        assert!(fixture.contains("AANobbMI"));
    }
}
