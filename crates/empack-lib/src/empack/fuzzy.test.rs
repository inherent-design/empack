use super::*;

// ============================================================================
// String Similarity Tests
// ============================================================================

#[test]
fn test_calculate_confidence_exact_match() {
    // Exact match should have very high confidence
    let conf = calculate_confidence("jei", "JEI", "jei", 1_000_000, 1_000_000);
    assert!(
        conf.score >= 0.9,
        "Exact match should have >=90% confidence, got {}",
        conf.score
    );
    assert!(
        conf.string_similarity >= 0.9,
        "String similarity should be very high for exact match"
    );
}

#[test]
fn test_calculate_confidence_case_insensitive() {
    // Case differences should not significantly affect similarity
    let conf = calculate_confidence("citadel", "Citadel", "citadel", 50_000_000, 50_000_000);
    assert!(
        conf.string_similarity >= 0.85,
        "Case differences should have minimal impact on similarity"
    );
}

#[test]
fn test_calculate_confidence_slug_matching() {
    // Should match against slug when title differs
    let conf = calculate_confidence(
        "just-enough-items",
        "JEI (Just Enough Items)",
        "just-enough-items",
        1_000_000,
        1_000_000,
    );
    assert!(
        conf.string_similarity >= 0.8,
        "Should match well against slug"
    );
}

#[test]
fn test_calculate_confidence_partial_match() {
    // Partial match should have lower confidence
    let conf = calculate_confidence(
        "Apotheosis",
        "Apotheosis Ascended",
        "apotheosis-ascended",
        5_000_000,
        50_000_000,
    );
    // Should be below Modrinth threshold (90%)
    assert!(
        conf.score < 0.90,
        "Partial match should fail Modrinth threshold"
    );
}

#[test]
fn test_calculate_confidence_completely_different() {
    // Completely different strings should have very low confidence
    let conf = calculate_confidence("JEI", "Create Mod", "create", 1_000_000, 1_000_000);
    assert!(
        conf.string_similarity < 0.3,
        "Unrelated strings should have very low similarity"
    );
}

// ============================================================================
// Download Confidence Tests
// ============================================================================

#[test]
fn test_download_confidence_scaling() {
    // Higher downloads should yield higher confidence (logarithmic)
    let conf_low = calculate_confidence("test", "test", "test", 1_000, 10_000_000);
    let conf_mid = calculate_confidence("test", "test", "test", 100_000, 10_000_000);
    let conf_high = calculate_confidence("test", "test", "test", 1_000_000, 10_000_000);

    assert!(
        conf_mid.download_confidence > conf_low.download_confidence,
        "100k downloads should have higher confidence than 1k"
    );
    assert!(
        conf_high.download_confidence > conf_mid.download_confidence,
        "1M downloads should have higher confidence than 100k"
    );
}

#[test]
fn test_download_confidence_max() {
    // Maximum downloads should yield 1.0 download confidence
    let conf = calculate_confidence("test", "test", "test", 10_000_000, 10_000_000);
    assert!(
        (conf.download_confidence - 1.0).abs() < 0.01,
        "Max downloads should yield ~1.0 confidence, got {}",
        conf.download_confidence
    );
}

#[test]
fn test_download_confidence_zero() {
    // Zero downloads should yield 0.0 download confidence
    let conf = calculate_confidence("test", "test", "test", 0, 10_000_000);
    assert_eq!(conf.download_confidence, 0.0);
}

#[test]
fn test_download_confidence_weighting() {
    // Download confidence should be 30% of total score
    // If string similarity = 1.0 and downloads = max:
    // score = (1.0 * 0.7) + (1.0 * 0.3) = 1.0
    let conf = calculate_confidence("test", "test", "test", 10_000_000, 10_000_000);
    assert!(
        (conf.score - 1.0).abs() < 0.01,
        "Perfect match with max downloads should yield ~1.0 score"
    );

    // If string similarity = 1.0 and downloads = 0:
    // score = (1.0 * 0.7) + (0.0 * 0.3) = 0.7
    let conf = calculate_confidence("test", "test", "test", 0, 10_000_000);
    assert!(
        (conf.score - 0.7).abs() < 0.01,
        "Perfect string match with 0 downloads should yield ~0.7 score"
    );
}

// ============================================================================
// Extra Words Detection Tests
// ============================================================================

#[test]
fn test_has_extra_words_rejects_variants() {
    // Should reject mods with extra words
    assert!(
        has_extra_words("Apotheosis", "Apotheosis Ascended"),
        "Should detect 'Ascended' as extra word"
    );
    assert!(
        has_extra_words("Create", "Create: Steam 'n' Rails"),
        "Should detect addon name as extra words"
    );
}

#[test]
fn test_has_extra_words_allows_exact() {
    // Exact matches should not be flagged
    assert!(
        !has_extra_words("JEI", "JEI"),
        "Exact match should not have extra words"
    );
    assert!(
        !has_extra_words("Fabric API", "Fabric API"),
        "Exact multi-word match should not have extra words"
    );
}

#[test]
fn test_has_extra_words_case_insensitive() {
    // Case should not affect extra words detection
    assert!(
        has_extra_words("apotheosis", "Apotheosis Ascended"),
        "Should detect extra words regardless of case"
    );
    assert!(
        !has_extra_words("FABRIC API", "Fabric API"),
        "Case differences should not trigger false positives"
    );
}

#[test]
fn test_has_extra_words_shorter_result() {
    // Results shorter than query should not be flagged
    assert!(
        !has_extra_words("Just Enough Items", "JEI"),
        "Shorter result (acronym) should not have extra words"
    );
}

#[test]
fn test_has_extra_words_partial_word_match() {
    // Partial word matches should still detect extra words
    assert!(
        has_extra_words("Iron Chest", "Iron Chests Plus"),
        "Should detect extra word even with partial match"
    );
}

// ============================================================================
// Platform Threshold Tests
// ============================================================================

#[test]
fn test_modrinth_threshold() {
    // Modrinth requires 90% confidence
    assert!(
        meets_threshold(0.91, ProjectPlatform::Modrinth),
        "91% should meet Modrinth threshold"
    );
    assert!(
        meets_threshold(0.90, ProjectPlatform::Modrinth),
        "90% should meet Modrinth threshold (boundary)"
    );
    assert!(
        !meets_threshold(0.89, ProjectPlatform::Modrinth),
        "89% should fail Modrinth threshold"
    );
}

#[test]
fn test_curseforge_threshold() {
    // CurseForge requires 85% confidence (lower bar for fallback)
    assert!(
        meets_threshold(0.86, ProjectPlatform::CurseForge),
        "86% should meet CurseForge threshold"
    );
    assert!(
        meets_threshold(0.85, ProjectPlatform::CurseForge),
        "85% should meet CurseForge threshold (boundary)"
    );
    assert!(
        !meets_threshold(0.84, ProjectPlatform::CurseForge),
        "84% should fail CurseForge threshold"
    );
}

#[test]
fn test_threshold_platform_difference() {
    // Same score should pass CurseForge but fail Modrinth
    let score = 0.87;
    assert!(
        meets_threshold(score, ProjectPlatform::CurseForge),
        "87% should pass CurseForge"
    );
    assert!(
        !meets_threshold(score, ProjectPlatform::Modrinth),
        "87% should fail Modrinth"
    );
}

// ============================================================================
// Integration Tests (Combined Validation)
// ============================================================================

#[test]
fn test_apotheosis_ascended_rejection() {
    // Real-world test case from v2: Apotheosis should not match Apotheosis Ascended
    let conf = calculate_confidence(
        "Apotheosis",
        "Apotheosis Ascended",
        "apotheosis-ascended",
        5_000_000,
        50_000_000,
    );

    // Should fail Modrinth threshold
    assert!(
        !meets_threshold(conf.score, ProjectPlatform::Modrinth),
        "Apotheosis vs Apotheosis Ascended should fail Modrinth threshold"
    );

    // Should be rejected by extra words check
    assert!(
        has_extra_words("Apotheosis", "Apotheosis Ascended"),
        "Should detect extra words"
    );
}

#[test]
fn test_jei_exact_match_acceptance() {
    // Real-world test case: JEI exact match should pass
    let conf = calculate_confidence("JEI", "JEI", "jei", 10_000_000, 10_000_000);

    // Should pass Modrinth threshold
    assert!(
        meets_threshold(conf.score, ProjectPlatform::Modrinth),
        "Exact JEI match should pass Modrinth threshold"
    );

    // Should not be flagged as extra words
    assert!(!has_extra_words("JEI", "JEI"), "Exact match has no extra words");
}

#[test]
fn test_citadel_high_confidence() {
    // Real-world test case: Citadel exact match with high downloads
    let conf = calculate_confidence("Citadel", "Citadel", "citadel", 50_000_000, 50_000_000);

    // Should have very high confidence
    assert!(
        conf.score >= 0.95,
        "Citadel exact match should have >=95% confidence"
    );

    // Should pass Modrinth threshold
    assert!(
        meets_threshold(conf.score, ProjectPlatform::Modrinth),
        "Citadel should pass Modrinth threshold"
    );
}

#[test]
fn test_fuzzy_match_debug_fields() {
    // Verify FuzzyMatch contains expected debug fields
    let conf = calculate_confidence("test", "test", "test", 1_000_000, 10_000_000);

    // Should have all components
    assert!(conf.score >= 0.0 && conf.score <= 1.0);
    assert!(conf.string_similarity >= 0.0 && conf.string_similarity <= 1.0);
    assert!(conf.download_confidence >= 0.0 && conf.download_confidence <= 1.0);

    // Components should sum correctly (weighted)
    let expected = (conf.string_similarity * 0.7) + (conf.download_confidence * 0.3);
    assert!(
        (conf.score - expected).abs() < 0.001,
        "Score should match weighted sum of components"
    );
}
