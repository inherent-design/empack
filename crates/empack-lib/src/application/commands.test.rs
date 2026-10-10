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
        targets: vec!["mrpack".into()],
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
        fs::read(root.path().join("project/game/mods/test.jar")).unwrap(),
        b"A"
    );
    let b = make(b"B");
    execute_command_with_session(b.clone(), &session(root.path(), true, false))
        .await
        .unwrap();
    assert_eq!(
        fs::read(root.path().join("project/game/mods/test.jar")).unwrap(),
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
    fs::remove_file(root.path().join("project/game/mods/test.jar")).unwrap();
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
        fs::read(root.path().join("project/game/mods/test.jar")).unwrap(),
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
        fs::read(root.path().join("project/game/mods/test.jar")).unwrap(),
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
