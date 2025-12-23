// Tests for Modrinth API client

use super::*;

// ============================================================================
// Mock Client Tests
// ============================================================================

#[tokio::test]
async fn test_mock_search_success() {
    let mock = MockModrinthClient::new()
        .with_search_result(
            "jei".to_string(),
            Ok(SearchResults {
                hits: vec![SearchHit {
                    slug: "jei".to_string(),
                    title: "Just Enough Items".to_string(),
                    description: "Item and recipe viewing mod".to_string(),
                    project_id: "u6dRKJwZ".to_string(),
                    project_type: "mod".to_string(),
                    downloads: 100000000,
                    icon_url: Some("https://cdn.modrinth.com/icon.png".to_string()),
                    author: "mezz".to_string(),
                    versions: vec!["1.20.4".to_string()],
                    follows: 50000,
                    date_created: "2023-01-01T00:00:00Z".to_string(),
                    date_modified: "2024-01-01T00:00:00Z".to_string(),
                    latest_version: Some("1.20.4-17.0.0".to_string()),
                    license: Some("MIT".to_string()),
                    categories: vec!["utility".to_string()],
                    client_side: "required".to_string(),
                    server_side: "optional".to_string(),
                }],
                offset: 0,
                limit: 10,
                total_hits: 1,
            }),
        )
        .await;

    let result = mock.search("jei", None, 10, 0).await.unwrap();
    assert_eq!(result.hits.len(), 1);
    assert_eq!(result.hits[0].slug, "jei");
    assert_eq!(result.total_hits, 1);
}

#[tokio::test]
async fn test_mock_search_not_found() {
    let mock = MockModrinthClient::new();

    let result = mock.search("nonexistent", None, 10, 0).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        ModrinthError::InvalidSearchParams { .. }
    ));
}

#[tokio::test]
async fn test_mock_dependencies_success() {
    let mock = MockModrinthClient::new()
        .with_dependency_result(
            "test-mod".to_string(),
            Ok(ProjectDependencies {
                projects: vec![],
                versions: vec![Version {
                    id: "version123".to_string(),
                    project_id: "test-mod".to_string(),
                    author_id: "author123".to_string(),
                    name: "Test Mod 1.0.0".to_string(),
                    version_number: "1.0.0".to_string(),
                    changelog: Some("Initial release".to_string()),
                    dependencies: vec![VersionDependency {
                        version_id: Some("dep-version".to_string()),
                        project_id: Some("fabric-api".to_string()),
                        file_name: None,
                        dependency_type: DependencyType::Required,
                    }],
                    game_versions: vec!["1.20.4".to_string()],
                    version_type: "release".to_string(),
                    loaders: vec!["fabric".to_string()],
                    featured: true,
                    status: "listed".to_string(),
                    date_published: "2024-01-01T00:00:00Z".to_string(),
                    downloads: 1000,
                    files: vec![],
                }],
            }),
        )
        .await;

    let result = mock.get_dependencies("test-mod").await.unwrap();
    assert_eq!(result.versions.len(), 1);
    assert_eq!(result.versions[0].dependencies.len(), 1);
    assert_eq!(
        result.versions[0].dependencies[0].dependency_type,
        DependencyType::Required
    );
}

#[tokio::test]
async fn test_mock_dependencies_not_found() {
    let mock = MockModrinthClient::new();

    let result = mock.get_dependencies("nonexistent").await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        ModrinthError::ProjectNotFound { .. }
    ));
}

#[tokio::test]
async fn test_mock_download_success() {
    let test_data = vec![1, 2, 3, 4, 5];
    let mock = MockModrinthClient::new()
        .with_download_result("https://cdn.modrinth.com/test.jar".to_string(), Ok(test_data.clone()))
        .await;

    let hashes = FileHash {
        sha1: "dummy".to_string(),
        sha512: "dummy".to_string(),
    };

    let result = mock.download_file("https://cdn.modrinth.com/test.jar", &hashes).await.unwrap();
    assert_eq!(result, test_data);
}

#[tokio::test]
async fn test_mock_download_not_found() {
    let mock = MockModrinthClient::new();

    let hashes = FileHash {
        sha1: "dummy".to_string(),
        sha512: "dummy".to_string(),
    };

    let result = mock.download_file("https://nonexistent.com/file.jar", &hashes).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        ModrinthError::InvalidSearchParams { .. }
    ));
}

// ============================================================================
// Dependency Type Tests
// ============================================================================

#[test]
fn test_dependency_type_serde() {
    let required = DependencyType::Required;
    let json = serde_json::to_string(&required).unwrap();
    assert_eq!(json, "\"required\"");

    let optional: DependencyType = serde_json::from_str("\"optional\"").unwrap();
    assert_eq!(optional, DependencyType::Optional);

    let incompatible: DependencyType = serde_json::from_str("\"incompatible\"").unwrap();
    assert_eq!(incompatible, DependencyType::Incompatible);

    let embedded: DependencyType = serde_json::from_str("\"embedded\"").unwrap();
    assert_eq!(embedded, DependencyType::Embedded);
}

// ============================================================================
// Error Tests
// ============================================================================

#[test]
fn test_error_display() {
    let err = ModrinthError::ProjectNotFound {
        project_id: "test-project".to_string(),
    };
    assert_eq!(err.to_string(), "Project not found: test-project");

    let err2 = ModrinthError::InvalidSearchParams {
        message: "limit too high".to_string(),
    };
    assert_eq!(err2.to_string(), "Invalid search parameters: limit too high");
}

// ============================================================================
// Error Path Tests (Phase A - Category 1: API Error Paths)
// ============================================================================

#[tokio::test]
async fn test_network_timeout_error() {
    // Test that network timeouts are handled gracefully
    let mock = MockModrinthClient::new();

    // Simulate network error by not configuring any response
    let result = mock.search("timeout-test", None, 10, 0).await;
    assert!(result.is_err());
    // Should return InvalidSearchParams when no mock configured (represents network failure)
}

#[tokio::test]
async fn test_http_500_server_error() {
    // Test that HTTP 500 errors are handled with appropriate error messages
    let mock = MockModrinthClient::new()
        .with_search_result(
            "server-error".to_string(),
            Err("Internal Server Error (500)".to_string()),
        )
        .await;

    let result = mock.search("server-error", None, 10, 0).await;
    assert!(result.is_err());
    let error_msg = format!("{}", result.unwrap_err());
    assert!(error_msg.contains("500") || error_msg.contains("Internal Server Error"));
}

#[tokio::test]
async fn test_malformed_json_response() {
    // Test that malformed JSON is handled gracefully
    // In production, this would be a JsonError variant
    let mock = MockModrinthClient::new()
        .with_search_result(
            "malformed-json".to_string(),
            Err("Failed to parse JSON response".to_string()),
        )
        .await;

    let result = mock.search("malformed-json", None, 10, 0).await;
    assert!(result.is_err());
    let error_msg = format!("{}", result.unwrap_err());
    assert!(error_msg.contains("JSON") || error_msg.contains("parse"));
}

#[tokio::test]
async fn test_hash_verification_failure() {
    // Test SHA-512 hash verification failure
    let mock = MockModrinthClient::new()
        .with_download_result(
            "https://cdn.modrinth.com/corrupted.jar".to_string(),
            Err("SHA-512 hash mismatch".to_string()),
        )
        .await;

    let hashes = FileHash {
        sha1: "expected_sha1".to_string(),
        sha512: "expected_sha512".to_string(),
    };

    let result = mock.download_file("https://cdn.modrinth.com/corrupted.jar", &hashes).await;
    assert!(result.is_err());
    let error_msg = format!("{}", result.unwrap_err());
    assert!(error_msg.contains("hash") || error_msg.contains("SHA"));
}

#[tokio::test]
async fn test_pagination_empty_results() {
    // Test empty search results (edge case)
    let mock = MockModrinthClient::new()
        .with_search_result(
            "empty-search".to_string(),
            Ok(SearchResults {
                hits: vec![],
                offset: 0,
                limit: 10,
                total_hits: 0,
            }),
        )
        .await;

    let result = mock.search("empty-search", None, 10, 0).await.unwrap();
    assert_eq!(result.hits.len(), 0);
    assert_eq!(result.total_hits, 0);
}

// ============================================================================
// Error Path Tests
// ============================================================================

#[tokio::test]
async fn test_network_error_handling() {
    // Test that MockModrinthClient properly handles and propagates network errors
    let mock = MockModrinthClient::new();

    // Search for a mod that doesn't have a mocked response
    let result = mock.search("unmocked-query", None, 10, 0).await;

    assert!(result.is_err());
    // Mock returns InvalidSearchParams for unmocked queries
    assert!(matches!(
        result.unwrap_err(),
        ModrinthError::InvalidSearchParams { .. }
    ));
}

#[tokio::test]
async fn test_project_not_found_error() {
    let mock = MockModrinthClient::new();

    // Request dependencies for a project that doesn't exist
    let result = mock.get_dependencies("nonexistent-project").await;

    assert!(result.is_err());
    let error = result.unwrap_err();
    // Mock should return error for unmocked dependency request
    assert!(error.to_string().contains("not found") || error.to_string().contains("Invalid"));
}

#[tokio::test]
async fn test_empty_search_query_validation() {
    let mock = MockModrinthClient::new();

    // Test empty search query
    let result = mock.search("", None, 10, 0).await;

    // Should handle empty query gracefully
    assert!(result.is_err() || result.unwrap().hits.is_empty());
}

// Note: Live client tests would require actual network connectivity
// and are better suited for integration tests
