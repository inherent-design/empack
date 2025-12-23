// Tests for CurseForge API client

use super::*;

// ============================================================================
// Mock Client Tests
// ============================================================================

#[tokio::test]
async fn test_mock_search_success() {
    let mock = MockCurseForgeClient::new()
        .with_search_result(
            "jei".to_string(),
            Ok(SearchResults {
                data: vec![SearchResult {
                    id: 238222,
                    game_id: 432,
                    name: "Just Enough Items (JEI)".to_string(),
                    slug: "jei".to_string(),
                    summary: "JEI is an item and recipe viewing mod".to_string(),
                    download_count: 425789012,
                    date_created: "2016-05-15T10:00:00Z".to_string(),
                    date_modified: "2024-01-10T15:30:00Z".to_string(),
                    date_released: "2024-01-10T15:30:00Z".to_string(),
                    authors: vec![ModAuthor {
                        id: 166630,
                        name: "mezz".to_string(),
                        url: "https://www.curseforge.com/members/mezz".to_string(),
                    }],
                    categories: vec![Category {
                        id: 423,
                        name: "Map and Information".to_string(),
                        slug: "map-information".to_string(),
                        icon_url: "https://media.forgecdn.net/icon.png".to_string(),
                    }],
                    latest_files: vec![],
                }],
                pagination: Pagination {
                    index: 0,
                    page_size: 50,
                    result_count: 1,
                    total_count: 1,
                },
            }),
        )
        .await;

    let result = mock.search(432, "jei", 50, 0).await.unwrap();
    assert_eq!(result.data.len(), 1);
    assert_eq!(result.data[0].slug, "jei");
    assert_eq!(result.pagination.total_count, 1);
}

#[tokio::test]
async fn test_mock_search_not_found() {
    let mock = MockCurseForgeClient::new();

    let result = mock.search(432, "nonexistent", 50, 0).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        CurseForgeError::InvalidSearchParams { .. }
    ));
}

#[tokio::test]
async fn test_mock_dependencies_success() {
    let mock = MockCurseForgeClient::new()
        .with_dependency_result(
            "238222:5284115".to_string(),
            Ok(ModDependencies {
                mods: vec![SearchResult {
                    id: 419699,
                    game_id: 432,
                    name: "Bookshelf".to_string(),
                    slug: "bookshelf".to_string(),
                    summary: "Library mod for JEI".to_string(),
                    download_count: 50000000,
                    date_created: "2015-01-01T00:00:00Z".to_string(),
                    date_modified: "2024-01-01T00:00:00Z".to_string(),
                    date_released: "2024-01-01T00:00:00Z".to_string(),
                    authors: vec![],
                    categories: vec![],
                    latest_files: vec![],
                }],
                files: vec![FileInfo {
                    id: 5284115,
                    game_id: 432,
                    mod_id: 238222,
                    is_available: true,
                    display_name: "jei-1.20.4-17.0.0.60.jar".to_string(),
                    file_name: "jei-1.20.4-17.0.0.60.jar".to_string(),
                    file_date: "2024-01-10T15:30:00Z".to_string(),
                    file_length: 1248576,
                    download_url: "https://edge.forgecdn.net/files/5284/115/jei-1.20.4-17.0.0.60.jar".to_string(),
                    game_versions: vec!["1.20.4".to_string(), "Forge".to_string()],
                    dependencies: vec![FileDependency {
                        mod_id: 419699,
                        relation_type: 3,
                    }],
                    hashes: vec![],
                }],
            }),
        )
        .await;

    let result = mock.get_dependencies(238222, 5284115).await.unwrap();
    assert_eq!(result.mods.len(), 1);
    assert_eq!(result.files.len(), 1);
    assert_eq!(result.files[0].dependencies.len(), 1);
    assert_eq!(result.files[0].dependencies[0].relation_type, 3);
}

#[tokio::test]
async fn test_mock_dependencies_not_found() {
    let mock = MockCurseForgeClient::new();

    let result = mock.get_dependencies(999999, 999999).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        CurseForgeError::ModNotFound { .. }
    ));
}

#[tokio::test]
async fn test_mock_download_success() {
    let test_data = vec![1, 2, 3, 4, 5];
    let mock = MockCurseForgeClient::new()
        .with_download_result(
            "https://edge.forgecdn.net/test.jar".to_string(),
            Ok(test_data.clone()),
        )
        .await;

    let hashes = vec![FileHash {
        value: "dummy".to_string(),
        algo: 2,
    }];

    let result = mock
        .download_file("https://edge.forgecdn.net/test.jar", &hashes)
        .await
        .unwrap();
    assert_eq!(result, test_data);
}

#[tokio::test]
async fn test_mock_download_not_found() {
    let mock = MockCurseForgeClient::new();

    let hashes = vec![FileHash {
        value: "dummy".to_string(),
        algo: 2,
    }];

    let result = mock
        .download_file("https://nonexistent.com/file.jar", &hashes)
        .await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        CurseForgeError::InvalidSearchParams { .. }
    ));
}

// ============================================================================
// Dependency Type Tests
// ============================================================================

#[test]
fn test_dependency_type_values() {
    assert_eq!(DependencyType::EmbeddedLibrary as u8, 1);
    assert_eq!(DependencyType::OptionalDependency as u8, 2);
    assert_eq!(DependencyType::RequiredDependency as u8, 3);
    assert_eq!(DependencyType::Tool as u8, 4);
    assert_eq!(DependencyType::Incompatible as u8, 5);
    assert_eq!(DependencyType::Include as u8, 6);
}

// ============================================================================
// Error Tests
// ============================================================================

#[test]
fn test_error_display() {
    let err = CurseForgeError::ModNotFound { mod_id: 238222 };
    assert_eq!(err.to_string(), "Mod not found: 238222");

    let err2 = CurseForgeError::InvalidSearchParams {
        message: "pageSize too high".to_string(),
    };
    assert_eq!(
        err2.to_string(),
        "Invalid search parameters: pageSize too high"
    );

    let err3 = CurseForgeError::FileNotFound { file_id: 5284115 };
    assert_eq!(err3.to_string(), "File not found: 5284115");
}

#[test]
fn test_missing_api_key_error() {
    let err = CurseForgeError::MissingApiKey;
    assert_eq!(err.to_string(), "Missing API key");
}

// ============================================================================
// Search Parameter Validation Tests
// ============================================================================

#[tokio::test]
async fn test_search_pagesize_limit() {
    let mock = MockCurseForgeClient::new();

    // This should be handled by LiveCurseForgeClient validation
    // Mock client doesn't enforce this, so we test error message format
    let result = mock.search(432, "test", 51, 0).await;
    // Mock will fail with no response configured
    assert!(result.is_err());
}

#[tokio::test]
async fn test_search_pagination_limit() {
    let mock = MockCurseForgeClient::new();

    // Test that pagination constraint is documented
    // index + pageSize <= 10,000
    let result = mock.search(432, "test", 50, 9951).await;
    // Mock will fail with no response configured
    assert!(result.is_err());
}

// ============================================================================
// File Hash Tests
// ============================================================================

#[test]
fn test_file_hash_md5() {
    let hash = FileHash {
        value: "a3f8b7c2d1e5f6a9b8c7d6e5f4a3b2c1".to_string(),
        algo: 2, // MD5
    };
    assert_eq!(hash.algo, 2);
}

#[test]
fn test_file_hash_sha1() {
    let hash = FileHash {
        value: "9f8e7d6c5b4a3928374650192837465".to_string(),
        algo: 1, // SHA1
    };
    assert_eq!(hash.algo, 1);
}

// ============================================================================
// Mock Builder Pattern Tests
// ============================================================================

#[tokio::test]
async fn test_mock_builder_chaining() {
    let mock = MockCurseForgeClient::new()
        .with_search_result(
            "test1".to_string(),
            Ok(SearchResults {
                data: vec![],
                pagination: Pagination {
                    index: 0,
                    page_size: 50,
                    result_count: 0,
                    total_count: 0,
                },
            }),
        )
        .await
        .with_search_result(
            "test2".to_string(),
            Ok(SearchResults {
                data: vec![],
                pagination: Pagination {
                    index: 0,
                    page_size: 50,
                    result_count: 0,
                    total_count: 0,
                },
            }),
        )
        .await;

    // Both searches should work
    assert!(mock.search(432, "test1", 50, 0).await.is_ok());
    assert!(mock.search(432, "test2", 50, 0).await.is_ok());
}

// ============================================================================
// Error Path Tests (Phase A - Category 1: API Error Paths)
// ============================================================================

#[tokio::test]
async fn test_http_429_rate_limit_error() {
    // Test HTTP 429 Too Many Requests handling
    let mock = MockCurseForgeClient::new()
        .with_search_result(
            "rate-limited".to_string(),
            Err("Rate limit exceeded (429)".to_string()),
        )
        .await;

    let result = mock.search(432, "rate-limited", 50, 0).await;
    assert!(result.is_err());
    let error_msg = format!("{}", result.unwrap_err());
    assert!(error_msg.contains("429") || error_msg.contains("Rate limit"));
}

#[tokio::test]
async fn test_md5_hash_verification_failure() {
    // Test MD5 hash verification failure for CurseForge
    let mock = MockCurseForgeClient::new()
        .with_download_result(
            "https://edge.forgecdn.net/corrupted.jar".to_string(),
            Err("MD5 hash mismatch".to_string()),
        )
        .await;

    let hashes = vec![FileHash {
        value: "expected_md5_hash".to_string(),
        algo: 2, // MD5
    }];

    let result = mock.download_file("https://edge.forgecdn.net/corrupted.jar", &hashes).await;
    assert!(result.is_err());
    let error_msg = format!("{}", result.unwrap_err());
    assert!(error_msg.contains("hash") || error_msg.contains("MD5"));
}

#[tokio::test]
async fn test_missing_api_key_on_request() {
    // Test that missing API key produces appropriate error
    // MissingApiKey error variant already exists
    let mock = MockCurseForgeClient::new()
        .with_search_result(
            "no-api-key".to_string(),
            Err("Missing API key".to_string()),
        )
        .await;

    let result = mock.search(432, "no-api-key", 50, 0).await;
    assert!(result.is_err());
    let error_msg = format!("{}", result.unwrap_err());
    assert!(error_msg.contains("API key") || error_msg.contains("Missing"));
}

#[tokio::test]
async fn test_pagination_single_result() {
    // Test pagination with exactly 1 result (edge case)
    let mock = MockCurseForgeClient::new()
        .with_search_result(
            "single-result".to_string(),
            Ok(SearchResults {
                data: vec![SearchResult {
                    id: 123456,
                    game_id: 432,
                    name: "Single Mod".to_string(),
                    slug: "single-mod".to_string(),
                    summary: "Only one result".to_string(),
                    download_count: 1000,
                    date_created: "2024-01-01T00:00:00Z".to_string(),
                    date_modified: "2024-01-01T00:00:00Z".to_string(),
                    date_released: "2024-01-01T00:00:00Z".to_string(),
                    authors: vec![],
                    categories: vec![],
                    latest_files: vec![],
                }],
                pagination: Pagination {
                    index: 0,
                    page_size: 50,
                    result_count: 1,
                    total_count: 1,
                },
            }),
        )
        .await;

    let result = mock.search(432, "single-result", 50, 0).await.unwrap();
    assert_eq!(result.data.len(), 1);
    assert_eq!(result.pagination.total_count, 1);
}

#[tokio::test]
async fn test_pagination_max_page_size() {
    // Test pagination with maximum page size (50 for CurseForge)
    let mut data = Vec::new();
    for i in 0..50 {
        data.push(SearchResult {
            id: i,
            game_id: 432,
            name: format!("Mod {}", i),
            slug: format!("mod-{}", i),
            summary: format!("Description {}", i),
            download_count: 1000,
            date_created: "2024-01-01T00:00:00Z".to_string(),
            date_modified: "2024-01-01T00:00:00Z".to_string(),
            date_released: "2024-01-01T00:00:00Z".to_string(),
            authors: vec![],
            categories: vec![],
            latest_files: vec![],
        });
    }

    let mock = MockCurseForgeClient::new()
        .with_search_result(
            "max-page".to_string(),
            Ok(SearchResults {
                data,
                pagination: Pagination {
                    index: 0,
                    page_size: 50,
                    result_count: 50,
                    total_count: 100, // More results available
                },
            }),
        )
        .await;

    let result = mock.search(432, "max-page", 50, 0).await.unwrap();
    assert_eq!(result.data.len(), 50);
    assert_eq!(result.pagination.page_size, 50);
    assert_eq!(result.pagination.total_count, 100);
}

// Note: Live client tests would require actual API key and network connectivity
// and are better suited for integration tests
