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
fn classify_url_with_trailing_dot_extension() {
    let url = "https://example.com/file.";
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
// URL classifier: new Modrinth path patterns (v0.2)
// ---------------------------------------------------------------------------

#[test]
fn classify_modrinth_resourcepack() {
    let url = "https://modrinth.com/resourcepack/complementary";
    let kind = classify_url(url).unwrap();
    assert_eq!(
        kind,
        UrlKind::ModrinthProject {
            slug: "complementary".to_string(),
        }
    );
}

#[test]
fn classify_modrinth_datapack() {
    let url = "https://modrinth.com/datapack/terralith";
    let kind = classify_url(url).unwrap();
    assert_eq!(
        kind,
        UrlKind::ModrinthProject {
            slug: "terralith".to_string(),
        }
    );
}

#[test]
fn classify_modrinth_shader() {
    let url = "https://modrinth.com/shader/bsl-shaders";
    let kind = classify_url(url).unwrap();
    assert_eq!(
        kind,
        UrlKind::ModrinthProject {
            slug: "bsl-shaders".to_string(),
        }
    );
}

// ---------------------------------------------------------------------------
// API contract tests: Modrinth version-file response
// ---------------------------------------------------------------------------

#[test]
fn contract_modrinth_version_file_deserialization() {
    #[derive(serde::Deserialize)]
    struct ModrinthVersionFile {
        project_id: String,
        #[serde(rename = "id")]
        version_id: String,
        name: String,
    }

    let cassette_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../empack-tests/fixtures/cassettes/modrinth/version_file_sha1.json");
    let content = std::fs::read_to_string(&cassette_path)
        .expect("version_file_sha1.json cassette missing");
    let cassette: serde_json::Value = serde_json::from_str(&content).unwrap();
    let body = &cassette["response"]["body"];

    let parsed: ModrinthVersionFile = serde_json::from_value(body.clone()).unwrap();
    assert_eq!(parsed.project_id, "AANobbMI");
    assert!(!parsed.version_id.is_empty());
    assert!(parsed.name.contains("Sodium"));
}

// ---------------------------------------------------------------------------
// API contract tests: CurseForge fingerprint response
// ---------------------------------------------------------------------------

#[test]
fn contract_curseforge_fingerprint_match_deserialization() {
    #[derive(serde::Deserialize)]
    struct DataEnvelope { data: FingerprintData }

    #[derive(serde::Deserialize)]
    struct FingerprintData {
        #[serde(rename = "exactMatches", default)]
        exact_matches: Vec<ExactMatch>,
    }

    #[derive(serde::Deserialize)]
    struct ExactMatch { file: ExactMatchFile }

    #[derive(serde::Deserialize)]
    struct ExactMatchFile {
        id: u64,
        #[serde(rename = "modId")]
        mod_id: u64,
        #[serde(rename = "displayName")]
        display_name: String,
    }

    let cassette_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../empack-tests/fixtures/cassettes/curseforge/fingerprint_match.json");
    let content = std::fs::read_to_string(&cassette_path)
        .expect("fingerprint_match.json cassette missing");
    let cassette: serde_json::Value = serde_json::from_str(&content).unwrap();
    let body = &cassette["response"]["body"];

    let parsed: DataEnvelope = serde_json::from_value(body.clone()).unwrap();
    assert!(!parsed.data.exact_matches.is_empty());

    let m = &parsed.data.exact_matches[0];
    assert_eq!(m.file.mod_id, 238222, "modId should be JEI project ID");
    assert!(m.file.id > 0, "file.id should be a valid file ID");
    assert_ne!(m.file.id, m.file.mod_id, "file.id must differ from modId");
    assert!(!m.file.display_name.is_empty());
}

#[test]
fn contract_curseforge_fingerprint_miss_deserialization() {
    #[derive(serde::Deserialize)]
    struct DataEnvelope { data: FingerprintData }

    #[derive(serde::Deserialize)]
    struct FingerprintData {
        #[serde(rename = "exactMatches", default)]
        exact_matches: Vec<serde_json::Value>,
        #[serde(rename = "installedFingerprints", default)]
        installed: Vec<u64>,
    }

    let cassette_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../empack-tests/fixtures/cassettes/curseforge/fingerprint_miss.json");
    let content = std::fs::read_to_string(&cassette_path)
        .expect("fingerprint_miss.json cassette missing");
    let cassette: serde_json::Value = serde_json::from_str(&content).unwrap();
    let body = &cassette["response"]["body"];

    let parsed: DataEnvelope = serde_json::from_value(body.clone()).unwrap();
    assert!(parsed.data.exact_matches.is_empty());
    assert!(parsed.data.installed.contains(&999999999));
}

// ---------------------------------------------------------------------------
// API contract tests: CurseForge mod response
// ---------------------------------------------------------------------------

#[test]
fn contract_curseforge_mod_deserialization() {
    #[derive(serde::Deserialize)]
    struct DataEnvelope { data: CfMod }

    #[derive(serde::Deserialize)]
    struct CfMod {
        name: String,
        #[serde(rename = "classId", default)]
        class_id: Option<u32>,
    }

    let cassette_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../empack-tests/fixtures/cassettes/curseforge/mod_238222.json");
    let content = std::fs::read_to_string(&cassette_path)
        .expect("mod_238222.json cassette missing");
    let cassette: serde_json::Value = serde_json::from_str(&content).unwrap();
    let body = &cassette["response"]["body"];

    let parsed: DataEnvelope = serde_json::from_value(body.clone()).unwrap();
    assert_eq!(parsed.data.name, "Just Enough Items (JEI)");
    assert_eq!(parsed.data.class_id, Some(6));
}

// ---------------------------------------------------------------------------
// API contract tests: CurseForge class taxonomy
// ---------------------------------------------------------------------------

#[test]
fn contract_curseforge_class_taxonomy() {
    let cassette_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../empack-tests/fixtures/cassettes/curseforge/categories_minecraft.json");
    let content = std::fs::read_to_string(&cassette_path)
        .expect("categories_minecraft.json cassette missing");
    let cassette: serde_json::Value = serde_json::from_str(&content).unwrap();

    let classes = cassette["response"]["body"]["data"]
        .as_array()
        .expect("data should be array");

    let class_map: std::collections::HashMap<u32, String> = classes
        .iter()
        .filter(|c| c["isClass"].as_bool() == Some(true))
        .map(|c| (c["id"].as_u64().unwrap() as u32, c["name"].as_str().unwrap().to_string()))
        .collect();

    assert_eq!(class_map.get(&6).map(|s| s.as_str()), Some("Mods"));
    assert_eq!(class_map.get(&5).map(|s| s.as_str()), Some("Bukkit Plugins"));
    assert_eq!(class_map.get(&12).map(|s| s.as_str()), Some("Resource Packs"));
    assert_eq!(class_map.get(&17).map(|s| s.as_str()), Some("Worlds"));
    assert_eq!(class_map.get(&6552).map(|s| s.as_str()), Some("Shaders"));
    assert_eq!(class_map.get(&6945).map(|s| s.as_str()), Some("Data Packs"));
    assert_eq!(class_map.get(&4471).map(|s| s.as_str()), Some("Modpacks"));
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

// ---------------------------------------------------------------------------
// hex::encode tests
// ---------------------------------------------------------------------------

#[test]
fn hex_encode_empty() {
    assert_eq!(hex::encode([] as [u8; 0]), "");
}

#[test]
fn hex_encode_known_bytes() {
    assert_eq!(hex::encode([0x00, 0xff, 0xab, 0xcd]), "00ffabcd");
}

#[test]
fn hex_encode_all_zeros() {
    assert_eq!(hex::encode([0x00, 0x00, 0x00]), "000000");
}

#[test]
fn hex_encode_all_ones() {
    assert_eq!(hex::encode([0xff, 0xff]), "ffff");
}

#[test]
fn hex_encode_single_byte() {
    assert_eq!(hex::encode([0x42]), "42");
}

// ---------------------------------------------------------------------------
// compute_sha1_hex tests
// ---------------------------------------------------------------------------

#[test]
fn sha1_of_empty_data() {
    let hash = compute_sha1_hex(b"");
    assert_eq!(hash, "da39a3ee5e6b4b0d3255bfef95601890afd80709");
}

#[test]
fn sha1_of_hello_world() {
    let hash = compute_sha1_hex(b"hello world");
    assert_eq!(hash, "2aae6c35c94fcfb415dbe95f408b9ce91ee846ed");
}

#[test]
fn sha1_has_correct_length() {
    let hash = compute_sha1_hex(b"test data");
    assert_eq!(hash.len(), 40, "SHA-1 hex should be 40 characters");
}

// ---------------------------------------------------------------------------
// URL classifier: additional edge cases
// ---------------------------------------------------------------------------

#[test]
fn classify_modrinth_modpack_version_with_query_string() {
    let url = "https://modrinth.com/modpack/pack-name/version/1.0.0?utm_source=app";
    let kind = classify_url(url).unwrap();
    assert_eq!(
        kind,
        UrlKind::ModrinthModpack {
            slug: "pack-name".to_string(),
            version: Some("1.0.0".to_string()),
        }
    );
}

#[test]
fn classify_modrinth_modpack_version_with_fragment() {
    let url = "https://modrinth.com/modpack/pack-name/version/2.0.0#changelog";
    let kind = classify_url(url).unwrap();
    assert_eq!(
        kind,
        UrlKind::ModrinthModpack {
            slug: "pack-name".to_string(),
            version: Some("2.0.0".to_string()),
        }
    );
}

#[test]
fn classify_curseforge_project_trailing_slash() {
    let url = "https://www.curseforge.com/minecraft/mc-mods/jei/";
    let kind = classify_url(url).unwrap();
    assert_eq!(
        kind,
        UrlKind::CurseForgeProject {
            slug: "jei".to_string(),
        }
    );
}

#[test]
fn classify_curseforge_modpack_trailing_slash() {
    let url = "https://www.curseforge.com/minecraft/modpacks/rlcraft/";
    let kind = classify_url(url).unwrap();
    assert_eq!(
        kind,
        UrlKind::CurseForgeModpack {
            slug: "rlcraft".to_string(),
        }
    );
}

#[test]
fn classify_jar_url_with_uppercase_extension() {
    let url = "https://example.com/downloads/Mod-1.0.JAR";
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
fn classify_url_without_extension() {
    let url = "https://example.com/random/page";
    let err = classify_url(url).unwrap_err();
    assert!(err.to_string().contains("unrecognized"));
}

#[test]
fn classify_url_with_unsupported_extension_txt() {
    let url = "https://example.com/readme.txt";
    let kind = classify_url(url).unwrap();
    assert_eq!(
        kind,
        UrlKind::DirectDownload {
            url: url.to_string(),
            extension: "txt".to_string(),
        }
    );
}

#[test]
fn classify_modrinth_mod_with_fragment() {
    let url = "https://modrinth.com/mod/sodium#description";
    let kind = classify_url(url).unwrap();
    assert_eq!(
        kind,
        UrlKind::ModrinthProject {
            slug: "sodium".to_string(),
        }
    );
}

// ---------------------------------------------------------------------------
// split_slug_version edge cases
// ---------------------------------------------------------------------------

#[test]
fn split_slug_version_empty_segment() {
    let (slug, version) = split_slug_version("");
    assert_eq!(slug, "");
    assert_eq!(version, None);
}

#[test]
fn split_slug_version_slug_only() {
    let (slug, version) = split_slug_version("my-pack");
    assert_eq!(slug, "my-pack");
    assert_eq!(version, None);
}

#[test]
fn split_slug_version_with_version_segment() {
    let (slug, version) = split_slug_version("my-pack/version/1.2.3");
    assert_eq!(slug, "my-pack");
    assert_eq!(version, Some("1.2.3".to_string()));
}

#[test]
fn split_slug_version_version_segment_empty_version() {
    let (slug, version) = split_slug_version("my-pack/version/");
    // Empty version after "version/" causes splitn to produce ["my-pack", "version", ""]
    // The guard !version.is_empty() fails, so the whole string is returned as slug
    assert_eq!(slug, "my-pack/version/");
    assert_eq!(version, None);
}

#[test]
fn split_slug_version_non_version_segment() {
    let (slug, version) = split_slug_version("my-pack/files");
    assert_eq!(slug, "my-pack");
    assert_eq!(version, None);
}

// ---------------------------------------------------------------------------
// UrlKind variant coverage
// ---------------------------------------------------------------------------

#[test]
fn url_kind_direct_download_equality() {
    let a = UrlKind::DirectDownload {
        url: "https://a.com/f.jar".to_string(),
        extension: "jar".to_string(),
    };
    let b = UrlKind::DirectDownload {
        url: "https://a.com/f.jar".to_string(),
        extension: "jar".to_string(),
    };
    assert_eq!(a, b);

    let c = UrlKind::DirectDownload {
        url: "https://b.com/f.jar".to_string(),
        extension: "jar".to_string(),
    };
    assert_ne!(a, c);
}

#[test]
fn url_kind_curseforge_project_equality() {
    let a = UrlKind::CurseForgeProject { slug: "jei".to_string() };
    let b = UrlKind::CurseForgeProject { slug: "jei".to_string() };
    assert_eq!(a, b);

    let c = UrlKind::CurseForgeProject { slug: "rei".to_string() };
    assert_ne!(a, c);
}

#[test]
fn url_kind_curseforge_modpack_equality() {
    let a = UrlKind::CurseForgeModpack { slug: "atm9".to_string() };
    let b = UrlKind::CurseForgeModpack { slug: "atm9".to_string() };
    assert_eq!(a, b);
}

// ---------------------------------------------------------------------------
// JarIdentity coverage
// ---------------------------------------------------------------------------

#[test]
fn jar_identity_modrinth_equality() {
    let a = JarIdentity::Modrinth {
        project_id: "AANobbMI".to_string(),
        version_id: "v1".to_string(),
        title: "Sodium".to_string(),
    };
    let b = JarIdentity::Modrinth {
        project_id: "AANobbMI".to_string(),
        version_id: "v1".to_string(),
        title: "Sodium".to_string(),
    };
    assert_eq!(a, b);
}

#[test]
fn jar_identity_curseforge_equality() {
    let a = JarIdentity::CurseForge {
        project_id: 238222,
        file_id: 5678,
        title: "JEI".to_string(),
    };
    let b = JarIdentity::CurseForge {
        project_id: 238222,
        file_id: 5678,
        title: "JEI".to_string(),
    };
    assert_eq!(a, b);
}

#[test]
fn jar_identity_unidentified() {
    assert_eq!(JarIdentity::Unidentified, JarIdentity::Unidentified);
}

#[test]
fn jar_identity_variants_are_distinct() {
    let modrinth = JarIdentity::Modrinth {
        project_id: "id".to_string(),
        version_id: "v".to_string(),
        title: "t".to_string(),
    };
    let curseforge = JarIdentity::CurseForge {
        project_id: 1,
        file_id: 2,
        title: "t".to_string(),
    };
    let unidentified = JarIdentity::Unidentified;

    assert_ne!(modrinth, curseforge);
    assert_ne!(modrinth, unidentified);
    assert_ne!(curseforge, unidentified);
}

// ---------------------------------------------------------------------------
// UrlClassifyError coverage
// ---------------------------------------------------------------------------

#[test]
fn url_classify_error_display() {
    let err = UrlClassifyError::Unrecognized("https://bad.url".to_string());
    let msg = format!("{}", err);
    assert!(msg.contains("unrecognized URL"));
    assert!(msg.contains("https://bad.url"));
}

#[test]
fn url_classify_error_debug() {
    let err = UrlClassifyError::Unrecognized("test".to_string());
    let debug = format!("{:?}", err);
    assert!(debug.contains("Unrecognized"));
}
