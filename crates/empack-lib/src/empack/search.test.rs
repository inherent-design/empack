use super::*;

#[test]
fn test_calculate_confidence_exact_match() {
    let resolver = ProjectResolver::new(Client::new(), None);

    // Exact match should always return 100%
    assert_eq!(resolver.calculate_confidence("JEI", "JEI", 1000), 100);
    assert_eq!(resolver.calculate_confidence("JEI", "jei", 1000), 100);
    assert_eq!(resolver.calculate_confidence("Just Enough Items", "Just Enough Items", 1000), 100);
    assert_eq!(resolver.calculate_confidence("OptiFine", "OptiFine", 0), 100);
}

#[test]
fn test_calculate_confidence_contains_match() {
    let resolver = ProjectResolver::new(Client::new(), None);

    // Contains match with high downloads should get 85 + 5 = 90%
    assert_eq!(resolver.calculate_confidence("test", "testing", 1000), 90);
    assert_eq!(resolver.calculate_confidence("test", "testing", 2000), 90);
    
    // Contains match with low downloads should get 85%
    assert_eq!(resolver.calculate_confidence("test", "testing", 100), 85);
    assert_eq!(resolver.calculate_confidence("test", "testing", 0), 85);
    
    // Reverse contains match
    assert_eq!(resolver.calculate_confidence("test", "testing", 1000), 90);
    assert_eq!(resolver.calculate_confidence("test", "testing", 100), 85);
}

#[test]
fn test_calculate_confidence_levenshtein_distance() {
    let resolver = ProjectResolver::new(Client::new(), None);

    // Similar strings should have high confidence
    assert!(resolver.calculate_confidence("JEI", "JEI Addon", 1000) > 80);
    assert!(resolver.calculate_confidence("OptiFine", "Optifine", 1000) > 90);
    
    // Very different strings should have low confidence
    assert!(resolver.calculate_confidence("JEI", "Biomes O' Plenty", 1000) < 50);
    assert!(resolver.calculate_confidence("short", "very long string indeed", 1000) < 60);
}

#[test]
fn test_calculate_confidence_download_boost() {
    let resolver = ProjectResolver::new(Client::new(), None);

    // High download count should boost confidence by 5%
    let high_downloads = resolver.calculate_confidence("test", "testing", 1000);
    let low_downloads = resolver.calculate_confidence("test", "testing", 100);
    
    assert_eq!(high_downloads, low_downloads + 5);
}

#[test]
fn test_calculate_confidence_edge_cases() {
    let resolver = ProjectResolver::new(Client::new(), None);

    // Empty strings
    assert_eq!(resolver.calculate_confidence("", "", 1000), 100);
    
    // Empty query or result should have very low confidence
    let empty_query_confidence = resolver.calculate_confidence("", "something", 1000);
    assert!(empty_query_confidence <= 100); // This will use Levenshtein distance
    
    let empty_result_confidence = resolver.calculate_confidence("something", "", 1000);
    assert!(empty_result_confidence <= 100); // This will use Levenshtein distance

    // Very long strings
    let long_query = "a".repeat(100);
    let long_found = "b".repeat(100);
    assert!(resolver.calculate_confidence(&long_query, &long_found, 1000) < 10);
}

#[test]
fn test_has_extra_words_normal_cases() {
    let resolver = ProjectResolver::new(Client::new(), None);

    // Normal acceptable expansion (within 150% ratio)
    assert!(!resolver.has_extra_words("Create", "Create")); // Same length
    assert!(!resolver.has_extra_words("test", "test1")); // 5/4 = 125%
    assert!(!resolver.has_extra_words("mod", "mods")); // 4/3 = 133%
    assert!(!resolver.has_extra_words("ab", "abc")); // 3/2 = 150% exactly
    
    // Acceptable with punctuation that doesn't exceed ratio
    assert!(!resolver.has_extra_words("RF.Tools", "RFTools")); // "rftools" vs "rftools" = 100%
    assert!(!resolver.has_extra_words("a-b", "ab")); // "ab" vs "ab" = 100%
}

#[test]
fn test_has_extra_words_excessive_expansion() {
    let resolver = ProjectResolver::new(Client::new(), None);

    // Excessive expansion should be rejected
    assert!(resolver.has_extra_words("JEI", "Just Enough Items Plus Extra Functionality And More"));
    assert!(resolver.has_extra_words("RF", "Redstone Flux API Implementation Framework"));
    assert!(resolver.has_extra_words("mod", "very long descriptive modification name"));
}

#[test]
fn test_has_extra_words_edge_cases() {
    let resolver = ProjectResolver::new(Client::new(), None);

    // Empty query should not trigger extra words
    assert!(!resolver.has_extra_words("", "anything"));
    assert!(!resolver.has_extra_words("", ""));
    
    // Same length after normalization
    assert!(!resolver.has_extra_words("a-b-c", "abc"));
    assert!(!resolver.has_extra_words("test", "TEST"));
    
    // Exact 150% ratio (boundary condition)
    assert!(!resolver.has_extra_words("ab", "abc")); // 150% exactly
}

#[test]
fn test_has_extra_words_normalization() {
    let resolver = ProjectResolver::new(Client::new(), None);

    // Normalization should remove spaces, dashes, underscores, dots
    assert!(!resolver.has_extra_words("just-enough_items", "Just Enough Items"));
    assert!(!resolver.has_extra_words("rf.tools", "RFTools"));
    assert!(!resolver.has_extra_words("a.b-c_d", "abcd"));
    
    // Case insensitive
    assert!(!resolver.has_extra_words("JEI", "jei"));
    assert!(!resolver.has_extra_words("OptiFine", "optifine"));
}

#[test]
fn test_levenshtein_distance() {
    let resolver = ProjectResolver::new(Client::new(), None);

    // Test basic distance calculations
    assert_eq!(resolver.levenshtein_distance("", ""), 0);
    assert_eq!(resolver.levenshtein_distance("", "a"), 1);
    assert_eq!(resolver.levenshtein_distance("a", ""), 1);
    assert_eq!(resolver.levenshtein_distance("a", "a"), 0);
    assert_eq!(resolver.levenshtein_distance("a", "b"), 1);
    assert_eq!(resolver.levenshtein_distance("ab", "ac"), 1);
    assert_eq!(resolver.levenshtein_distance("abc", "def"), 3);
    
    // Test longer strings
    assert_eq!(resolver.levenshtein_distance("kitten", "sitting"), 3);
    assert_eq!(resolver.levenshtein_distance("saturday", "sunday"), 3);
}

#[test]
fn test_normalize_project_type() {
    let resolver = ProjectResolver::new(Client::new(), None);

    assert_eq!(
        resolver.normalize_project_type("texture-pack"),
        "resourcepack"
    );
    assert_eq!(resolver.normalize_project_type("data-pack"), "datapack");
    assert_eq!(resolver.normalize_project_type("mod"), "mod");
}

#[test]
fn test_curseforge_class_id() {
    let resolver = ProjectResolver::new(Client::new(), None);

    assert_eq!(resolver.curseforge_class_id("mod"), 6);
    assert_eq!(resolver.curseforge_class_id("resourcepack"), 12);
    assert_eq!(resolver.curseforge_class_id("datapack"), 17);
    assert_eq!(resolver.curseforge_class_id("unknown"), 6);
}

#[test]
fn test_curseforge_loader_id() {
    let resolver = ProjectResolver::new(Client::new(), None);

    assert_eq!(resolver.curseforge_loader_id("forge"), Some(1));
    assert_eq!(resolver.curseforge_loader_id("fabric"), Some(4));
    assert_eq!(resolver.curseforge_loader_id("quilt"), Some(5));
    assert_eq!(resolver.curseforge_loader_id("neoforge"), Some(6));
    assert_eq!(resolver.curseforge_loader_id("unknown"), None);
}

// ===== PLATFORM PRIORITY TESTS =====

/// Helper: Modrinth JSON response with a single hit
fn modrinth_hit_json(project_id: &str, title: &str, downloads: u64) -> String {
    serde_json::json!({
        "hits": [{
            "project_id": project_id,
            "title": title,
            "downloads": downloads,
            "categories": ["fabric"]
        }]
    })
    .to_string()
}

/// Helper: CurseForge JSON response with a single result
fn curseforge_hit_json(id: u32, name: &str, download_count: u64) -> String {
    serde_json::json!({
        "data": [{
            "id": id,
            "name": name,
            "downloadCount": download_count
        }]
    })
    .to_string()
}

/// Helper: empty Modrinth response
fn modrinth_empty_json() -> String {
    serde_json::json!({ "hits": [] }).to_string()
}

/// Helper: empty CurseForge response
fn curseforge_empty_json() -> String {
    serde_json::json!({ "data": [] }).to_string()
}

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_platform_priority_default_modrinth_first() {
    // Default (preferred_platform=None): Modrinth is tried first.
    // Mock Modrinth to succeed → result should be Modrinth.
    let mut mr_server = mockito::Server::new_async().await;
    let mut cf_server = mockito::Server::new_async().await;

    let mr_mock = mr_server
        .mock("GET", mockito::Matcher::Regex(r"/v2/search\?.*".to_string()))
        .with_status(200)
        .with_body(modrinth_hit_json("AANobbMI", "Sodium", 50_000))
        .create_async()
        .await;

    // CurseForge should NOT be called
    let cf_mock = cf_server
        .mock("GET", mockito::Matcher::Regex(r"/v1/mods/search\?.*".to_string()))
        .expect(0)
        .create_async()
        .await;

    let resolver = ProjectResolver::new_with_base_urls(
        Client::new(),
        Some("test-api-key".to_string()),
        Some(mr_server.url()),
        Some(cf_server.url()),
    );

    let result = resolver
        .resolve_project("Sodium", Some("mod"), None, None, None)
        .await
        .expect("should resolve");

    assert_eq!(result.platform, ProjectPlatform::Modrinth);
    assert_eq!(result.project_id, "AANobbMI");
    assert_eq!(result.title, "Sodium");
    assert!(result.confidence >= MODRINTH_CONFIDENCE_THRESHOLD);

    mr_mock.assert_async().await;
    cf_mock.assert_async().await;
}

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_platform_priority_preferred_curseforge_first() {
    // preferred_platform=CurseForge: CurseForge is tried first.
    let mut mr_server = mockito::Server::new_async().await;
    let mut cf_server = mockito::Server::new_async().await;

    // Modrinth should NOT be called
    let mr_mock = mr_server
        .mock("GET", mockito::Matcher::Regex(r"/v2/search\?.*".to_string()))
        .expect(0)
        .create_async()
        .await;

    let cf_mock = cf_server
        .mock("GET", mockito::Matcher::Regex(r"/v1/mods/search\?.*".to_string()))
        .with_status(200)
        .with_body(curseforge_hit_json(12345, "Sodium", 50_000))
        .create_async()
        .await;

    let resolver = ProjectResolver::new_with_base_urls(
        Client::new(),
        Some("test-api-key".to_string()),
        Some(mr_server.url()),
        Some(cf_server.url()),
    );

    let result = resolver
        .resolve_project(
            "Sodium",
            Some("mod"),
            None,
            None,
            Some(ProjectPlatform::CurseForge),
        )
        .await
        .expect("should resolve from CurseForge");

    assert_eq!(result.platform, ProjectPlatform::CurseForge);
    assert_eq!(result.project_id, "12345");
    assert_eq!(result.title, "Sodium");

    mr_mock.assert_async().await;
    cf_mock.assert_async().await;
}

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_platform_priority_first_fails_second_succeeds() {
    // Default order: Modrinth first → returns NoResults → falls back to CurseForge
    let mut mr_server = mockito::Server::new_async().await;
    let mut cf_server = mockito::Server::new_async().await;

    mr_server
        .mock("GET", mockito::Matcher::Regex(r"/v2/search\?.*".to_string()))
        .with_status(200)
        .with_body(modrinth_empty_json())
        .create_async()
        .await;

    cf_server
        .mock("GET", mockito::Matcher::Regex(r"/v1/mods/search\?.*".to_string()))
        .with_status(200)
        .with_body(curseforge_hit_json(99, "Sodium", 50_000))
        .create_async()
        .await;

    let resolver = ProjectResolver::new_with_base_urls(
        Client::new(),
        Some("test-api-key".to_string()),
        Some(mr_server.url()),
        Some(cf_server.url()),
    );

    let result = resolver
        .resolve_project("Sodium", Some("mod"), None, None, None)
        .await
        .expect("should fall back to CurseForge");

    assert_eq!(result.platform, ProjectPlatform::CurseForge);
    assert_eq!(result.project_id, "99");
}

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_platform_priority_both_fail_returns_no_results() {
    // Both platforms return empty → final NoResults error
    let mut mr_server = mockito::Server::new_async().await;
    let mut cf_server = mockito::Server::new_async().await;

    mr_server
        .mock("GET", mockito::Matcher::Regex(r"/v2/search\?.*".to_string()))
        .with_status(200)
        .with_body(modrinth_empty_json())
        .create_async()
        .await;

    cf_server
        .mock("GET", mockito::Matcher::Regex(r"/v1/mods/search\?.*".to_string()))
        .with_status(200)
        .with_body(curseforge_empty_json())
        .create_async()
        .await;

    let resolver = ProjectResolver::new_with_base_urls(
        Client::new(),
        Some("test-api-key".to_string()),
        Some(mr_server.url()),
        Some(cf_server.url()),
    );

    let err = resolver
        .resolve_project("NonexistentMod", Some("mod"), None, None, None)
        .await
        .unwrap_err();

    assert!(
        matches!(err, SearchError::NoResults { ref query } if query == "NonexistentMod"),
        "Expected NoResults with query 'NonexistentMod', got: {err:?}"
    );
}

// ===== TIERED SEARCH FALLBACK TESTS =====

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_tiered_search_first_tier_mod_succeeds() {
    // project_type=None: tries mod first → succeeds → stops
    let mut mr_server = mockito::Server::new_async().await;

    // Only expect one call (for "mod" tier)
    let mr_mock = mr_server
        .mock("GET", mockito::Matcher::Regex(r"/v2/search\?.*".to_string()))
        .with_status(200)
        .with_body(modrinth_hit_json("MOD123", "Sodium", 50_000))
        .expect(1)
        .create_async()
        .await;

    let resolver = ProjectResolver::new_with_base_urls(
        Client::new(),
        None, // No CF key: CurseForge will return MissingApiKey (swallowed as Ok(None))
        Some(mr_server.url()),
        Some("http://unused-cf:1".to_string()),
    );

    let result = resolver
        .resolve_project("Sodium", None, None, None, None)
        .await
        .expect("mod tier should succeed");

    assert_eq!(result.project_id, "MOD123");
    assert_eq!(result.project_type, "mod");
    mr_mock.assert_async().await;
}

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_tiered_search_first_fails_second_succeeds() {
    // project_type=None: mod tier → empty → resourcepack tier → success
    let mut mr_server = mockito::Server::new_async().await;

    // First call (mod): empty. Second call (resourcepack): hit.
    mr_server
        .mock("GET", mockito::Matcher::Regex(r"project_type.mod%22".to_string()))
        .with_status(200)
        .with_body(modrinth_empty_json())
        .create_async()
        .await;

    mr_server
        .mock("GET", mockito::Matcher::Regex(r"project_type.resourcepack".to_string()))
        .with_status(200)
        .with_body(modrinth_hit_json("RP456", "Faithful", 30_000))
        .create_async()
        .await;

    let resolver = ProjectResolver::new_with_base_urls(
        Client::new(),
        None,
        Some(mr_server.url()),
        Some("http://unused-cf:1".to_string()),
    );

    let result = resolver
        .resolve_project("Faithful", None, None, None, None)
        .await
        .expect("resourcepack tier should succeed");

    assert_eq!(result.project_id, "RP456");
    assert_eq!(result.project_type, "resourcepack");
}

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_tiered_search_third_tier_shader_succeeds() {
    // mod → empty, resourcepack → empty, shader → success
    let mut mr_server = mockito::Server::new_async().await;

    mr_server
        .mock("GET", mockito::Matcher::Regex(r"project_type.mod%22".to_string()))
        .with_status(200)
        .with_body(modrinth_empty_json())
        .create_async()
        .await;

    mr_server
        .mock("GET", mockito::Matcher::Regex(r"project_type.resourcepack".to_string()))
        .with_status(200)
        .with_body(modrinth_empty_json())
        .create_async()
        .await;

    mr_server
        .mock("GET", mockito::Matcher::Regex(r"project_type.shader".to_string()))
        .with_status(200)
        .with_body(modrinth_hit_json("SH789", "BSL Shaders", 20_000))
        .create_async()
        .await;

    let resolver = ProjectResolver::new_with_base_urls(
        Client::new(),
        None,
        Some(mr_server.url()),
        Some("http://unused-cf:1".to_string()),
    );

    let result = resolver
        .resolve_project("BSL Shaders", None, None, None, None)
        .await
        .expect("shader tier should succeed");

    assert_eq!(result.project_id, "SH789");
    assert_eq!(result.project_type, "shader");
}

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_tiered_search_all_tiers_fail() {
    // All four tiers return empty → NoResults error with original query
    let mut mr_server = mockito::Server::new_async().await;

    mr_server
        .mock("GET", mockito::Matcher::Regex(r"/v2/search\?.*".to_string()))
        .with_status(200)
        .with_body(modrinth_empty_json())
        .create_async()
        .await;

    let resolver = ProjectResolver::new_with_base_urls(
        Client::new(),
        None,
        Some(mr_server.url()),
        Some("http://unused-cf:1".to_string()),
    );

    let err = resolver
        .resolve_project("TotallyFakeMod", None, None, None, None)
        .await
        .unwrap_err();

    assert!(
        matches!(err, SearchError::NoResults { ref query } if query == "TotallyFakeMod"),
        "Expected NoResults with query 'TotallyFakeMod', got: {err:?}"
    );
}

// ===== ERROR PROPAGATION TESTS =====

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_error_propagation_network_error_stops_immediately() {
    // NetworkError on first platform → propagates, second platform NOT tried
    let mut mr_server = mockito::Server::new_async().await;
    let mut cf_server = mockito::Server::new_async().await;

    // Modrinth returns 500 → triggers NetworkError
    mr_server
        .mock("GET", mockito::Matcher::Regex(r"/v2/search\?.*".to_string()))
        .with_status(500)
        .with_body("Internal Server Error")
        .create_async()
        .await;

    // CurseForge should NOT be called
    let cf_mock = cf_server
        .mock("GET", mockito::Matcher::Regex(r"/v1/mods/search\?.*".to_string()))
        .expect(0)
        .create_async()
        .await;

    let resolver = ProjectResolver::new_with_base_urls(
        Client::new(),
        Some("test-api-key".to_string()),
        Some(mr_server.url()),
        Some(cf_server.url()),
    );

    let err = resolver
        .resolve_project("Sodium", Some("mod"), None, None, None)
        .await
        .unwrap_err();

    assert!(
        matches!(err, SearchError::NetworkError { .. }),
        "Expected NetworkError, got: {err:?}"
    );
    cf_mock.assert_async().await;
}

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_error_propagation_network_error_in_tiered_search() {
    // During tiered search: mod tier → NetworkError → propagates immediately,
    // next tiers NOT tried
    let mut mr_server = mockito::Server::new_async().await;

    let mr_mock = mr_server
        .mock("GET", mockito::Matcher::Regex(r"/v2/search\?.*".to_string()))
        .with_status(500)
        .with_body("Internal Server Error")
        .expect(1) // Only one call, then error propagates
        .create_async()
        .await;

    let resolver = ProjectResolver::new_with_base_urls(
        Client::new(),
        None,
        Some(mr_server.url()),
        Some("http://unused-cf:1".to_string()),
    );

    let err = resolver
        .resolve_project("Sodium", None, None, None, None)
        .await
        .unwrap_err();

    assert!(
        matches!(err, SearchError::NetworkError { .. }),
        "Expected NetworkError to propagate through tiered search, got: {err:?}"
    );
    mr_mock.assert_async().await;
}

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_error_propagation_missing_api_key_swallowed() {
    // MissingApiKey is treated as recoverable in try_platform_search (Ok(None)),
    // so it falls through to the next platform.
    // If CurseForge is tried first (preferred) but has no key → falls to Modrinth.
    // But wait: MissingApiKey only happens inside search_curseforge when key is None.
    // With default order: Modrinth first (succeeds or not), then CurseForge (MissingApiKey → Ok(None)).
    // Let's test: no CF key, Modrinth empty → both fail → NoResults
    let mut mr_server = mockito::Server::new_async().await;

    mr_server
        .mock("GET", mockito::Matcher::Regex(r"/v2/search\?.*".to_string()))
        .with_status(200)
        .with_body(modrinth_empty_json())
        .create_async()
        .await;

    let resolver = ProjectResolver::new_with_base_urls(
        Client::new(),
        None, // No CF API key → MissingApiKey when CF is tried → swallowed
        Some(mr_server.url()),
        Some("http://unused-cf:1".to_string()),
    );

    // MissingApiKey does NOT propagate; we get NoResults at the end
    let err = resolver
        .resolve_project("Sodium", Some("mod"), None, None, None)
        .await
        .unwrap_err();

    assert!(
        matches!(err, SearchError::NoResults { .. }),
        "MissingApiKey should be swallowed, final error should be NoResults; got: {err:?}"
    );
}

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_error_propagation_no_results_swallowed_second_platform_succeeds() {
    // Modrinth → NoResults (empty hits) → swallowed, falls to CurseForge → success
    let mut mr_server = mockito::Server::new_async().await;
    let mut cf_server = mockito::Server::new_async().await;

    mr_server
        .mock("GET", mockito::Matcher::Regex(r"/v2/search\?.*".to_string()))
        .with_status(200)
        .with_body(modrinth_empty_json())
        .create_async()
        .await;

    cf_server
        .mock("GET", mockito::Matcher::Regex(r"/v1/mods/search\?.*".to_string()))
        .with_status(200)
        .with_body(curseforge_hit_json(777, "Sodium", 50_000))
        .create_async()
        .await;

    let resolver = ProjectResolver::new_with_base_urls(
        Client::new(),
        Some("test-api-key".to_string()),
        Some(mr_server.url()),
        Some(cf_server.url()),
    );

    let result = resolver
        .resolve_project("Sodium", Some("mod"), None, None, None)
        .await
        .expect("Should fall back to CurseForge after Modrinth NoResults");

    assert_eq!(result.platform, ProjectPlatform::CurseForge);
    assert_eq!(result.project_id, "777");
}

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_error_propagation_low_confidence_swallowed_next_tier_succeeds() {
    // Tiered search: mod tier → low confidence result (swallowed) → resourcepack tier → success.
    // Low confidence: Modrinth returns a result but title is very different from query.
    let mut mr_server = mockito::Server::new_async().await;

    // Mod tier: returns result with wrong title → low confidence → swallowed
    mr_server
        .mock("GET", mockito::Matcher::Regex(r"project_type.mod%22".to_string()))
        .with_status(200)
        .with_body(modrinth_hit_json("WRONG1", "Totally Different Name XYZ", 100))
        .create_async()
        .await;

    // Resourcepack tier: returns exact match → high confidence → accepted
    mr_server
        .mock("GET", mockito::Matcher::Regex(r"project_type.resourcepack".to_string()))
        .with_status(200)
        .with_body(modrinth_hit_json("RP999", "Faithful", 30_000))
        .create_async()
        .await;

    let resolver = ProjectResolver::new_with_base_urls(
        Client::new(),
        None,
        Some(mr_server.url()),
        Some("http://unused-cf:1".to_string()),
    );

    let result = resolver
        .resolve_project("Faithful", None, None, None, None)
        .await
        .expect("Low confidence in mod tier should be swallowed, resourcepack tier should succeed");

    assert_eq!(result.project_id, "RP999");
    assert_eq!(result.project_type, "resourcepack");
}
