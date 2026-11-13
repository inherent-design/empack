use super::*;

#[test]
fn java_properties_reject_ambiguous_missing_or_malformed_versions() {
    assert_eq!(
        java_major("", "    java.specification.version = 1.8\n").unwrap(),
        8
    );
    assert_eq!(
        java_major("java.specification.version = 21\n", "").unwrap(),
        21
    );
    for (out, err) in [
        ("", "java.version = 21"),
        (
            "java.specification.version = 21",
            "java.specification.version = 17",
        ),
        ("", "java.specification.version = 21.0.1"),
        ("", "java.specification.version = +21"),
    ] {
        assert!(java_major(out, err).is_err());
    }
}
