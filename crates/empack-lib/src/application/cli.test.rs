
use crate::application::cli::SearchPlatform;

#[test]
fn test_search_platform_enum() {
    // Test that SearchPlatform can be parsed from strings
    assert_eq!(
        "modrinth".parse::<SearchPlatform>().unwrap(),
        SearchPlatform::Modrinth
    );
    assert_eq!(
        "curseforge".parse::<SearchPlatform>().unwrap(),
        SearchPlatform::Curseforge
    );
    assert_eq!(
        "both".parse::<SearchPlatform>().unwrap(),
        SearchPlatform::Both
    );
}
