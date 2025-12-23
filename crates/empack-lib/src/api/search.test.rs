// Tests for SearchProvider trait and implementations

use super::*;
use crate::api::modrinth::{MockModrinthClient, SearchHit};
use crate::api::curseforge::{MockCurseForgeClient, SearchResult as CurseForgeSearchResult, ModAuthor, Category, FileInfo, FileDependency, FileHash};

#[tokio::test]
async fn test_modrinth_search_provider_basic() {
    // Create mock Modrinth client with test data
    let mock_client = MockModrinthClient::new()
        .with_search_result(
            "jei".to_string(),
            Ok(crate::api::modrinth::SearchResults {
                hits: vec![SearchHit {
                    slug: "jei".to_string(),
                    title: "Just Enough Items".to_string(),
                    description: "Item and recipe viewing mod".to_string(),
                    project_id: "u6dRKJwZ".to_string(),
                    project_type: "mod".to_string(),
                    downloads: 100000000,
                    icon_url: Some("https://cdn.modrinth.com/data/u6dRKJwZ/icon.png".to_string()),
                    author: "mezz".to_string(),
                    versions: vec!["1.20.4".to_string(), "1.20.3".to_string()],
                    follows: 5000,
                    date_created: "2016-05-15T10:00:00Z".to_string(),
                    date_modified: "2024-01-10T15:30:00Z".to_string(),
                    latest_version: Some("1.20.4".to_string()),
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

    let provider = ModrinthSearchProvider::new(Arc::new(mock_client));

    let results = provider.search("jei", 10, 0).await.unwrap();

    assert_eq!(results.len(), 1);
    let result = &results[0];
    assert_eq!(result.slug, "jei");
    assert_eq!(result.title, "Just Enough Items");
    assert_eq!(result.platform, ProjectPlatform::Modrinth);
    assert_eq!(result.downloads, 100000000);
    assert_eq!(result.author, "mezz");
}

#[tokio::test]
async fn test_modrinth_search_provider_limit_validation() {
    let mock_client = MockModrinthClient::new();
    let provider = ModrinthSearchProvider::new(Arc::new(mock_client));

    // Modrinth max limit is 100
    let result = provider.search("test", 101, 0).await;
    assert!(result.is_err());
    assert!(matches!(result, Err(SearchError::InvalidQuery(_))));
}

#[tokio::test]
async fn test_modrinth_search_provider_platform() {
    let mock_client = MockModrinthClient::new();
    let provider = ModrinthSearchProvider::new(Arc::new(mock_client));

    assert_eq!(provider.platform(), ProjectPlatform::Modrinth);
}

#[tokio::test]
async fn test_curseforge_search_provider_basic() {
    // Create mock CurseForge client with test data
    let mock_client = MockCurseForgeClient::new()
        .with_search_result(
            "jei".to_string(),
            Ok(crate::api::curseforge::SearchResults {
                data: vec![CurseForgeSearchResult {
                    id: 238222,
                    game_id: 432,
                    name: "Just Enough Items (JEI)".to_string(),
                    slug: "jei".to_string(),
                    summary: "Item and recipe viewing mod for Minecraft".to_string(),
                    download_count: 425000000,
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
                    latest_files: vec![FileInfo {
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
                        dependencies: vec![],
                        hashes: vec![],
                    }],
                }],
                pagination: crate::api::curseforge::Pagination {
                    index: 0,
                    page_size: 50,
                    result_count: 1,
                    total_count: 1,
                },
            }),
        )
        .await;

    let provider = CurseForgeSearchProvider::new(Arc::new(mock_client));

    let results = provider.search("jei", 10, 0).await.unwrap();

    assert_eq!(results.len(), 1);
    let result = &results[0];
    assert_eq!(result.slug, "jei");
    assert_eq!(result.title, "Just Enough Items (JEI)");
    assert_eq!(result.platform, ProjectPlatform::CurseForge);
    assert_eq!(result.downloads, 425000000);
    assert_eq!(result.author, "mezz");
    assert_eq!(result.project_id, "238222");
}

#[tokio::test]
async fn test_curseforge_search_provider_limit_validation() {
    let mock_client = MockCurseForgeClient::new();
    let provider = CurseForgeSearchProvider::new(Arc::new(mock_client));

    // CurseForge max limit is 50
    let result = provider.search("test", 51, 0).await;
    assert!(result.is_err());
    assert!(matches!(result, Err(SearchError::InvalidQuery(_))));
}

#[tokio::test]
async fn test_curseforge_search_provider_pagination_constraint() {
    let mock_client = MockCurseForgeClient::new();
    let provider = CurseForgeSearchProvider::new(Arc::new(mock_client));

    // CurseForge constraint: offset + limit <= 10,000
    let result = provider.search("test", 50, 9951).await;
    assert!(result.is_err());
    assert!(matches!(result, Err(SearchError::InvalidQuery(_))));
}

#[tokio::test]
async fn test_curseforge_search_provider_platform() {
    let mock_client = MockCurseForgeClient::new();
    let provider = CurseForgeSearchProvider::new(Arc::new(mock_client));

    assert_eq!(provider.platform(), ProjectPlatform::CurseForge);
}

#[tokio::test]
async fn test_mock_search_provider() {
    let provider = MockSearchProvider::new(ProjectPlatform::Modrinth)
        .with_results(
            "test-query".to_string(),
            vec![SearchResult {
                slug: "test-mod".to_string(),
                title: "Test Mod".to_string(),
                description: "A test mod".to_string(),
                project_id: "test123".to_string(),
                downloads: 1000,
                platform: ProjectPlatform::Modrinth,
                author: "tester".to_string(),
                versions: vec!["1.20.4".to_string()],
                icon_url: None,
                date_created: "2024-01-01T00:00:00Z".to_string(),
                date_modified: "2024-01-02T00:00:00Z".to_string(),
            }],
        )
        .await;

    let results = provider.search("test-query", 10, 0).await.unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].slug, "test-mod");
    assert_eq!(results[0].title, "Test Mod");
    assert_eq!(provider.platform(), ProjectPlatform::Modrinth);
}

#[tokio::test]
async fn test_mock_search_provider_missing_query() {
    let provider = MockSearchProvider::new(ProjectPlatform::Modrinth);

    let result = provider.search("nonexistent", 10, 0).await;
    assert!(result.is_err());
    assert!(matches!(result, Err(SearchError::InvalidQuery(_))));
}

#[tokio::test]
async fn test_search_result_platform_distinction() {
    // Test that results maintain correct platform attribution
    let modrinth_provider = MockSearchProvider::new(ProjectPlatform::Modrinth)
        .with_results(
            "mod1".to_string(),
            vec![SearchResult {
                slug: "mod1".to_string(),
                title: "Mod 1".to_string(),
                description: "Modrinth mod".to_string(),
                project_id: "mr1".to_string(),
                downloads: 100,
                platform: ProjectPlatform::Modrinth,
                author: "author1".to_string(),
                versions: vec!["1.20.4".to_string()],
                icon_url: None,
                date_created: "2024-01-01T00:00:00Z".to_string(),
                date_modified: "2024-01-01T00:00:00Z".to_string(),
            }],
        )
        .await;

    let curseforge_provider = MockSearchProvider::new(ProjectPlatform::CurseForge)
        .with_results(
            "mod2".to_string(),
            vec![SearchResult {
                slug: "mod2".to_string(),
                title: "Mod 2".to_string(),
                description: "CurseForge mod".to_string(),
                project_id: "12345".to_string(),
                downloads: 200,
                platform: ProjectPlatform::CurseForge,
                author: "author2".to_string(),
                versions: vec!["1.20.4".to_string()],
                icon_url: None,
                date_created: "2024-01-01T00:00:00Z".to_string(),
                date_modified: "2024-01-01T00:00:00Z".to_string(),
            }],
        )
        .await;

    let modrinth_results = modrinth_provider.search("mod1", 10, 0).await.unwrap();
    let curseforge_results = curseforge_provider.search("mod2", 10, 0).await.unwrap();

    assert_eq!(modrinth_results[0].platform, ProjectPlatform::Modrinth);
    assert_eq!(curseforge_results[0].platform, ProjectPlatform::CurseForge);
}

#[tokio::test]
async fn test_curseforge_version_deduplication() {
    // Test that CurseForge provider deduplicates game versions from latest files
    let mock_client = MockCurseForgeClient::new()
        .with_search_result(
            "multi-version".to_string(),
            Ok(crate::api::curseforge::SearchResults {
                data: vec![CurseForgeSearchResult {
                    id: 1,
                    game_id: 432,
                    name: "Multi Version Mod".to_string(),
                    slug: "multi-version".to_string(),
                    summary: "Supports multiple versions".to_string(),
                    download_count: 1000,
                    date_created: "2024-01-01T00:00:00Z".to_string(),
                    date_modified: "2024-01-01T00:00:00Z".to_string(),
                    date_released: "2024-01-01T00:00:00Z".to_string(),
                    authors: vec![ModAuthor {
                        id: 1,
                        name: "author".to_string(),
                        url: "https://example.com".to_string(),
                    }],
                    categories: vec![],
                    latest_files: vec![
                        FileInfo {
                            id: 1,
                            game_id: 432,
                            mod_id: 1,
                            is_available: true,
                            display_name: "file1.jar".to_string(),
                            file_name: "file1.jar".to_string(),
                            file_date: "2024-01-01T00:00:00Z".to_string(),
                            file_length: 1000,
                            download_url: "https://example.com/file1.jar".to_string(),
                            game_versions: vec!["1.20.4".to_string(), "1.20.3".to_string()],
                            dependencies: vec![],
                            hashes: vec![],
                        },
                        FileInfo {
                            id: 2,
                            game_id: 432,
                            mod_id: 1,
                            is_available: true,
                            display_name: "file2.jar".to_string(),
                            file_name: "file2.jar".to_string(),
                            file_date: "2024-01-01T00:00:00Z".to_string(),
                            file_length: 1000,
                            download_url: "https://example.com/file2.jar".to_string(),
                            game_versions: vec!["1.20.4".to_string(), "1.20.2".to_string()],
                            dependencies: vec![],
                            hashes: vec![],
                        },
                    ],
                }],
                pagination: crate::api::curseforge::Pagination {
                    index: 0,
                    page_size: 50,
                    result_count: 1,
                    total_count: 1,
                },
            }),
        )
        .await;

    let provider = CurseForgeSearchProvider::new(Arc::new(mock_client));
    let results = provider.search("multi-version", 10, 0).await.unwrap();

    // Should have deduplicated versions: 1.20.4, 1.20.3, 1.20.2
    assert_eq!(results[0].versions.len(), 3);
    assert!(results[0].versions.contains(&"1.20.4".to_string()));
    assert!(results[0].versions.contains(&"1.20.3".to_string()));
    assert!(results[0].versions.contains(&"1.20.2".to_string()));
}
