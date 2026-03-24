use super::*;

#[test]
fn test_calculate_confidence_exact_match() {
    assert_eq!(calculate_confidence("JEI", "JEI", 1000), 100);
    assert_eq!(calculate_confidence("JEI", "jei", 1000), 100);
    assert_eq!(calculate_confidence("Just Enough Items", "Just Enough Items", 1000), 100);
    assert_eq!(calculate_confidence("OptiFine", "OptiFine", 0), 100);
}

#[test]
fn test_calculate_confidence_contains_match() {
    assert_eq!(calculate_confidence("test", "testing", 1000), 90);
    assert_eq!(calculate_confidence("test", "testing", 2000), 90);

    assert_eq!(calculate_confidence("test", "testing", 100), 85);
    assert_eq!(calculate_confidence("test", "testing", 0), 85);

    assert_eq!(calculate_confidence("test", "testing", 1000), 90);
    assert_eq!(calculate_confidence("test", "testing", 100), 85);
}

#[test]
fn test_calculate_confidence_levenshtein_distance() {
    assert!(calculate_confidence("JEI", "JEI Addon", 1000) > 80);
    assert!(calculate_confidence("OptiFine", "Optifine", 1000) > 90);

    assert!(calculate_confidence("JEI", "Biomes O' Plenty", 1000) < 50);
    assert!(calculate_confidence("short", "very long string indeed", 1000) < 60);
}

#[test]
fn test_calculate_confidence_download_boost() {
    let high_downloads = calculate_confidence("test", "testing", 1000);
    let low_downloads = calculate_confidence("test", "testing", 100);

    assert_eq!(high_downloads, low_downloads + 5);
}

#[test]
fn test_calculate_confidence_edge_cases() {
    assert_eq!(calculate_confidence("", "", 1000), 100);

    let empty_query_confidence = calculate_confidence("", "something", 1000);
    assert!(empty_query_confidence <= 100);

    let empty_result_confidence = calculate_confidence("something", "", 1000);
    assert!(empty_result_confidence <= 100);

    let long_query = "a".repeat(100);
    let long_found = "b".repeat(100);
    assert!(calculate_confidence(&long_query, &long_found, 1000) < 10);
}

#[test]
fn test_has_extra_words_normal_cases() {
    assert!(!has_extra_words("Create", "Create"));
    assert!(!has_extra_words("test", "test1"));
    assert!(!has_extra_words("mod", "mods"));
    assert!(!has_extra_words("ab", "abc"));

    assert!(!has_extra_words("RF.Tools", "RFTools"));
    assert!(!has_extra_words("a-b", "ab"));
}

#[test]
fn test_has_extra_words_excessive_expansion() {
    assert!(has_extra_words("JEI", "Just Enough Items Plus Extra Functionality And More"));
    assert!(has_extra_words("RF", "Redstone Flux API Implementation Framework"));
    assert!(has_extra_words("mod", "very long descriptive modification name"));
}

#[test]
fn test_has_extra_words_edge_cases() {
    assert!(!has_extra_words("", "anything"));
    assert!(!has_extra_words("", ""));

    assert!(!has_extra_words("a-b-c", "abc"));
    assert!(!has_extra_words("test", "TEST"));

    assert!(!has_extra_words("ab", "abc"));
}

#[test]
fn test_has_extra_words_normalization() {
    assert!(!has_extra_words("just-enough_items", "Just Enough Items"));
    assert!(!has_extra_words("rf.tools", "RFTools"));
    assert!(!has_extra_words("a.b-c_d", "abcd"));

    assert!(!has_extra_words("JEI", "jei"));
    assert!(!has_extra_words("OptiFine", "optifine"));
}

#[test]
fn test_levenshtein_distance() {
    assert_eq!(levenshtein_distance("", ""), 0);
    assert_eq!(levenshtein_distance("", "a"), 1);
    assert_eq!(levenshtein_distance("a", ""), 1);
    assert_eq!(levenshtein_distance("a", "a"), 0);
    assert_eq!(levenshtein_distance("a", "b"), 1);
    assert_eq!(levenshtein_distance("ab", "ac"), 1);
    assert_eq!(levenshtein_distance("abc", "def"), 3);

    assert_eq!(levenshtein_distance("kitten", "sitting"), 3);
    assert_eq!(levenshtein_distance("saturday", "sunday"), 3);
}

#[test]
fn test_levenshtein_unicode() {
    assert_eq!(levenshtein_distance("café", "cafe"), 1);
}

#[test]
fn test_calculate_confidence_unicode_lowercase_expansion() {
    // Turkish İ (U+0130) lowercases to "i\u{0307}" (2 chars) — max_len must use
    // lowercased strings to avoid distance > max_len underflow.
    let result = calculate_confidence("İ", "i", 0);
    assert!(result <= 100, "confidence must not overflow: got {result}");
}
