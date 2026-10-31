//! Real rollback plans interrupted in a child; no product fault configuration.
//! Literal assets establish native publication behavior, not runnable game content.
use super::*;
use crate::engine::{
    content::{InitialObservation, SourceEvidencePolicy, verify_stream},
    publication::PublicationPoint,
    release::{self, ReleaseDocument, ReleaseLayer, ReleaseLoader, ReleaseRuntime},
};
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf, process::Command};

fn input(key: &str, next: bool) -> &'static [u8] {
    match key {
        "mod" if next => b"B",
        "mod" => b"A",
        "config" => b"publisher configuration",
        "level" => b"publisher world template",
        _ => panic!("unknown fixture input"),
    }
}

fn release(next: bool) -> DecodedRelease {
    let files = [
        (
            "mod",
            if next { "mods/b.jar" } else { "mods/a.jar" },
            FilePolicy::Managed,
        ),
        ("config", "config/settings.txt", FilePolicy::Seed),
        ("level", "saves/played/level.dat", FilePolicy::Seed),
    ]
    .into_iter()
    .map(|(key, destination, policy)| {
        let bytes = input(key, next);
        ReleaseFile {
            key: key.into(),
            destination: destination.into(),
            layer: ReleaseLayer::Common,
            policy,
            world_root: (key == "level").then(|| "saves/played".into()),
            client: Participation::Required,
            server: Participation::Required,
            sha256: release::hex(&Sha256::digest(bytes)),
            bytes: bytes.len() as u64,
            readonly: false,
            executable: false,
            assertions: vec![],
            asset: None,
            source: ReleaseSource::Asset {
                path: format!("assets/{key}"),
            },
        }
    })
    .collect();
    DecodedRelease::encode(ReleaseDocument {
        schema: 1,
        pack: "rollback.fixture".into(),
        version: if next { "B" } else { "A" }.into(),
        minimum_engine: ">=0.8.0-beta".into(),
        require_subscription: false,
        server_launch: None,
        runtime: ReleaseRuntime {
            minecraft: "1.21.1".into(),
            loader: ReleaseLoader::Vanilla,
            java_major: 21,
        },
        choices: vec![],
        files,
    })
    .unwrap()
}

fn prepare(root: &Path, state: &Path, next: bool, action: InstanceAction) -> PreparedInstance {
    let decoded = release(next);
    assert_eq!(decoded.id(), release::hex(&Sha256::digest(decoded.bytes())));
    let selected = release::trust::SelectedSnapshot::select(
        decoded.bytes(),
        decoded.id(),
        &semver::Version::parse("0.8.0-beta").unwrap(),
    )
    .unwrap();
    let cancel = Cancellation::default();
    let plan = plan(
        root,
        InstanceSelection {
            require_subscription: false,
            conflicts: vec![],
            release: SelectedRelease::Snapshot(selected),
            side: InstanceSide::Client,
            layout: Some(InstanceLayout::Game),
            choices: vec![],
            action,
        },
        RecoveryReader::new(state.into()),
        SnapshotLimits::default(),
        &cancel,
    )
    .unwrap();
    let supplied = decoded
        .document()
        .files
        .iter()
        .map(|file| {
            let mut bytes = input(&file.key, next);
            (
                file.key.clone(),
                verify_stream(
                    &mut bytes,
                    &file.expected().unwrap(),
                    file.bytes,
                    SourceEvidencePolicy::Compatibility,
                    InitialObservation::RequireEvidence,
                    &cancel,
                )
                .unwrap(),
            )
        })
        .collect();
    plan.stage(&supplied, &BTreeMap::new(), None, &cancel)
        .unwrap()
}

fn files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, here: &Path, result: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(here).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                visit(root, &entry.path(), result);
            } else {
                assert!(entry.file_type().unwrap().is_file());
                result.insert(
                    entry.path().strip_prefix(root).unwrap().into(),
                    fs::read(entry.path()).unwrap(),
                );
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result);
    result
}

fn attributes(path: &Path) -> u32 {
    let permissions = fs::metadata(path).unwrap().permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.mode() & 0o777
    }
    #[cfg(not(unix))]
    {
        u32::from(permissions.readonly())
    }
}

fn assert_installed(root: &Path, state: &Path, next: bool, subscription: &[u8]) {
    let (record, selected) = inspect(
        root,
        None,
        RecoveryReader::new(state.into()),
        SnapshotLimits::default(),
        &Cancellation::default(),
    )
    .unwrap();
    let a = release(false);
    let b = release(true);
    let expected = if next { &b } else { &a };
    let saved = subscription::SubscriptionRecord::decode(subscription).unwrap();
    let expected_record = InstanceRecord {
        schema: 1,
        root: saved.root.clone(),
        pack: "rollback.fixture".into(),
        release: expected.id().into(),
        side: InstanceSide::Client,
        layout: InstanceLayout::Game,
        initialized_worlds: vec!["saves/played".into()],
        choices: vec![],
        history: if next {
            vec![a.id().into()]
        } else {
            vec![a.id().into(), b.id().into()]
        },
        asset_bases: vec![],
        local_overrides: vec![],
    };
    assert_eq!(record, expected_record);
    assert_eq!(selected.bytes(), expected.bytes());
    let expected_files = BTreeMap::from([
        (
            PathBuf::from(".empack/instance.json"),
            serde_json::to_vec(&expected_record).unwrap(),
        ),
        (
            PathBuf::from(format!(".empack/releases/{}.json", a.id())),
            a.bytes().to_vec(),
        ),
        (
            PathBuf::from(format!(".empack/releases/{}.json", b.id())),
            b.bytes().to_vec(),
        ),
        (
            PathBuf::from(".empack/subscription.json"),
            subscription.to_vec(),
        ),
        (
            PathBuf::from("game/.empack-layout"),
            b"empack-game-layout-v1\n".to_vec(),
        ),
        (
            PathBuf::from(if next {
                "game/mods/b.jar"
            } else {
                "game/mods/a.jar"
            }),
            if next { b"B".to_vec() } else { b"A".to_vec() },
        ),
        (
            PathBuf::from("game/config/settings.txt"),
            b"player configuration".to_vec(),
        ),
        (
            PathBuf::from("game/saves/played/level.dat"),
            b"played world".to_vec(),
        ),
        (
            PathBuf::from("game/saves/played/player.dat"),
            b"player-added world member".to_vec(),
        ),
        (
            PathBuf::from("player notes.txt"),
            b"unrelated player notes".to_vec(),
        ),
    ]);
    assert_eq!(files(root), expected_files);
    assert_eq!(record.root, saved.root);
    assert_eq!(saved.floor.as_ref().unwrap().sequence, 7);
}

#[test]
fn rollback_publication_crashes_recover_content_metadata_and_preserve_player_state() {
    use crate::engine::release::trust::{
        ChannelDocument, ChannelRelease, EnvelopeKind, PublisherTrust, sign,
    };
    use ed25519_dalek::SigningKey;
    for phase in [
        "retained",
        "intent",
        "content-target",
        "record-target",
        "record-progress",
        "verified",
        "committed",
    ] {
        for restore in [false, true] {
            if restore && matches!(phase, "retained" | "committed") {
                continue;
            }
            let project = tempfile::tempdir().unwrap();
            let host = tempfile::tempdir().unwrap();
            let state = host.path().join("private");
            let scratch = host.path().join("child scratch");
            fs::create_dir(&scratch).unwrap();
            let root = project.path();
            fs::write(root.join("player notes.txt"), b"unrelated player notes").unwrap();
            let publisher = Publisher::open(&state).unwrap();
            for next in [false, true] {
                prepare(root, &state, next, InstanceAction::Apply)
                    .publish(&publisher, &Cancellation::default())
                    .unwrap();
            }
            fs::write(
                root.join("game/config/settings.txt"),
                b"player configuration",
            )
            .unwrap();
            fs::write(root.join("game/saves/played/level.dat"), b"played world").unwrap();
            fs::write(
                root.join("game/saves/played/player.dat"),
                b"player-added world member",
            )
            .unwrap();
            let native = ProjectReadRoot::open(root).unwrap();
            let key = SigningKey::from_bytes(&[94; 32]);
            let document = ChannelDocument {
                schema: 1,
                pack: "rollback.fixture".into(),
                channel: "stable".into(),
                sequence: 7,
                expires: 2_000_003_600,
                minimum_engine: ">=0.8.0-beta".into(),
                release: ChannelRelease {
                    id: release(true).id().into(),
                    url: "https://publisher.test/releases/B.json".into(),
                    maximum_bytes: 4096,
                },
            };
            let envelope =
                sign(EnvelopeKind::Channel, &document.encode().unwrap(), &[&key]).unwrap();
            let trust = PublisherTrust::enroll(
                "rollback.fixture".into(),
                "https://publisher.test",
                vec![key.verifying_key()],
            )
            .unwrap();
            let floor = trust
                .channel(
                    &envelope,
                    "stable",
                    2_000_000_000,
                    &semver::Version::parse("0.8.0-beta").unwrap(),
                    None,
                )
                .unwrap()
                .floor()
                .clone();
            let subscription = serde_json::to_vec(&subscription::SubscriptionRecord {
                schema: 1,
                root: publisher.bound_root_key(&native).unwrap(),
                pack: "rollback.fixture".into(),
                channel: "stable".into(),
                url: "https://publisher.test/stable.json".into(),
                keys: vec![release::hex(key.verifying_key().as_bytes())],
                floor: Some(floor),
                observed: Some(String::from_utf8(envelope).unwrap()),
            })
            .unwrap();
            fs::write(root.join(".empack/subscription.json"), &subscription).unwrap();
            assert_installed(root, &state, true, &subscription);
            let before_record = fs::read(root.join(".empack/instance.json")).unwrap();
            let player_paths = [
                "player notes.txt",
                "game/config/settings.txt",
                "game/saves/played/level.dat",
                "game/saves/played/player.dat",
            ];
            let before_attributes: Vec<_> = player_paths
                .iter()
                .map(|p| attributes(&root.join(p)))
                .collect();
            drop(native);
            drop(publisher);
            let status = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "engine::instance::publication_tests::rollback_crash_worker",
                    "--nocapture",
                ])
                .env("EMPACK_ROLLBACK_TEST_ROOT", root)
                .env("EMPACK_ROLLBACK_TEST_STATE", &state)
                .env("EMPACK_ROLLBACK_TEST_PHASE", phase)
                .env("TMPDIR", &scratch)
                .env("TMP", &scratch)
                .env("TEMP", &scratch)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::inherit())
                .status()
                .unwrap();
            assert_eq!(status.code(), Some(86), "worker failed before {phase}");
            let native = ProjectReadRoot::open(root).unwrap();
            let publisher = Publisher::open(&state).unwrap();
            let pending = !matches!(phase, "retained" | "committed");
            assert_eq!(publisher.recovery_required(&native).unwrap(), pending);
            if phase == "retained" {
                assert_installed(root, &state, true, &subscription);
                prepare(root, &state, false, InstanceAction::Rollback)
                    .publish(&publisher, &Cancellation::default())
                    .unwrap();
            } else {
                if pending {
                    let error = inspect(
                        root,
                        None,
                        RecoveryReader::new(state.clone()),
                        SnapshotLimits::default(),
                        &Cancellation::default(),
                    )
                    .expect_err("hot rollback must block ordinary inspection");
                    let location = error
                        .downcast_ref::<crate::engine::publication::RecoveryLocation>()
                        .expect("inspection must identify the recovery target");
                    assert_eq!(location.project, native.selected());
                    assert_eq!(location.state, state);
                    assert!(
                        format!("{error:#}").contains(
                            "Project requires publication recovery before ordinary reads"
                        )
                    );
                }
                let receipt = if restore {
                    publisher.restore_before_images(&native).unwrap()
                } else {
                    publisher.recover(&native).unwrap()
                };
                assert_eq!(
                    publisher.recover(&native).unwrap(),
                    receipt,
                    "recovery must be idempotent"
                );
            }
            assert!(!publisher.recovery_required(&native).unwrap());
            assert_installed(root, &state, restore, &subscription);
            if restore {
                assert_eq!(
                    fs::read(root.join(".empack/instance.json")).unwrap(),
                    before_record
                );
            }
            assert_eq!(
                player_paths
                    .iter()
                    .map(|p| attributes(&root.join(p)))
                    .collect::<Vec<_>>(),
                before_attributes
            );
        }
    }
}

#[test]
fn rollback_crash_worker() {
    let Some(root) = std::env::var_os("EMPACK_ROLLBACK_TEST_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let state = PathBuf::from(std::env::var_os("EMPACK_ROLLBACK_TEST_STATE").unwrap());
    let phase = std::env::var("EMPACK_ROLLBACK_TEST_PHASE").unwrap();
    let prepared = prepare(&root, &state, false, InstanceAction::Rollback);
    // Target order selects fault locations; literal inventory and record values
    // above independently define the expected recovered state.
    let changes = prepared.change.plan().changes();
    let content_ordinal = changes
        .iter()
        .position(|change| matches!(change.target(), ManagedPath::InstanceFile(_)))
        .unwrap()
        + 1;
    let record_ordinal = changes
        .iter()
        .position(|change| change.target() == &ManagedPath::InstanceRecord)
        .unwrap()
        + 1;
    assert_ne!(content_ordinal, record_ordinal);
    let (point, ordinal) = match phase.as_str() {
        "retained" => (PublicationPoint::RecoveryDataDurable, 1),
        "intent" => (PublicationPoint::IntentDurable, 1),
        "content-target" => (PublicationPoint::TargetChanged, content_ordinal),
        "record-target" => (PublicationPoint::TargetChanged, record_ordinal),
        "record-progress" => (PublicationPoint::ProgressDurable, record_ordinal),
        "verified" => (PublicationPoint::Verified, 1),
        "committed" => (PublicationPoint::Committed, 1),
        _ => panic!("unknown rollback crash phase"),
    };
    let publisher = Publisher::open(&state).unwrap();
    assert_eq!(
        publisher.bound_root_key(&prepared.root).unwrap(),
        prepared.record.root
    );
    let mut seen = 0;
    publisher
        .publish_test_change(
            &prepared.root,
            prepared.change,
            &Cancellation::default(),
            &mut |at| {
                if at == point {
                    seen += 1;
                    if seen == ordinal {
                        std::process::exit(86);
                    }
                }
                Ok(())
            },
        )
        .unwrap();
    panic!("rollback crash point was not reached");
}
