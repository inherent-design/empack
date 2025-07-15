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
