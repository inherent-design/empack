use super::*;
use crate::{
    application::{
        Commands,
        cli::Cli,
        commands::execute_command_with_session,
        session_mocks::{MockCommandSession, MockConfigProvider},
    },
    engine::{
        publication::{
            PublicationPoint, Publisher,
            tests::{interrupt_publication, prepare},
        },
        snapshot::ProjectReadRoot,
    },
};
use clap::Parser;
use std::{collections::BTreeMap, fs};

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, dir: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                visit(root, &entry.path(), files);
            } else {
                files.insert(
                    entry.path().strip_prefix(root).unwrap().to_path_buf(),
                    fs::read(entry.path()).unwrap(),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    if root.exists() {
        visit(root, root, &mut files);
    }
    files
}
#[test]
fn recovery_parsing_and_state_selection_have_one_invocation_root() {
    let cli = Cli::try_parse_from([
        "empack",
        "--state-dir",
        "host/state",
        "recover",
        "restore",
        "--operation",
        "exact",
    ])
    .unwrap();
    assert!(
        matches!(cli.command, Some(Commands::Recover { action: CliRecoveryAction::Restore, operation: Some(ref id) }) if id == "exact")
    );
    let root = tempfile::tempdir().unwrap();
    assert_eq!(
        state_root(&cli.config, root.path()).unwrap(),
        root.path().join("host/state")
    );
    let cli = Cli::try_parse_from(["empack", "recover"]).unwrap();
    assert!(matches!(
        cli.command,
        Some(Commands::Recover {
            action: CliRecoveryAction::Inspect,
            ..
        })
    ));
    assert!(
        !Commands::Recover {
            action: CliRecoveryAction::Inspect,
            operation: None
        }
        .requires_modpack()
    );
    assert!(Cli::try_parse_from(["empack", "recover", "delete"]).is_err());
}
#[tokio::test]
async fn recovery_dispatch_inspects_previews_declines_and_recovers_native_publication() {
    for action in [CliRecoveryAction::Finish, CliRecoveryAction::Restore] {
        let project = tempfile::tempdir().unwrap();
        let host = tempfile::tempdir().unwrap();
        let state = host.path().join("state");
        fs::write(project.path().join("empack.yml"), b"old intent").unwrap();
        let root = ProjectReadRoot::open(project.path()).unwrap();
        let publisher = Publisher::open(&state).unwrap();
        assert!(
            interrupt_publication(
                &publisher,
                &root,
                prepare(&root),
                PublicationPoint::TargetChanged
            )
            .is_err()
        );
        let status = publisher.inspect_recovery(&root).unwrap().unwrap();
        let config = AppConfig {
            workdir: Some(project.path().to_path_buf()),
            state_dir: Some(state),
            ..Default::default()
        };
        let before_project = snapshot(project.path());
        let before_host = snapshot(host.path());
        for mode in ["inspect", "preview", "decline", "wrong-operation"] {
            let mut config = config.clone();
            config.dry_run = mode == "preview";
            let session = MockCommandSession::new().with_config(MockConfigProvider::new(config));
            let result = execute_command_with_session(
                Commands::Recover {
                    action: if mode == "inspect" {
                        CliRecoveryAction::Inspect
                    } else {
                        action
                    },
                    operation: Some(if mode == "wrong-operation" {
                        "wrong".into()
                    } else {
                        status.operation.clone()
                    }),
                },
                &session,
            )
            .await;
            assert_eq!(
                result.is_err(),
                mode == "wrong-operation",
                "{mode}: {result:?}"
            );
            assert_eq!(snapshot(project.path()), before_project);
            assert_eq!(snapshot(host.path()), before_host);
        }
        let mut config = config;
        config.yes = true;
        let session = MockCommandSession::new().with_config(MockConfigProvider::new(config));
        execute_command_with_session(
            Commands::Recover {
                action,
                operation: Some(status.operation),
            },
            &session,
        )
        .await
        .unwrap();
        assert!(publisher.inspect_recovery(&root).unwrap().is_none());
        assert_eq!(
            fs::read(project.path().join("empack.yml")).unwrap(),
            if action == CliRecoveryAction::Finish {
                b"new intent"
            } else {
                b"old intent"
            }
        );
        assert_eq!(
            project.path().join("pack/config/new.txt").exists(),
            action == CliRecoveryAction::Finish
        );
    }
}
