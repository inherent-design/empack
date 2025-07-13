use super::*;

#[test]
fn test_levenshtein_distance() {
    let resolver = ProjectResolver::new(Client::new(), None);

    assert_eq!(resolver.levenshtein_distance("kitten", "sitting"), 3);
    assert_eq!(resolver.levenshtein_distance("test", "test"), 0);
    assert_eq!(resolver.levenshtein_distance("", "test"), 4);
    assert_eq!(resolver.levenshtein_distance("test", ""), 4);
}

#[test]
fn test_confidence_calculation() {
    let resolver = ProjectResolver::new(Client::new(), None);

    // Exact match
    assert_eq!(resolver.calculate_confidence("JEI", "JEI", 1000), 100);

    // Contains match with high downloads
    assert_eq!(
        resolver.calculate_confidence("JEI", "Just Enough Items (JEI)", 5000),
        90
    );

    // Contains match with low downloads
    assert_eq!(resolver.calculate_confidence("test", "test mod", 500), 85);
}

#[test]
fn test_has_extra_words() {
    let resolver = ProjectResolver::new(Client::new(), None);

    // Cases that should trigger extra words detection (like bash implementation)
    assert!(resolver.has_extra_words("JEI", "Just Enough Items"));
    assert!(resolver.has_extra_words("Apotheosis", "Apotheosis Ascended"));

    // Too many extra words
    assert!(resolver.has_extra_words("test", "test mod with lots of extra words here"));

    // Normal cases that should NOT trigger extra words detection
    assert!(!resolver.has_extra_words("Create", "Create Mod"));
    assert!(!resolver.has_extra_words("Iron Chests", "Iron Chests"));

    // Edge case - empty query
    assert!(!resolver.has_extra_words("", "anything"));
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
