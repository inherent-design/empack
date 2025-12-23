use super::*;
use crate::api::search::{MockSearchProvider, SearchResult};
use crate::primitives::ProjectPlatform;

fn mock_result(platform: ProjectPlatform, slug: &str, title: &str, downloads: u64) -> SearchResult {
    SearchResult {
        slug: slug.to_string(),
        title: title.to_string(),
        description: "Test mod".to_string(),
        project_id: slug.to_string(),
        downloads,
        platform,
        author: "test".to_string(),
        versions: vec!["1.21".to_string()],
        icon_url: None,
        date_created: "2024-01-01T00:00:00Z".to_string(),
        date_modified: "2024-01-01T00:00:00Z".to_string(),
    }
}

#[tokio::test]
async fn test_resolution_creation() {
    let result = mock_result(ProjectPlatform::Modrinth, "citadel", "Citadel", 50_000_000);
    let resolution = Resolution::new(result.clone(), 0.95, false);

    assert_eq!(resolution.project_id(), "citadel");
    assert_eq!(resolution.title(), "Citadel");
    assert_eq!(resolution.slug(), "citadel");
    assert_eq!(resolution.confidence, 0.95);
    assert_eq!(resolution.platform, ProjectPlatform::Modrinth);
    assert!(!resolution.was_fallback);
}

#[tokio::test]
async fn test_resolver_modrinth_success() {
    // Modrinth has high-confidence match
    let modrinth = Arc::new(
        MockSearchProvider::new(ProjectPlatform::Modrinth)
            .with_results(
                "Citadel".to_string(),
                vec![mock_result(
                    ProjectPlatform::Modrinth,
                    "citadel",
                    "Citadel",
                    50_000_000,
                )],
            )
            .await,
    );

    let curseforge = Arc::new(MockSearchProvider::new(ProjectPlatform::CurseForge));
    let resolver = RemoteResolver::new(modrinth, curseforge);

    let resolution = resolver.resolve("Citadel").await.unwrap();

    assert_eq!(resolution.platform, ProjectPlatform::Modrinth);
    assert_eq!(resolution.title(), "Citadel");
    assert!(!resolution.was_fallback);
    assert!(resolution.confidence >= 0.90); // Modrinth threshold
}

#[tokio::test]
async fn test_resolver_curseforge_fallback() {
    // Modrinth has no results, CurseForge has high-confidence match
    let modrinth = Arc::new(
        MockSearchProvider::new(ProjectPlatform::Modrinth)
            .with_results("Botarium".to_string(), vec![])
            .await,
    );

    let curseforge = Arc::new(
        MockSearchProvider::new(ProjectPlatform::CurseForge)
            .with_results(
                "Botarium".to_string(),
                vec![mock_result(
                    ProjectPlatform::CurseForge,
                    "botarium",
                    "Botarium",
                    10_000_000,
                )],
            )
            .await,
    );

    let resolver = RemoteResolver::new(modrinth, curseforge);
    let resolution = resolver.resolve("Botarium").await.unwrap();

    assert_eq!(resolution.platform, ProjectPlatform::CurseForge);
    assert_eq!(resolution.title(), "Botarium");
    assert!(resolution.was_fallback);
    assert!(resolution.confidence >= 0.85); // CurseForge threshold
}

#[tokio::test]
async fn test_resolver_modrinth_preferred_over_curseforge() {
    // Both platforms have results, Modrinth should win (preferred)
    let modrinth = Arc::new(
        MockSearchProvider::new(ProjectPlatform::Modrinth)
            .with_results(
                "Citadel".to_string(),
                vec![mock_result(
                    ProjectPlatform::Modrinth,
                    "citadel",
                    "Citadel",
                    30_000_000,
                )],
            )
            .await,
    );

    let curseforge = Arc::new(
        MockSearchProvider::new(ProjectPlatform::CurseForge)
            .with_results(
                "Citadel".to_string(),
                vec![mock_result(
                    ProjectPlatform::CurseForge,
                    "citadel",
                    "Citadel",
                    25_000_000,
                )],
            )
            .await,
    );

    let resolver = RemoteResolver::new(modrinth, curseforge);
    let resolution = resolver.resolve("Citadel").await.unwrap();

    // Should prefer Modrinth even if both are available
    assert_eq!(resolution.platform, ProjectPlatform::Modrinth);
    assert!(!resolution.was_fallback);
}

#[tokio::test]
async fn test_resolver_no_match() {
    // Neither platform has high-confidence results
    let modrinth = Arc::new(
        MockSearchProvider::new(ProjectPlatform::Modrinth)
            .with_results("Unknown".to_string(), vec![])
            .await,
    );

    let curseforge = Arc::new(
        MockSearchProvider::new(ProjectPlatform::CurseForge)
            .with_results("Unknown".to_string(), vec![])
            .await,
    );

    let resolver = RemoteResolver::new(modrinth, curseforge);
    let result = resolver.resolve("Unknown").await;

    assert!(result.is_err());
    match result {
        Err(ResolverError::NoMatch { query }) => {
            assert_eq!(query, "Unknown");
        }
        _ => panic!("Expected NoMatch error"),
    }
}

#[tokio::test]
async fn test_resolver_extra_words_rejection() {
    // Modrinth has result with extra words (should be filtered out)
    let modrinth = Arc::new(
        MockSearchProvider::new(ProjectPlatform::Modrinth)
            .with_results(
                "Apotheosis".to_string(),
                vec![mock_result(
                    ProjectPlatform::Modrinth,
                    "apotheosis-ascended",
                    "Apotheosis Ascended", // Extra words
                    5_000_000,
                )],
            )
            .await,
    );

    let curseforge = Arc::new(
        MockSearchProvider::new(ProjectPlatform::CurseForge)
            .with_results("Apotheosis".to_string(), vec![])
            .await,
    );

    let resolver = RemoteResolver::new(modrinth, curseforge);
    let result = resolver.resolve("Apotheosis").await;

    // Should fail - extra words rejected on both platforms
    assert!(result.is_err());
}

#[tokio::test]
async fn test_resolver_confidence_threshold_filtering() {
    // Modrinth has low-confidence match (below 90%), should fallback to CurseForge
    let modrinth = Arc::new(
        MockSearchProvider::new(ProjectPlatform::Modrinth)
            .with_results(
                "fuzzy".to_string(),
                vec![mock_result(
                    ProjectPlatform::Modrinth,
                    "fuzzy-similar",
                    "fuzzy match", // Low similarity
                    1_000,         // Low downloads
                )],
            )
            .await,
    );

    let curseforge = Arc::new(
        MockSearchProvider::new(ProjectPlatform::CurseForge)
            .with_results(
                "fuzzy".to_string(),
                vec![mock_result(
                    ProjectPlatform::CurseForge,
                    "fuzzy",
                    "fuzzy", // Exact match
                    100_000,
                )],
            )
            .await,
    );

    let resolver = RemoteResolver::new(modrinth, curseforge);
    let resolution = resolver.resolve("fuzzy").await.unwrap();

    // Should fallback to CurseForge (exact match)
    assert_eq!(resolution.platform, ProjectPlatform::CurseForge);
    assert!(resolution.was_fallback);
}

#[tokio::test]
async fn test_resolve_all_multiple_queries() {
    let modrinth = Arc::new(
        MockSearchProvider::new(ProjectPlatform::Modrinth)
            .with_results(
                "Citadel".to_string(),
                vec![mock_result(
                    ProjectPlatform::Modrinth,
                    "citadel",
                    "Citadel",
                    50_000_000,
                )],
            )
            .await
            .with_results(
                "Moonlight".to_string(),
                vec![mock_result(
                    ProjectPlatform::Modrinth,
                    "moonlight",
                    "Moonlight",
                    30_000_000,
                )],
            )
            .await
            .with_results("Botarium".to_string(), vec![])
            .await,
    );

    let curseforge = Arc::new(
        MockSearchProvider::new(ProjectPlatform::CurseForge)
            .with_results(
                "Botarium".to_string(),
                vec![mock_result(
                    ProjectPlatform::CurseForge,
                    "botarium",
                    "Botarium",
                    10_000_000,
                )],
            )
            .await,
    );

    let resolver = RemoteResolver::new(modrinth, curseforge);
    let queries = vec![
        "Citadel".to_string(),
        "Moonlight".to_string(),
        "Botarium".to_string(),
    ];

    let results = resolver.resolve_all(&queries).await;

    assert_eq!(results.len(), 3);

    // Citadel - Modrinth
    assert!(results[0].is_ok());
    let citadel = results[0].as_ref().unwrap();
    assert_eq!(citadel.title(), "Citadel");
    assert_eq!(citadel.platform, ProjectPlatform::Modrinth);
    assert!(!citadel.was_fallback);

    // Moonlight - Modrinth
    assert!(results[1].is_ok());
    let moonlight = results[1].as_ref().unwrap();
    assert_eq!(moonlight.title(), "Moonlight");
    assert_eq!(moonlight.platform, ProjectPlatform::Modrinth);
    assert!(!moonlight.was_fallback);

    // Botarium - CurseForge (fallback)
    assert!(results[2].is_ok());
    let botarium = results[2].as_ref().unwrap();
    assert_eq!(botarium.title(), "Botarium");
    assert_eq!(botarium.platform, ProjectPlatform::CurseForge);
    assert!(botarium.was_fallback);
}

#[tokio::test]
async fn test_resolve_all_handles_failures() {
    let modrinth = Arc::new(
        MockSearchProvider::new(ProjectPlatform::Modrinth)
            .with_results(
                "Found".to_string(),
                vec![mock_result(
                    ProjectPlatform::Modrinth,
                    "found",
                    "Found",
                    1_000_000,
                )],
            )
            .await
            .with_results("NotFound".to_string(), vec![])
            .await,
    );

    let curseforge = Arc::new(
        MockSearchProvider::new(ProjectPlatform::CurseForge)
            .with_results("NotFound".to_string(), vec![])
            .await,
    );

    let resolver = RemoteResolver::new(modrinth, curseforge);
    let queries = vec!["Found".to_string(), "NotFound".to_string()];

    let results = resolver.resolve_all(&queries).await;

    assert_eq!(results.len(), 2);

    // First query succeeds
    assert!(results[0].is_ok());

    // Second query fails
    assert!(results[1].is_err());
}

#[tokio::test]
async fn test_resolver_handles_search_errors() {
    // Mock provider that will return errors for unknown queries
    let modrinth = Arc::new(MockSearchProvider::new(ProjectPlatform::Modrinth));
    let curseforge = Arc::new(MockSearchProvider::new(ProjectPlatform::CurseForge));

    let resolver = RemoteResolver::new(modrinth, curseforge);
    let result = resolver.resolve("UnknownQuery").await;

    // Should propagate search error
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ResolverError::Search(_)));
}
