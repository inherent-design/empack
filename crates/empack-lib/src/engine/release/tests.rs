use super::trust::*;
use super::*;
use ed25519_dalek::SigningKey;

pub(super) fn document() -> ReleaseDocument {
    ReleaseDocument {
        require_subscription: false,
        server_launch: None,
        schema: 1,
        pack: "test-pack".into(),
        version: "1.0".into(),
        minimum_engine: ">=0.6.0-beta".into(),
        runtime: ReleaseRuntime {
            minecraft: "1.21.1".into(),
            loader: ReleaseLoader::NeoForge {
                version: "21.1.200".into(),
            },
            java_major: 21,
        },
        choices: vec![],
        files: vec![ReleaseFile {
            key: "config".into(),
            destination: "config/example.toml".into(),
            layer: ReleaseLayer::Common,
            policy: FilePolicy::Seed,
            client: Participation::Required,
            server: Participation::Required,
            sha256: hash(b"example"),
            bytes: 7,
            readonly: false,
            executable: false,
            assertions: vec![],
            asset: None,
            source: ReleaseSource::Asset {
                path: "assets/config".into(),
            },
        }],
    }
}
fn version() -> semver::Version {
    semver::Version::parse("0.6.0-beta").unwrap()
}
fn key() -> SigningKey {
    SigningKey::from_bytes(&[7; 32])
}
fn trust() -> PublisherTrust {
    PublisherTrust::enroll(
        "test-pack".into(),
        "https://example.com",
        vec![key().verifying_key()],
    )
    .unwrap()
}
fn channel(release: &DecodedRelease) -> ChannelDocument {
    ChannelDocument {
        schema: 1,
        pack: "test-pack".into(),
        channel: "stable".into(),
        sequence: 3,
        expires: 1_800_000_100,
        release: ChannelRelease {
            id: release.id().into(),
            url: "https://example.com/release.json".into(),
            maximum_bytes: 16384,
        },
        minimum_engine: ">=0.6.0-beta".into(),
    }
}
#[test]
fn exact_payload_identity_is_distinct_from_semantic_equality() {
    let release = DecodedRelease::encode(document()).unwrap();
    let pretty = serde_json::to_vec_pretty(release.document()).unwrap();
    let alternative = DecodedRelease::decode(&pretty).unwrap();
    assert_eq!(release.document(), alternative.document());
    assert_ne!(release.id(), alternative.id());
    assert!(SelectedSnapshot::select(&pretty, release.id(), &version()).is_err());
    assert!(SelectedSnapshot::select(release.bytes(), release.id(), &version()).is_ok());
    assert!(
        SelectedSnapshot::select(
            release.bytes(),
            release.id(),
            &semver::Version::new(0, 5, 0)
        )
        .is_err()
    );
}
#[test]
fn strict_decoding_rejects_duplicates_unknown_fields_and_future_schemas() {
    let bytes = DecodedRelease::encode(document()).unwrap().bytes;
    let text = String::from_utf8(bytes).unwrap();
    for malformed in [
        text.replacen("\"schema\":1", "\"schema\":1,\"schema\":1", 1),
        text.replacen("\"schema\":1", "\"schema\":2", 1),
        text.replacen("\"schema\":1", "\"schema\":1,\"surprise\":true", 1),
        text.replacen(
            "\"java_major\":21",
            "\"java_major\":21,\"java_major\":21",
            1,
        ),
    ] {
        assert!(DecodedRelease::decode(malformed.as_bytes()).is_err());
    }
    assert!(DecodedRelease::decode(&vec![b' '; MAX_RELEASE_BYTES + 1]).is_err());
}
#[test]
fn unsafe_paths_world_ownership_and_unbound_choices_fail_validation() {
    for path in [
        "../outside",
        "/absolute",
        ".empack/instance.json",
        ".EMPACK/keys",
        "empack.lock",
    ] {
        let mut doc = document();
        doc.files[0].destination = path.into();
        assert!(DecodedRelease::encode(doc).is_err(), "{path}");
    }
    let mut doc = document();
    doc.files[0].destination = "world/level.dat".into();
    doc.files[0].policy = FilePolicy::Managed;
    assert!(DecodedRelease::encode(doc).is_err());
    let mut doc = document();
    doc.files[0].client = Participation::Choice {
        key: "missing".into(),
        value: "enabled".into(),
    };
    assert!(DecodedRelease::encode(doc).is_err());
}
#[test]
fn every_original_digest_remains_an_acquisition_obligation() {
    let mut file = document().files.remove(0);
    file.assertions.push(SourceDigest {
        algorithm: "md5".into(),
        value: "00".repeat(16),
    });
    let expected = file.expected().unwrap();
    assert_eq!(expected.digests.unwrap().values().len(), 1);
    assert!(expected.accepted_observation.is_some());
    file.assertions.push(SourceDigest {
        algorithm: "sha256".into(),
        value: "00".repeat(32),
    });
    assert!(file.expected().is_err());
}
#[test]
fn signature_identity_key_and_context_are_independent_requirements() {
    let release = DecodedRelease::encode(document()).unwrap();
    let signed = sign(EnvelopeKind::Release, release.bytes(), &[&key()]).unwrap();
    assert_eq!(
        trust()
            .release(&signed, release.id(), &version())
            .unwrap()
            .release()
            .id(),
        release.id()
    );
    let other = PublisherTrust::enroll(
        "test-pack".into(),
        "https://example.com",
        vec![SigningKey::from_bytes(&[8; 32]).verifying_key()],
    )
    .unwrap();
    assert!(other.release(&signed, release.id(), &version()).is_err());
    let other_pack = PublisherTrust::enroll(
        "another-pack".into(),
        "https://example.com",
        vec![key().verifying_key()],
    )
    .unwrap();
    assert!(
        other_pack
            .release(&signed, release.id(), &version())
            .is_err()
    );
    assert!(
        trust()
            .release(&signed, &"00".repeat(32), &version())
            .is_err()
    );
    let mut envelope: serde_json::Value = serde_json::from_slice(&signed).unwrap();
    envelope["payload"] = hex(b"tampered").into();
    assert!(
        trust()
            .release(
                &serde_json::to_vec(&envelope).unwrap(),
                release.id(),
                &version()
            )
            .is_err()
    );
    let chan = channel(&release);
    let signed_channel = sign(EnvelopeKind::Channel, &chan.encode().unwrap(), &[&key()]).unwrap();
    assert!(
        trust()
            .release(&signed_channel, release.id(), &version())
            .is_err()
    );
}
#[test]
fn channel_floor_prevents_replay_and_equivocation_without_blocking_retries() {
    let release = DecodedRelease::encode(document()).unwrap();
    let mut doc = channel(&release);
    let encode = |doc: &ChannelDocument| {
        sign(EnvelopeKind::Channel, &doc.encode().unwrap(), &[&key()]).unwrap()
    };
    let first = trust()
        .channel(&encode(&doc), "stable", 1_800_000_000, &version(), None)
        .unwrap();
    assert!(
        trust()
            .channel(
                &encode(&doc),
                "stable",
                1_800_000_001,
                &version(),
                Some(first.floor())
            )
            .is_ok()
    );
    doc.sequence = 2;
    assert!(
        trust()
            .channel(
                &encode(&doc),
                "stable",
                1_800_000_000,
                &version(),
                Some(first.floor())
            )
            .is_err()
    );
    doc.sequence = 3;
    doc.expires += 1;
    assert!(
        trust()
            .channel(
                &encode(&doc),
                "stable",
                1_800_000_000,
                &version(),
                Some(first.floor())
            )
            .is_err()
    );
    doc.sequence = 4;
    assert!(
        trust()
            .channel(
                &encode(&doc),
                "stable",
                1_800_000_000,
                &version(),
                Some(first.floor())
            )
            .is_ok()
    );
    let signed = sign(EnvelopeKind::Release, release.bytes(), &[&key()]).unwrap();
    assert!(first.release(&trust(), &signed, &version()).is_ok());
}
#[test]
fn channel_rejects_expiry_cross_origin_wrong_subscription_and_bound_overflow() {
    let release = DecodedRelease::encode(document()).unwrap();
    let encode = |doc: &ChannelDocument| {
        sign(EnvelopeKind::Channel, &doc.encode().unwrap(), &[&key()]).unwrap()
    };
    let mut doc = channel(&release);
    assert!(
        trust()
            .channel(&encode(&doc), "stable", doc.expires, &version(), None)
            .is_err()
    );
    assert!(
        trust()
            .channel(&encode(&doc), "other", 1_800_000_000, &version(), None)
            .is_err()
    );
    assert!(
        trust()
            .channel(&encode(&doc), "stable", 0, &version(), None)
            .is_err()
    );
    doc.release.url = "https://other.example/release".into();
    assert!(
        trust()
            .channel(&encode(&doc), "stable", 1_800_000_000, &version(), None)
            .is_err()
    );
    doc.release.url = "https://example.com/release".into();
    doc.release.maximum_bytes = 1;
    let checked = trust()
        .channel(&encode(&doc), "stable", 1_800_000_000, &version(), None)
        .unwrap();
    let signed = sign(EnvelopeKind::Release, release.bytes(), &[&key()]).unwrap();
    assert!(checked.release(&trust(), &signed, &version()).is_err());
}
#[test]
fn unknown_algorithms_duplicate_signatures_and_ambiguous_encodings_are_rejected() {
    let release = DecodedRelease::encode(document()).unwrap();
    let signed = sign(EnvelopeKind::Release, release.bytes(), &[&key()]).unwrap();
    let original: serde_json::Value = serde_json::from_slice(&signed).unwrap();
    for change in 0..4 {
        let mut envelope = original.clone();
        match change {
            0 => envelope["signatures"][0]["algorithm"] = "none".into(),
            1 => {
                let value = envelope["signatures"][0].clone();
                envelope["signatures"].as_array_mut().unwrap().push(value);
            }
            2 => envelope["payload"] = envelope["payload"].as_str().unwrap().to_uppercase().into(),
            _ => envelope["signatures"][0]["signature"] = "00".repeat(64).into(),
        }
        assert!(
            trust()
                .release(
                    &serde_json::to_vec(&envelope).unwrap(),
                    release.id(),
                    &version()
                )
                .is_err()
        );
    }
}

#[test]
fn release_wire_spellings_match_native_provider_and_loader_names() {
    let mut document = document();
    document.files[0].source = ReleaseSource::Provider {
        provider: ReleaseProvider::CurseForge,
        project: "12".into(),
        selection: "34".into(),
        slot: "primary".into(),
        alternatives: vec![],
    };
    let encoded = DecodedRelease::encode(document).unwrap();
    let json: serde_json::Value = serde_json::from_slice(encoded.bytes()).unwrap();
    assert_eq!(json["runtime"]["loader"]["kind"], "neoforge");
    assert_eq!(json["files"][0]["source"]["provider"], "curseforge");
}

#[test]
fn release_choices_preserve_author_labels_and_descriptions() {
    let mut doc = document();
    doc.choices = vec![ReleaseChoice {
        key: "Fancy particles".into(),
        alternatives: vec!["enabled".into(), "disabled".into()],
        default: "enabled".into(),
        description: Some("Extra particles\nMay reduce performance".into()),
    }];
    doc.files[0].client = Participation::Choice {
        key: "Fancy particles".into(),
        value: "enabled".into(),
    };
    let encoded = DecodedRelease::encode(doc.clone()).unwrap();
    assert_eq!(encoded.document().choices[0].key, doc.choices[0].key);
    assert_eq!(
        encoded.document().choices[0].description,
        doc.choices[0].description
    );
}

#[test]
fn release_writer_bounds_escaped_output_before_serialization_finishes() {
    let value = "\\".repeat(128);
    assert!(bounded_json(&value, 128).is_err());
    let bytes = bounded_json(&value, 258).unwrap();
    assert_eq!(serde_json::from_slice::<String>(&bytes).unwrap(), value);
}

#[test]
fn provider_selections_require_canonical_project_and_version_identities() {
    for (provider, project, selection) in [
        (ReleaseProvider::Modrinth, "sodium", "abcdefgh"),
        (ReleaseProvider::Modrinth, "AANobbMI", "latest"),
        (ReleaseProvider::CurseForge, "0123", "456"),
        (ReleaseProvider::CurseForge, "123", "+456"),
    ] {
        let mut release = document();
        release.files[0].source = ReleaseSource::Provider {
            provider,
            project: project.into(),
            selection: selection.into(),
            slot: "primary".into(),
            alternatives: vec![],
        };
        assert!(DecodedRelease::encode(release).is_err());
    }
}

#[test]
fn server_entry_point_requires_exact_managed_required_content() {
    let mut payload = document();
    payload.server_launch = Some(ReleaseServerLaunch::Jar {
        path: "server.jar".into(),
    });
    assert!(DecodedRelease::encode(payload.clone()).is_err());
    payload.files[0].destination = "server.jar".into();
    assert!(DecodedRelease::encode(payload.clone()).is_err()); // seed cannot authorize a runtime
    payload.files[0].policy = FilePolicy::Managed;
    let valid = DecodedRelease::encode(payload.clone()).unwrap();
    assert_eq!(
        valid
            .document()
            .server_launch
            .as_ref()
            .unwrap()
            .arguments(false),
        vec![std::ffi::OsString::from("-jar"), "server.jar".into()]
    );
    payload.files[0].server = Participation::Unsupported;
    assert!(DecodedRelease::encode(payload.clone()).is_err());
    payload.files[0].server = Participation::Required;
    let mut shadow = payload.files[0].clone();
    shadow.key = "shadow".into();
    shadow.layer = ReleaseLayer::Server;
    shadow.client = Participation::Unsupported;
    payload.files.push(shadow);
    assert!(DecodedRelease::encode(payload).is_err());
}
