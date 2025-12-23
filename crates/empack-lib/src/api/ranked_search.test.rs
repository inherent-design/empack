use super::*;
use crate::api::search::{MockSearchProvider, SearchResult};
use crate::primitives::ProjectPlatform;

#[tokio::test]
async fn test_ranked_search_result_creation() {
    let result = SearchResult {
        slug: "jei".to_string(),
        title: "Just Enough Items".to_string(),
        description: "Item and Recipe viewing mod".to_string(),
        project_id: "u6dRKJwZ".to_string(),
        downloads: 100_000_000,
        platform: ProjectPlatform::Modrinth,
        author: "mezz".to_string(),
        versions: vec!["1.21".to_string()],
        icon_url: Some("https://example.com/icon.png".to_string()),
        date_created: "2015-01-01T00:00:00Z".to_string(),
        date_modified: "2024-12-01T00:00:00Z".to_string(),
    };

    let confidence = calculate_confidence("JEI", "Just Enough Items", "jei", 100_000_000, 100_000_000);
    let ranked = RankedSearchResult::new(result.clone(), confidence.clone());

    assert_eq!(ranked.result.slug, "jei");
    assert_eq!(ranked.confidence.score, confidence.score);
}

#[tokio::test]
async fn test_meets_threshold_modrinth() {
    let result = SearchResult {
        slug: "jei".to_string(),
        title: "JEI".to_string(),
        description: "Test".to_string(),
        project_id: "test".to_string(),
        downloads: 1000,
        platform: ProjectPlatform::Modrinth,
        author: "test".to_string(),
        versions: vec![],
        icon_url: None,
        date_created: "2024-01-01T00:00:00Z".to_string(),
        date_modified: "2024-01-01T00:00:00Z".to_string(),
    };

    // High confidence - meets threshold
    let confidence = FuzzyMatch {
        score: 0.95,
        string_similarity: 1.0,
        download_confidence: 0.75,
    };
    let ranked = RankedSearchResult::new(result.clone(), confidence);
    assert!(ranked.meets_threshold());

    // Low confidence - fails threshold
    let confidence = FuzzyMatch {
        score: 0.85,
        string_similarity: 0.85,
        download_confidence: 0.85,
    };
    let ranked = RankedSearchResult::new(result, confidence);
    assert!(!ranked.meets_threshold());
}

#[tokio::test]
async fn test_meets_threshold_curseforge() {
    let result = SearchResult {
        slug: "jei".to_string(),
        title: "JEI".to_string(),
        description: "Test".to_string(),
        project_id: "test".to_string(),
        downloads: 1000,
        platform: ProjectPlatform::CurseForge,
        author: "test".to_string(),
        versions: vec![],
        icon_url: None,
        date_created: "2024-01-01T00:00:00Z".to_string(),
        date_modified: "2024-01-01T00:00:00Z".to_string(),
    };

    // Medium confidence - meets CurseForge threshold (85%)
    let confidence = FuzzyMatch {
        score: 0.87,
        string_similarity: 0.90,
        download_confidence: 0.80,
    };
    let ranked = RankedSearchResult::new(result.clone(), confidence);
    assert!(ranked.meets_threshold());

    // Low confidence - fails threshold
    let confidence = FuzzyMatch {
        score: 0.80,
        string_similarity: 0.80,
        download_confidence: 0.80,
    };
    let ranked = RankedSearchResult::new(result, confidence);
    assert!(!ranked.meets_threshold());
}

#[tokio::test]
async fn test_search_with_confidence_empty_results() {
    let provider = MockSearchProvider::new(ProjectPlatform::Modrinth)
        .with_results("empty".to_string(), vec![])
        .await;

    let results = search_with_confidence(&provider, "empty", 10)
        .await
        .unwrap();

    assert!(results.is_empty());
}

#[tokio::test]
async fn test_search_with_confidence_sorting() {
    let provider = MockSearchProvider::new(ProjectPlatform::Modrinth)
        .with_results(
            "test".to_string(),
            vec![
                SearchResult {
                    slug: "test-low".to_string(),
                    title: "test low".to_string(),
                    description: "Low match".to_string(),
                    project_id: "low".to_string(),
                    downloads: 1000,
                    platform: ProjectPlatform::Modrinth,
                    author: "author".to_string(),
                    versions: vec![],
                    icon_url: None,
                    date_created: "2024-01-01T00:00:00Z".to_string(),
                    date_modified: "2024-01-01T00:00:00Z".to_string(),
                },
                SearchResult {
                    slug: "test-high".to_string(),
                    title: "test".to_string(),
                    description: "High match".to_string(),
                    project_id: "high".to_string(),
                    downloads: 100_000,
                    platform: ProjectPlatform::Modrinth,
                    author: "author".to_string(),
                    versions: vec![],
                    icon_url: None,
                    date_created: "2024-01-01T00:00:00Z".to_string(),
                    date_modified: "2024-01-01T00:00:00Z".to_string(),
                },
                SearchResult {
                    slug: "test-medium".to_string(),
                    title: "test medium".to_string(),
                    description: "Medium match".to_string(),
                    project_id: "medium".to_string(),
                    downloads: 10_000,
                    platform: ProjectPlatform::Modrinth,
                    author: "author".to_string(),
                    versions: vec![],
                    icon_url: None,
                    date_created: "2024-01-01T00:00:00Z".to_string(),
                    date_modified: "2024-01-01T00:00:00Z".to_string(),
                },
            ],
        )
        .await;

    let results = search_with_confidence(&provider, "test", 10).await.unwrap();

    // Should be sorted by confidence descending
    assert!(!results.is_empty());
    for i in 1..results.len() {
        assert!(
            results[i - 1].confidence.score >= results[i].confidence.score,
            "Results not sorted by confidence: {} < {}",
            results[i - 1].confidence.score,
            results[i].confidence.score
        );
    }

    // Highest confidence should be exact match with most downloads
    assert_eq!(results[0].result.project_id, "high");
}

#[tokio::test]
async fn test_search_with_confidence_filters_extra_words() {
    let provider = MockSearchProvider::new(ProjectPlatform::Modrinth)
        .with_results(
            "Apotheosis".to_string(),
            vec![
                SearchResult {
                    slug: "apotheosis".to_string(),
                    title: "Apotheosis".to_string(),
                    description: "Exact match".to_string(),
                    project_id: "exact".to_string(),
                    downloads: 50_000_000,
                    platform: ProjectPlatform::Modrinth,
                    author: "author".to_string(),
                    versions: vec![],
                    icon_url: None,
                    date_created: "2024-01-01T00:00:00Z".to_string(),
                    date_modified: "2024-01-01T00:00:00Z".to_string(),
                },
                SearchResult {
                    slug: "apotheosis-ascended".to_string(),
                    title: "Apotheosis Ascended".to_string(),
                    description: "Extra words".to_string(),
                    project_id: "extra".to_string(),
                    downloads: 5_000_000,
                    platform: ProjectPlatform::Modrinth,
                    author: "author".to_string(),
                    versions: vec![],
                    icon_url: None,
                    date_created: "2024-01-01T00:00:00Z".to_string(),
                    date_modified: "2024-01-01T00:00:00Z".to_string(),
                },
            ],
        )
        .await;

    let results = search_with_confidence(&provider, "Apotheosis", 10)
        .await
        .unwrap();

    // Should only return exact match, not "Apotheosis Ascended"
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].result.project_id, "exact");
    assert_eq!(results[0].result.title, "Apotheosis");
}

#[tokio::test]
async fn test_search_with_confidence_applies_modrinth_threshold() {
    let provider = MockSearchProvider::new(ProjectPlatform::Modrinth)
        .with_results(
            "fuzzy".to_string(),
            vec![
                SearchResult {
                    slug: "fuzzy-exact".to_string(),
                    title: "fuzzy".to_string(),
                    description: "High confidence".to_string(),
                    project_id: "high".to_string(),
                    downloads: 100_000,
                    platform: ProjectPlatform::Modrinth,
                    author: "author".to_string(),
                    versions: vec![],
                    icon_url: None,
                    date_created: "2024-01-01T00:00:00Z".to_string(),
                    date_modified: "2024-01-01T00:00:00Z".to_string(),
                },
                SearchResult {
                    slug: "fuzzy-similar".to_string(),
                    title: "fuzzy match".to_string(),
                    description: "Medium confidence".to_string(),
                    project_id: "medium".to_string(),
                    downloads: 10_000,
                    platform: ProjectPlatform::Modrinth,
                    author: "author".to_string(),
                    versions: vec![],
                    icon_url: None,
                    date_created: "2024-01-01T00:00:00Z".to_string(),
                    date_modified: "2024-01-01T00:00:00Z".to_string(),
                },
            ],
        )
        .await;

    let results = search_with_confidence(&provider, "fuzzy", 10).await.unwrap();

    // Modrinth threshold is 90%, so only high-confidence matches should pass
    for result in &results {
        assert!(
            result.confidence.score >= 0.90,
            "Modrinth result below 90% threshold: {}",
            result.confidence.score
        );
    }
}

#[tokio::test]
async fn test_search_with_confidence_applies_curseforge_threshold() {
    let provider = MockSearchProvider::new(ProjectPlatform::CurseForge)
        .with_results(
            "fuzzy".to_string(),
            vec![
                SearchResult {
                    slug: "fuzzy-exact".to_string(),
                    title: "fuzzy".to_string(),
                    description: "High confidence".to_string(),
                    project_id: "high".to_string(),
                    downloads: 100_000,
                    platform: ProjectPlatform::CurseForge,
                    author: "author".to_string(),
                    versions: vec![],
                    icon_url: None,
                    date_created: "2024-01-01T00:00:00Z".to_string(),
                    date_modified: "2024-01-01T00:00:00Z".to_string(),
                },
                SearchResult {
                    slug: "fuzzy-similar".to_string(),
                    title: "fuzzy match".to_string(),
                    description: "Medium confidence".to_string(),
                    project_id: "medium".to_string(),
                    downloads: 10_000,
                    platform: ProjectPlatform::CurseForge,
                    author: "author".to_string(),
                    versions: vec![],
                    icon_url: None,
                    date_created: "2024-01-01T00:00:00Z".to_string(),
                    date_modified: "2024-01-01T00:00:00Z".to_string(),
                },
            ],
        )
        .await;

    let results = search_with_confidence(&provider, "fuzzy", 10).await.unwrap();

    // CurseForge threshold is 85%, so medium-confidence matches should also pass
    for result in &results {
        assert!(
            result.confidence.score >= 0.85,
            "CurseForge result below 85% threshold: {}",
            result.confidence.score
        );
    }
}

#[tokio::test]
async fn test_search_with_confidence_integration() {
    // Integration test with realistic scenario
    let provider = MockSearchProvider::new(ProjectPlatform::Modrinth)
        .with_results(
            "Citadel".to_string(),
            vec![
                SearchResult {
                    slug: "citadel".to_string(),
                    title: "Citadel".to_string(),
                    description: "Library mod".to_string(),
                    project_id: "jJfV67b1".to_string(),
                    downloads: 50_000_000,
                    platform: ProjectPlatform::Modrinth,
                    author: "author".to_string(),
                    versions: vec!["1.21".to_string()],
                    icon_url: Some("https://example.com/icon.png".to_string()),
                    date_created: "2015-01-01T00:00:00Z".to_string(),
                    date_modified: "2024-12-01T00:00:00Z".to_string(),
                },
                SearchResult {
                    slug: "citadel-compat".to_string(),
                    title: "Citadel Compat".to_string(),
                    description: "Compatibility addon".to_string(),
                    project_id: "compat".to_string(),
                    downloads: 1_000_000,
                    platform: ProjectPlatform::Modrinth,
                    author: "other".to_string(),
                    versions: vec![],
                    icon_url: None,
                    date_created: "2020-01-01T00:00:00Z".to_string(),
                    date_modified: "2024-01-01T00:00:00Z".to_string(),
                },
                SearchResult {
                    slug: "citadel-addon".to_string(),
                    title: "Citadel Addon Pack".to_string(),
                    description: "Another addon".to_string(),
                    project_id: "addon".to_string(),
                    downloads: 500_000,
                    platform: ProjectPlatform::Modrinth,
                    author: "other".to_string(),
                    versions: vec![],
                    icon_url: None,
                    date_created: "2021-01-01T00:00:00Z".to_string(),
                    date_modified: "2024-01-01T00:00:00Z".to_string(),
                },
            ],
        )
        .await;

    let results = search_with_confidence(&provider, "Citadel", 10).await.unwrap();

    // Should have at least one result (exact match)
    assert!(!results.is_empty());

    // All results should meet 90% Modrinth threshold
    for result in &results {
        assert!(result.confidence.score >= 0.90);
    }

    // Results should be sorted by confidence
    for i in 1..results.len() {
        assert!(results[i - 1].confidence.score >= results[i].confidence.score);
    }

    // First result should be exact match
    assert_eq!(results[0].result.project_id, "jJfV67b1");
    assert_eq!(results[0].result.title, "Citadel");
}

#[tokio::test]
async fn test_search_with_confidence_handles_error() {
    let provider = MockSearchProvider::new(ProjectPlatform::Modrinth);

    // Query not in mock results should return error
    let result = search_with_confidence(&provider, "missing", 10).await;
    assert!(result.is_err());
}
