use super::*;

// ---------------------------------------------------------------------------
// UrlKind classifier tests
// ---------------------------------------------------------------------------

#[test]
fn classify_modrinth_modpack_slug_only() {
    let url = "https://modrinth.com/modpack/fabulously-optimized";
    let kind = classify_url(url).unwrap();
    assert_eq!(
        kind,
        UrlKind::ModrinthModpack {
            slug: "fabulously-optimized".to_string(),
            version: None,
        }
    );
}

#[test]
fn classify_modrinth_modpack_with_version() {
    let url = "https://modrinth.com/modpack/fabulously-optimized/version/5.2.0";
    let kind = classify_url(url).unwrap();
    assert_eq!(
        kind,
        UrlKind::ModrinthModpack {
            slug: "fabulously-optimized".to_string(),
            version: Some("5.2.0".to_string()),
        }
    );
}

#[test]
fn classify_modrinth_modpack_with_non_version_segment() {
    let url = "https://modrinth.com/modpack/fabulously-optimized/files";
    let kind = classify_url(url).unwrap();
    assert_eq!(
        kind,
        UrlKind::ModrinthModpack {
            slug: "fabulously-optimized".to_string(),
            version: None,
        }
    );
}

#[test]
fn classify_modrinth_mod() {
    let url = "https://modrinth.com/mod/sodium";
    let kind = classify_url(url).unwrap();
    assert_eq!(
        kind,
        UrlKind::ModrinthProject {
            slug: "sodium".to_string(),
        }
    );
}

#[test]
fn classify_modrinth_plugin() {
    let url = "https://modrinth.com/plugin/essentials";
    let kind = classify_url(url).unwrap();
    assert_eq!(
        kind,
        UrlKind::ModrinthProject {
            slug: "essentials".to_string(),
        }
    );
}

#[test]
fn classify_curseforge_modpack() {
    let url = "https://www.curseforge.com/minecraft/modpacks/all-the-mods-9";
    let kind = classify_url(url).unwrap();
    assert_eq!(
        kind,
        UrlKind::CurseForgeModpack {
            slug: "all-the-mods-9".to_string(),
        }
    );
}

#[test]
fn classify_curseforge_mc_mods() {
    let url = "https://www.curseforge.com/minecraft/mc-mods/journeys";
    let kind = classify_url(url).unwrap();
    assert_eq!(
        kind,
        UrlKind::CurseForgeProject {
            slug: "journeys".to_string(),
        }
    );
}

#[test]
fn classify_direct_jar() {
    let url = "https://example.com/downloads/sodium-0.5.8.jar";
    let kind = classify_url(url).unwrap();
    assert_eq!(
        kind,
        UrlKind::DirectDownload {
            url: url.to_string(),
            extension: "jar".to_string(),
        }
    );
}

#[test]
fn classify_direct_zip() {
    let url = "https://example.com/pack.zip";
    let kind = classify_url(url).unwrap();
    assert_eq!(
        kind,
        UrlKind::DirectDownload {
            url: url.to_string(),
            extension: "zip".to_string(),
        }
    );
}

#[test]
fn classify_direct_jar_with_query_params() {
    let url = "https://cdn.modrinth.com/data/AANobbMI/versions/1.0.0/fabric-api-0.92.0.jar?foo=bar";
    let kind = classify_url(url).unwrap();
    assert_eq!(
        kind,
        UrlKind::DirectDownload {
            url: url.to_string(),
            extension: "jar".to_string(),
        }
    );
}

#[test]
fn classify_unrecognized_url() {
    let url = "https://example.com/some/random/page";
    let err = classify_url(url).unwrap_err();
    assert!(err.to_string().contains(url));
}

#[test]
fn classify_unrecognized_extension() {
    let url = "https://example.com/file.exe";
    let err = classify_url(url).unwrap_err();
    assert!(err.to_string().contains(url));
}

#[test]
fn classify_modrinth_modpack_with_query_string() {
    let url = "https://modrinth.com/modpack/create-above-and-beyond?query=test";
    let kind = classify_url(url).unwrap();
    assert_eq!(
        kind,
        UrlKind::ModrinthModpack {
            slug: "create-above-and-beyond".to_string(),
            version: None,
        }
    );
}

#[test]
fn classify_curseforge_modpack_with_files_path() {
    let url = "https://www.curseforge.com/minecraft/modpacks/all-the-mods-9/files";
    let kind = classify_url(url).unwrap();
    assert_eq!(
        kind,
        UrlKind::CurseForgeModpack {
            slug: "all-the-mods-9".to_string(),
        }
    );
}

#[test]
fn classify_modrinth_modpack_slug_shadows_mod_prefix() {
    // "sodium" is also a valid mod slug; modpack must take priority.
    let url = "https://modrinth.com/modpack/sodium/version/1.0.0";
    let kind = classify_url(url).unwrap();
    assert!(matches!(
        kind,
        UrlKind::ModrinthModpack {
            slug,
            version: Some(ref v),
        } if slug == "sodium" && v == "1.0.0"
    ));
}

#[test]
fn classify_modrinth_project_trailing_slash() {
    let url = "https://modrinth.com/mod/sodium/";
    let kind = classify_url(url).unwrap();
    assert_eq!(
        kind,
        UrlKind::ModrinthProject {
            slug: "sodium".to_string(),
        }
    );
}

#[test]
fn classify_modrinth_modpack_trailing_slash() {
    let url = "https://modrinth.com/modpack/fabulously-optimized/";
    let kind = classify_url(url).unwrap();
    assert_eq!(
        kind,
        UrlKind::ModrinthModpack {
            slug: "fabulously-optimized".to_string(),
            version: None,
        }
    );
}

// ---------------------------------------------------------------------------
// SideEnv / SideRequirement tests
// ---------------------------------------------------------------------------

#[test]
fn side_env_equality() {
    let env1 = SideEnv {
        client: SideRequirement::Required,
        server: SideRequirement::Optional,
    };
    let env2 = SideEnv {
        client: SideRequirement::Required,
        server: SideRequirement::Optional,
    };
    assert_eq!(env1, env2);
}

#[test]
fn side_env_inequality() {
    let env1 = SideEnv {
        client: SideRequirement::Required,
        server: SideRequirement::Unsupported,
    };
    let env2 = SideEnv {
        client: SideRequirement::Required,
        server: SideRequirement::Optional,
    };
    assert_ne!(env1, env2);
}

#[test]
fn side_requirement_variants() {
    let variants = [
        SideRequirement::Required,
        SideRequirement::Optional,
        SideRequirement::Unsupported,
        SideRequirement::Unknown,
    ];
    // Ensure all variants are distinct
    for i in 0..variants.len() {
        for j in (i + 1)..variants.len() {
            assert_ne!(variants[i], variants[j]);
        }
    }
}

// ---------------------------------------------------------------------------
// OverrideSide / OverrideCategory tests
// ---------------------------------------------------------------------------

#[test]
fn override_side_variants_distinct() {
    let variants = [OverrideSide::Both, OverrideSide::ClientOnly, OverrideSide::ServerOnly];
    for i in 0..variants.len() {
        for j in (i + 1)..variants.len() {
            assert_ne!(variants[i], variants[j]);
        }
    }
}

#[test]
fn override_category_variants_distinct() {
    let variants = [
        OverrideCategory::Config,
        OverrideCategory::Script,
        OverrideCategory::ResourcePack,
        OverrideCategory::ShaderPack,
        OverrideCategory::DataPack,
        OverrideCategory::World,
        OverrideCategory::ServerConfig,
        OverrideCategory::ClientConfig,
        OverrideCategory::ModData,
        OverrideCategory::Other,
    ];
    for i in 0..variants.len() {
        for j in (i + 1)..variants.len() {
            assert_ne!(variants[i], variants[j]);
        }
    }
}

// ---------------------------------------------------------------------------
// UrlKind equality tests
// ---------------------------------------------------------------------------

#[test]
fn url_kind_equality() {
    let a = UrlKind::ModrinthProject { slug: "sodium".to_string() };
    let b = UrlKind::ModrinthProject { slug: "sodium".to_string() };
    assert_eq!(a, b);

    let c = UrlKind::ModrinthProject { slug: "lithium".to_string() };
    assert_ne!(a, c);
}

#[test]
fn url_kind_modrinth_modpack_equality() {
    let a = UrlKind::ModrinthModpack {
        slug: "rp".to_string(),
        version: Some("v2".to_string()),
    };
    let b = UrlKind::ModrinthModpack {
        slug: "rp".to_string(),
        version: Some("v2".to_string()),
    };
    assert_eq!(a, b);

    let c = UrlKind::ModrinthModpack {
        slug: "rp".to_string(),
        version: None,
    };
    assert_ne!(a, c);
}
