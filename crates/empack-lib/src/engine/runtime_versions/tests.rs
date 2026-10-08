use super::*;

#[test]
fn test_parse_version() {
    assert_eq!(parse_version("1.20"), Some(Version::new(1, 20, 0)));
    assert_eq!(parse_version("1.20.4"), Some(Version::new(1, 20, 4)));
    assert_eq!(parse_version("not-a-version"), None);
    assert!(parse_version("1.20.1") < parse_version("1.20.2"));
}

#[test]
fn test_sort_versions_desc_multi_digit() {
    let mut versions = vec![
        "21.1.7".to_string(),
        "21.1.69".to_string(),
        "21.1.8".to_string(),
    ];

    sort_versions_desc(&mut versions);
    assert_eq!(versions, vec!["21.1.69", "21.1.8", "21.1.7"]);
}

#[test]
fn test_sort_versions_desc_prerelease() {
    let mut versions = vec![
        "21.1.67-beta".to_string(),
        "21.1.67".to_string(),
        "21.1.69".to_string(),
    ];

    sort_versions_desc(&mut versions);
    assert_eq!(versions, vec!["21.1.69", "21.1.67", "21.1.67-beta"]);
}

#[test]
fn test_parse_version_two_component() {
    let v = parse_version("1.21").unwrap();
    assert_eq!(v, Version::new(1, 21, 0));
}

#[test]
fn test_parse_version_three_component() {
    let v = parse_version("1.20.4").unwrap();
    assert_eq!(v, Version::new(1, 20, 4));
}

#[test]
fn test_parse_version_four_component_forge_legacy() {
    let v = parse_version("11.14.4.1577");
    assert!(v.is_some(), "4-component Forge versions should parse");
    let v = v.unwrap();
    assert_eq!(v.major, 11);
    assert_eq!(v.minor, 14);
    assert_eq!(v.patch, 4);
}

#[test]
fn test_canonicalize_forge_loader_version_strips_late_1710_suffix() {
    assert_eq!(
        canonicalize_forge_loader_version("1.7.10", "10.13.4.1614-1.7.10"),
        "10.13.4.1614"
    );
    assert_eq!(
        canonicalize_forge_loader_version("1.7.10", "10.13.2.1291"),
        "10.13.2.1291"
    );
    assert_eq!(
        canonicalize_forge_loader_version("1.20.1", "47.3.0"),
        "47.3.0"
    );
}

#[test]
fn test_uses_legacy_forge_coordinate_switches_at_1710_boundary() {
    assert!(!uses_legacy_forge_coordinate("1.7.10", "10.13.2.1291"));
    assert!(uses_legacy_forge_coordinate("1.7.10", "10.13.2.1300"));
    assert!(uses_legacy_forge_coordinate(
        "1.7.10",
        "10.13.4.1614-1.7.10"
    ));
    assert!(!uses_legacy_forge_coordinate("1.20.1", "47.3.0"));
}

#[test]
fn test_parse_version_returns_none_for_garbage() {
    assert!(parse_version("").is_none());
    assert!(parse_version("abc").is_none());
    assert!(parse_version("...").is_none());
}

#[test]
fn test_parse_version_with_prerelease() {
    let v = parse_version("21.1.67-beta");
    assert!(v.is_some());
    let v = v.unwrap();
    assert_eq!(v.major, 21);
    assert_eq!(v.minor, 1);
    assert_eq!(v.patch, 67);
    assert!(!v.pre.is_empty());
}

#[test]
fn test_sort_versions_desc_empty() {
    let mut versions: Vec<String> = vec![];
    sort_versions_desc(&mut versions);
    assert!(versions.is_empty());
}

#[test]
fn test_sort_versions_desc_single() {
    let mut versions = vec!["1.0.0".to_string()];
    sort_versions_desc(&mut versions);
    assert_eq!(versions, vec!["1.0.0"]);
}

#[test]
fn test_sort_versions_desc_unparseable_sorts_to_end() {
    let mut versions = vec![
        "invalid".to_string(),
        "1.0.0".to_string(),
        "2.0.0".to_string(),
    ];
    sort_versions_desc(&mut versions);
    assert_eq!(versions[0], "2.0.0");
    assert_eq!(versions[1], "1.0.0");
    assert_eq!(versions[2], "invalid");
}

#[test]
fn test_sort_versions_desc_all_unparseable() {
    let mut versions = vec!["zzz".to_string(), "aaa".to_string(), "mmm".to_string()];
    sort_versions_desc(&mut versions);
    assert_eq!(versions, vec!["aaa", "mmm", "zzz"]);
}

#[test]
fn test_sort_versions_desc_two_component() {
    let mut versions = vec!["1.20".to_string(), "1.21".to_string(), "1.19".to_string()];
    sort_versions_desc(&mut versions);
    assert_eq!(versions, vec!["1.21", "1.20", "1.19"]);
}

#[test]
fn test_sort_versions_desc_parseable_before_unparseable() {
    let mut versions = vec!["1.0.0".to_string(), "invalid".to_string()];
    sort_versions_desc(&mut versions);
    assert_eq!(versions, vec!["1.0.0", "invalid"]);
}
