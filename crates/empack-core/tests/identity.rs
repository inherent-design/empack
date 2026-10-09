use empack_core::identity::*;

#[test]
fn provider_ids_do_not_accept_selectors_or_normalize_input() {
    for value in [
        "",
        "sodium",
        " AANobbMI",
        "AANobbMI ",
        "modrinth:AANobbMI",
        "https://modrinth.com/mod/sodium",
        "AANobbM_",
        "AANobbM１",
    ] {
        assert!(ModrinthProjectId::parse(value).is_err(), "{value}");
        assert!(ModrinthVersionId::parse(value).is_err(), "{value}");
    }
    let upper = ModrinthProjectId::parse("AANobbMI").unwrap();
    let lower = ModrinthProjectId::parse("aanobbmi").unwrap();
    assert_ne!(upper, lower);
    assert_eq!(upper.as_str(), "AANobbMI");
    for value in [
        "",
        "0",
        "01",
        "+1",
        "-1",
        " 1",
        "1 ",
        "1.0",
        "1e2",
        "18446744073709551616",
    ] {
        assert!(CurseForgeProjectId::parse(value).is_err(), "{value}");
        assert!(CurseForgeFileId::parse(value).is_err(), "{value}");
    }
    assert_eq!(CurseForgeProjectId::parse("238222").unwrap().get(), 238222);
    assert_eq!(
        CurseForgeFileId::parse("18446744073709551615")
            .unwrap()
            .get(),
        u64::MAX
    );
}

#[test]
fn equal_spelling_in_different_providers_is_not_equal_identity() {
    let mr = ProviderProjectId::Modrinth(ModrinthProjectId::parse("12345678").unwrap());
    let cf = ProviderProjectId::CurseForge(CurseForgeProjectId::parse("12345678").unwrap());
    assert_ne!(mr, cf);
}
