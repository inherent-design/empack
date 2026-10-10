//! Native dispatch tests assert durable outcomes, preview and selected filesystem authority.
use super::*;
use crate::{
    application::{
        BuildArgs, InitArgs,
        session_mocks::{
            MockCommandSession, MockConfigProvider, MockInteractiveProvider, MockInvocationProvider,
        },
    },
    engine::documents::DocumentCodec,
};
use std::{
    fs,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
};

fn session(root: &Path, yes: bool, dry: bool) -> MockCommandSession {
    MockCommandSession::new()
        .with_invocation(MockInvocationProvider::new().with_current_dir(root.into()))
        .with_config(MockConfigProvider::new(crate::application::AppConfig {
            workdir: Some("project".into()),
            state_dir: Some("state".into()),
            yes,
            dry_run: dry,
            curseforge_api_client_key: None,
            ..Default::default()
        }))
        .with_interactive(MockInteractiveProvider::new().with_confirm(yes))
}
fn init() -> Commands {
    Commands::Init(InitArgs {
        modloader: Some("none".into()),
        mc_version: Some("1.21.1".into()),
        pack_name: Some("Native Pack".into()),
        pack_version: Some("1".into()),
        author: Some("Test".into()),
        ..Default::default()
    })
}
fn add(name: &str) -> Commands {
    Commands::Add {
        mods: vec![name.into()],
        force: false,
        platform: None,
        project_type: None,
        version_id: None,
        file_id: None,
        file_plan: None,
        download_as_local: false,
        continue_independent: false,
    }
}
fn remove(name: &str) -> Commands {
    Commands::Remove {
        mods: vec![name.into()],
        deps: false,
        forget: false,
        acknowledge_unknown: false,
    }
}
fn jar(root: &Path) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(br#"{"schemaVersion":1,"id":"fixture","version":"1"}"#)
        .unwrap();
    let bytes = zip.finish().unwrap().into_inner();
    fs::write(root.join("fixture.jar"), &bytes).unwrap();
    bytes
}
fn snapshot(root: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    crate::application::engine_host::tests::snapshot(root)
}
fn resolved(root: &Path) -> empack_core::model::ResolvedProject {
    let intent = DocumentCodec
        .decode_intent(&fs::read(root.join("project/empack.yml")).unwrap(), "test")
        .unwrap();
    DocumentCodec
        .decode_lock(
            &fs::read(root.join("project/empack.lock")).unwrap(),
            &intent,
            "test",
        )
        .unwrap()
}
#[tokio::test]
async fn dispatch_initializes_adds_syncs_builds_removes_and_cleans_one_native_project() {
    let root = tempfile::tempdir().unwrap();
    let selected = session(root.path(), true, false);
    let bytes = jar(root.path());
    execute_command_with_session(init(), &selected)
        .await
        .unwrap();
    execute_command_with_session(add("fixture.jar"), &selected)
        .await
        .unwrap();
    assert_eq!(resolved(root.path()).intent().roots.len(), 1);
    assert_eq!(
        fs::read(root.path().join("project/pack/mods/fixture.jar")).unwrap(),
        bytes
    );
    let before = snapshot(&root.path().join("project"));
    for _ in 0..2 {
        execute_command_with_session(
            Commands::Sync {
                materialize: false,
                continue_sync: false,
                files: vec![],
            },
            &selected,
        )
        .await
        .unwrap();
    }
    assert_eq!(snapshot(&root.path().join("project")), before);
    fs::create_dir_all(root.path().join("project/pack/config")).unwrap();
    fs::write(
        root.path().join("project/pack/config/example.toml"),
        b"first",
    )
    .unwrap();
    let build = Commands::Build(BuildArgs {
        targets: vec!["modrinth".into()],
        ..Default::default()
    });
    execute_command_with_session(build.clone(), &selected)
        .await
        .unwrap();
    fs::write(
        root.path().join("project/pack/config/example.toml"),
        b"second",
    )
    .unwrap();
    execute_command_with_session(build, &selected)
        .await
        .unwrap();
    let mut archive = zip::ZipArchive::new(
        fs::File::open(root.path().join("project/dist/Native Pack-1.mrpack")).unwrap(),
    )
    .unwrap();
    let mut content = String::new();
    archive
        .by_name("overrides/config/example.toml")
        .unwrap()
        .read_to_string(&mut content)
        .unwrap();
    assert_eq!(content, "second");
    drop(archive);
    execute_command_with_session(remove("fixture"), &selected)
        .await
        .unwrap();
    assert!(resolved(root.path()).intent().roots.is_empty());
    assert!(!root.path().join("project/pack/mods/fixture.jar").exists());
    execute_command_with_session(
        Commands::Sync {
            materialize: false,
            continue_sync: false,
            files: vec![],
        },
        &selected,
    )
    .await
    .unwrap();
    execute_command_with_session(
        Commands::Clean {
            targets: vec!["builds".into()],
        },
        &selected,
    )
    .await
    .unwrap();
    assert_eq!(
        fs::read(root.path().join("project/pack/config/example.toml")).unwrap(),
        b"second"
    );
    assert!(
        !root
            .path()
            .join("project/dist/Native Pack-1.mrpack")
            .exists()
    );
}
#[tokio::test]
async fn dispatcher_preview_decline_and_invalid_input_preserve_whole_project() {
    let root = tempfile::tempdir().unwrap();
    jar(root.path());
    execute_command_with_session(init(), &session(root.path(), true, false))
        .await
        .unwrap();
    let before = snapshot(root.path());
    for (yes, dry) in [(true, true), (false, false)] {
        execute_command_with_session(add("fixture.jar"), &session(root.path(), yes, dry))
            .await
            .unwrap();
        assert_eq!(snapshot(root.path()), before);
        let Commands::Init(mut args) = init() else {
            unreachable!()
        };
        args.force = true;
        execute_command_with_session(Commands::Init(args), &session(root.path(), yes, dry))
            .await
            .unwrap();
        assert_eq!(snapshot(root.path()), before);
    }
    let mut invalid = match add("fixture.jar") {
        Commands::Add {
            mods,
            force,
            platform,
            project_type,
            version_id,
            file_id,
            ..
        } => (mods, force, platform, project_type, version_id, file_id),
        _ => unreachable!(),
    };
    invalid.0.push("missing.jar".into());
    execute_command_with_session(
        Commands::Add {
            mods: invalid.0,
            force: invalid.1,
            platform: invalid.2,
            project_type: invalid.3,
            version_id: invalid.4,
            file_id: invalid.5,
            file_plan: None,
            download_as_local: false,
            continue_independent: false,
        },
        &session(root.path(), true, false),
    )
    .await
    .unwrap_err();
    assert_eq!(snapshot(root.path()), before);
}
#[tokio::test]
async fn removal_refuses_changed_file_kind_without_recursive_deletion() {
    let root = tempfile::tempdir().unwrap();
    jar(root.path());
    let selected = session(root.path(), true, false);
    execute_command_with_session(init(), &selected)
        .await
        .unwrap();
    execute_command_with_session(add("fixture.jar"), &selected)
        .await
        .unwrap();
    let file = root.path().join("project/pack/mods/fixture.jar");
    fs::remove_file(&file).unwrap();
    fs::create_dir(&file).unwrap();
    fs::write(file.join("sentinel"), b"keep").unwrap();
    let before = snapshot(root.path());
    assert!(
        execute_command_with_session(remove("fixture"), &selected)
            .await
            .is_err()
    );
    assert_eq!(snapshot(root.path()), before);
}
#[tokio::test]
async fn inspection_commands_need_no_project_or_backend_bootstrap() {
    let root = tempfile::tempdir().unwrap();
    let selected = session(root.path(), true, false);
    let before = snapshot(root.path());
    execute_command_with_session(Commands::Requirements, &selected)
        .await
        .unwrap();
    execute_command_with_session(Commands::Version, &selected)
        .await
        .unwrap();
    assert_eq!(snapshot(root.path()), before);
}

#[tokio::test]
async fn native_snapshot_dispatch_previews_installs_updates_and_rejects_tampering() {
    use crate::application::cli::InstanceCommand;
    use crate::engine::release::*;
    use sha2::{Digest, Sha256};
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("project")).unwrap();
    fs::create_dir(root.path().join("assets")).unwrap();
    let make = |bytes: &[u8]| {
        fs::write(root.path().join("assets/mod"), bytes).unwrap();
        let payload = DecodedRelease::encode(ReleaseDocument {
            schema: 1,
            pack: "dispatch".into(),
            version: "1".into(),
            minimum_engine: ">=0.6.0-beta".into(),
            runtime: ReleaseRuntime {
                minecraft: "1.21.1".into(),
                loader: ReleaseLoader::Vanilla,
                java_major: 21,
            },
            choices: vec![],
            files: vec![ReleaseFile {
                key: "mod".into(),
                destination: "mods/test.jar".into(),
                layer: ReleaseLayer::Common,
                policy: FilePolicy::Managed,
                client: Participation::Required,
                server: Participation::Unsupported,
                sha256: Sha256::digest(bytes)
                    .iter()
                    .map(|v| format!("{v:02x}"))
                    .collect(),
                bytes: bytes.len() as u64,
                readonly: false,
                executable: false,
                assertions: vec![],
                asset: None,
                source: ReleaseSource::Asset {
                    path: "assets/mod".into(),
                },
            }],
        })
        .unwrap();
        fs::write(root.path().join("release.json"), payload.bytes()).unwrap();
        Commands::Instance {
            command: InstanceCommand::Install {
                release: "release.json".into(),
                sha256: payload.id().into(),
                side: "client".into(),
                layout: Some("prism".into()),
                choices: vec![],
                files: vec![],
            },
        }
    };
    let a = make(b"A");
    let Commands::Instance {
        command: InstanceCommand::Install { sha256: first, .. },
    } = &a
    else {
        panic!()
    };
    let first = first.clone();
    let before = snapshot(root.path());
    execute_command_with_session(a.clone(), &session(root.path(), true, true))
        .await
        .unwrap();
    assert_eq!(before, snapshot(root.path()));
    execute_command_with_session(a, &session(root.path(), true, false))
        .await
        .unwrap();
    assert_eq!(
        fs::read(root.path().join("project/.minecraft/mods/test.jar")).unwrap(),
        b"A"
    );
    let b = make(b"B");
    execute_command_with_session(b.clone(), &session(root.path(), true, false))
        .await
        .unwrap();
    assert_eq!(
        fs::read(root.path().join("project/.minecraft/mods/test.jar")).unwrap(),
        b"B"
    );
    let before = snapshot(root.path());
    execute_command_with_session(
        Commands::Instance {
            command: InstanceCommand::Inspect,
        },
        &session(root.path(), true, false),
    )
    .await
    .unwrap();
    assert_eq!(before, snapshot(root.path()));
    fs::remove_file(root.path().join("project/.minecraft/mods/test.jar")).unwrap();
    let repair = Commands::Instance {
        command: InstanceCommand::Repair {
            assets: Some(".".into()),
            files: vec![],
        },
    };
    let before = snapshot(root.path());
    execute_command_with_session(repair.clone(), &session(root.path(), true, true))
        .await
        .unwrap();
    assert_eq!(before, snapshot(root.path()));
    execute_command_with_session(repair, &session(root.path(), true, false))
        .await
        .unwrap();
    assert_eq!(
        fs::read(root.path().join("project/.minecraft/mods/test.jar")).unwrap(),
        b"B"
    );
    fs::write(root.path().join("assets/mod"), b"A").unwrap();
    execute_command_with_session(
        Commands::Instance {
            command: InstanceCommand::Rollback {
                release: first,
                assets: Some(".".into()),
                files: vec![],
                choices: vec![],
            },
        },
        &session(root.path(), true, false),
    )
    .await
    .unwrap();
    assert_eq!(
        fs::read(root.path().join("project/.minecraft/mods/test.jar")).unwrap(),
        b"A"
    );
    let c = make(b"C");
    fs::write(root.path().join("assets/mod"), b"wrong").unwrap();
    let before = snapshot(&root.path().join("project"));
    assert!(
        execute_command_with_session(c, &session(root.path(), true, false))
            .await
            .is_err()
    );
    assert_eq!(before, snapshot(&root.path().join("project")));
    assert!(!root.path().join("project/empack.yml").exists());
    assert!(!root.path().join("project/pack").exists());
}

#[tokio::test]
async fn native_export_to_install_preserves_layers_and_uses_author_source_policy() {
    use crate::{application::cli::InstanceCommand, engine::release::DecodedRelease};
    use empack_core::model::{NativeDistributionIntent, ResolvedProject};
    let root = tempfile::tempdir().unwrap();
    let selected = session(root.path(), true, false);
    let mod_bytes = jar(root.path());
    execute_command_with_session(init(), &selected)
        .await
        .unwrap();
    execute_command_with_session(add("fixture.jar"), &selected)
        .await
        .unwrap();
    let project = resolved(root.path());
    let mut intent = project.intent().clone();
    intent.distribution.native = Some(NativeDistributionIntent {
        pack_id: "native-dispatch".into(),
        java_major: 21,
        policies: Default::default(),
    });
    intent.source_excludes = vec![
        "*.pw.toml".into(),
        "pack.toml".into(),
        "index.toml".into(),
        ".packwizignore".into(),
        "private/**".into(),
        "mods/**".into(),
    ];
    let encoded = DocumentCodec.encode_intent(&intent).unwrap();
    let decoded = DocumentCodec
        .decode_intent(&encoded, "native fixture")
        .unwrap();
    let mut lock = project.lock().clone();
    lock.intent_revision = decoded.semantic_revision();
    let project = ResolvedProject::validate(intent, lock, decoded.semantic_revision()).unwrap();
    fs::write(root.path().join("project/empack.yml"), encoded).unwrap();
    fs::write(
        root.path().join("project/empack.lock"),
        DocumentCodec.encode_lock(&project).unwrap(),
    )
    .unwrap();
    for (path, bytes) in [
        ("pack/config/value", "common"),
        ("overrides/client/config/value", "client"),
        ("overrides/server/config/value", "server"),
        ("pack/private/secret", "exclude me"),
        // Invalid foreign controls are not parsed, and cannot exclude native source input.
        ("pack/pack.toml", "invalid"),
        ("pack/.packwizignore", "config/"),
    ] {
        let path = root.path().join("project").join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    let build = Commands::Build(BuildArgs {
        targets: vec!["empack".into()],
        delivery: Some("bundled".into()),
        ..Default::default()
    });
    let before = snapshot(root.path());
    execute_command_with_session(build.clone(), &session(root.path(), true, true))
        .await
        .unwrap();
    assert_eq!(before, snapshot(root.path()));
    execute_command_with_session(build, &selected)
        .await
        .unwrap();
    let export = root.path().join("export");
    fs::create_dir(&export).unwrap();
    let mut archive = zip::ZipArchive::new(
        fs::File::open(
            root.path()
                .join("project/dist/Native Pack-1-empack-bundled.empack"),
        )
        .unwrap(),
    )
    .unwrap();
    archive.extract(&export).unwrap();
    let decoded = DecodedRelease::decode(&fs::read(export.join("release.json")).unwrap()).unwrap();
    assert_eq!(decoded.document().files.len(), 4);
    assert!(!String::from_utf8_lossy(decoded.bytes()).contains("private/secret"));
    for side in ["client", "server"] {
        let instance = root.path().join(side);
        fs::create_dir(&instance).unwrap();
        let host = session(root.path(), true, false).with_config(MockConfigProvider::new(
            crate::application::AppConfig {
                workdir: Some(side.into()),
                state_dir: Some("state".into()),
                yes: true,
                curseforge_api_client_key: None,
                ..Default::default()
            },
        ));
        execute_command_with_session(
            Commands::Instance {
                command: InstanceCommand::Install {
                    release: export.join("release.json"),
                    sha256: decoded.id().into(),
                    side: side.into(),
                    layout: None,
                    choices: vec![],
                    files: vec![],
                },
            },
            &host,
        )
        .await
        .unwrap();
        assert_eq!(
            fs::read(instance.join("game/config/value")).unwrap(),
            side.as_bytes()
        );
        assert_eq!(
            fs::read(instance.join("game/mods/fixture.jar")).unwrap(),
            mod_bytes
        );
        assert!(!instance.join("game/private").exists());
        assert!(!instance.join("game/pack.toml").exists());
    }
    // The actual client build dispatch uses native preparation without acquiring installer JARs.
    execute_command_with_session(
        Commands::Build(BuildArgs {
            targets: vec!["prism".into()],
            delivery: Some("references".into()),
            ..Default::default()
        }),
        &selected,
    )
    .await
    .unwrap();
    let prism = root.path().join("prism");
    fs::create_dir(&prism).unwrap();
    let mut archive = zip::ZipArchive::new(
        fs::File::open(
            root.path()
                .join("project/dist/Native Pack-1-prism-references.zip"),
        )
        .unwrap(),
    )
    .unwrap();
    archive.extract(&prism).unwrap();
    let descriptor = prism.join(".minecraft/.empack-consumer/release.json");
    let release = DecodedRelease::decode(&fs::read(&descriptor).unwrap()).unwrap();
    let host = session(root.path(), true, false).with_config(MockConfigProvider::new(
        crate::application::AppConfig {
            workdir: Some("prism".into()),
            state_dir: Some("state".into()),
            yes: true,
            curseforge_api_client_key: None,
            ..Default::default()
        },
    ));
    let prepare = Commands::Instance {
        command: InstanceCommand::Prepare {
            release: descriptor,
            sha256: release.id().into(),
            side: "client".into(),
            layout: Some("prism".into()),
            choices: vec![],
            files: vec![],
        },
    };
    for _ in 0..2 {
        execute_command_with_session(prepare.clone(), &host)
            .await
            .unwrap();
        assert_eq!(
            fs::read(prism.join(".minecraft/mods/fixture.jar")).unwrap(),
            mod_bytes
        );
        assert_eq!(
            fs::read(prism.join(".minecraft/config/value")).unwrap(),
            b"client"
        );
    }
}

#[tokio::test]
async fn instance_publisher_commands_preserve_preview_and_save_verified_floor() {
    use crate::application::cli::InstanceCommand;
    use crate::engine::{
        api::SubscriptionRecord,
        release::trust::{ChannelDocument, ChannelRelease, EnvelopeKind, sign},
    };
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("project")).unwrap();
    let key = ed25519_dalek::SigningKey::from_bytes(&[27; 32]);
    let encoded: String = key
        .verifying_key()
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let enroll = || Commands::Instance {
        command: InstanceCommand::Subscribe {
            pack: "fixture".into(),
            channel: "stable".into(),
            url: "https://publisher.test/channel.json".into(),
            keys: vec![encoded.clone()],
        },
    };
    execute_command_with_session(enroll(), &session(root.path(), true, true))
        .await
        .unwrap();
    assert_eq!(
        fs::read_dir(root.path().join("project")).unwrap().count(),
        0
    );
    execute_command_with_session(enroll(), &session(root.path(), true, false))
        .await
        .unwrap();
    let foreign =
        crate::engine::release::DecodedRelease::encode(crate::engine::release::ReleaseDocument {
            schema: 1,
            pack: "another-pack".into(),
            version: "1".into(),
            minimum_engine: ">=0.6.0-beta".into(),
            runtime: crate::engine::release::ReleaseRuntime {
                minecraft: "1.21.1".into(),
                loader: crate::engine::release::ReleaseLoader::Vanilla,
                java_major: 21,
            },
            choices: vec![],
            files: vec![],
        })
        .unwrap();
    fs::write(root.path().join("foreign.json"), foreign.bytes()).unwrap();
    assert!(
        execute_command_with_session(
            Commands::Instance {
                command: InstanceCommand::Install {
                    release: "foreign.json".into(),
                    sha256: foreign.id().into(),
                    side: "client".into(),
                    layout: None,
                    choices: vec![],
                    files: vec![],
                }
            },
            &session(root.path(), true, false)
        )
        .await
        .is_err()
    );
    use crate::engine::release::*;
    use sha2::{Digest, Sha256};
    fs::create_dir(root.path().join("assets")).unwrap();
    fs::write(root.path().join("assets/config"), b"signed bytes").unwrap();
    let release = DecodedRelease::encode(ReleaseDocument {
        schema: 1,
        pack: "fixture".into(),
        version: "1".into(),
        minimum_engine: ">=0.6.0-beta".into(),
        runtime: ReleaseRuntime {
            minecraft: "1.21.1".into(),
            loader: ReleaseLoader::Vanilla,
            java_major: 21,
        },
        choices: vec![],
        files: vec![ReleaseFile {
            key: "config".into(),
            destination: "config/example.txt".into(),
            layer: ReleaseLayer::Common,
            policy: FilePolicy::Managed,
            client: Participation::Required,
            server: Participation::Required,
            sha256: Sha256::digest(b"signed bytes")
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
            bytes: 12,
            readonly: false,
            executable: false,
            assertions: vec![],
            asset: None,
            source: ReleaseSource::Asset {
                path: "assets/config".into(),
            },
        }],
    })
    .unwrap();
    let envelope = sign(EnvelopeKind::Release, release.bytes(), &[&key]).unwrap();
    fs::write(root.path().join("signed-release.json"), &envelope).unwrap();
    let update = || Commands::Instance {
        command: InstanceCommand::Update {
            release: Some("signed-release.json".into()),
            side: "client".into(),
            layout: None,
            choices: vec![],
            files: vec![],
        },
    };
    assert!(
        execute_command_with_session(update(), &session(root.path(), true, false))
            .await
            .is_err()
    );
    let channel = ChannelDocument {
        schema: 1,
        pack: "fixture".into(),
        channel: "stable".into(),
        sequence: 3,
        expires: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            + 60,
        minimum_engine: ">=0.6.0-beta".into(),
        release: ChannelRelease {
            id: release.id().into(),
            url: "https://publisher.test/release.json".into(),
            maximum_bytes: envelope.len() as u64,
        },
    };
    fs::write(
        root.path().join("channel.json"),
        sign(EnvelopeKind::Channel, &channel.encode().unwrap(), &[&key]).unwrap(),
    )
    .unwrap();
    execute_command_with_session(
        Commands::Instance {
            command: InstanceCommand::ObserveChannel {
                envelope: Some("channel.json".into()),
            },
        },
        &session(root.path(), true, false),
    )
    .await
    .unwrap();
    let saved = root.path().join("project/.empack/subscription.json");
    assert_eq!(
        SubscriptionRecord::decode(&fs::read(&saved).unwrap())
            .unwrap()
            .floor
            .unwrap()
            .sequence,
        3
    );
    let before = snapshot(&root.path().join("project"));
    execute_command_with_session(update(), &session(root.path(), true, true))
        .await
        .unwrap();
    assert_eq!(before, snapshot(&root.path().join("project")));
    execute_command_with_session(update(), &session(root.path(), true, false))
        .await
        .unwrap();
    assert_eq!(
        fs::read(root.path().join("project/game/config/example.txt")).unwrap(),
        b"signed bytes"
    );
    execute_command_with_session(
        Commands::Instance {
            command: InstanceCommand::Trust {
                keys: vec![],
                revoke_all: true,
            },
        },
        &session(root.path(), true, false),
    )
    .await
    .unwrap();
    let record = SubscriptionRecord::decode(&fs::read(saved).unwrap()).unwrap();
    assert!(record.keys.is_empty());
    assert_eq!(record.floor.unwrap().sequence, 3);
    let before = snapshot(&root.path().join("project"));
    assert!(
        execute_command_with_session(update(), &session(root.path(), true, false))
            .await
            .is_err()
    );
    assert_eq!(before, snapshot(&root.path().join("project")));
}
