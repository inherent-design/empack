use super::*;
use crate::application::{
    AppConfig, Commands,
    cli::PendingCommand,
    commands::execute_command_with_session,
    session_mocks::{MockCommandSession, MockConfigProvider, MockInvocationProvider},
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs};

#[derive(Serialize, Deserialize)]
struct OpaqueRecord {
    schema: u32,
    binding: store::Binding,
    marker: u32,
}
impl store::BoundRecord for OpaqueRecord {
    fn schema(&self) -> u32 {
        self.schema
    }
    fn binding(&self) -> &store::Binding {
        &self.binding
    }
}
fn snapshot(path: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, current: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(current).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                visit(root, &entry.path(), files);
            } else {
                files.insert(
                    entry.path().strip_prefix(root).unwrap().into(),
                    fs::read(entry.path()).unwrap(),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(path, path, &mut files);
    files
}
fn host(root: &Path, dry: bool) -> MockCommandSession {
    MockCommandSession::new()
        .with_invocation(MockInvocationProvider::new().with_current_dir(root.into()))
        .with_config(MockConfigProvider::new(AppConfig {
            workdir: Some(root.join("project")),
            state_dir: Some(root.join("state")),
            cache_dir: Some(root.join("cache")),
            yes: true,
            dry_run: dry,
            ..Default::default()
        }))
}
#[tokio::test]
async fn shared_pending_views_and_discard_preserve_neighboring_state() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("project");
    fs::create_dir(&target).unwrap();
    let state = root.path().join("state");
    let cancel = Cancellation::default();
    for kind in [Kind::Import, Kind::Synchronization, Kind::Instance] {
        let record = OpaqueRecord {
            schema: if kind == Kind::Synchronization { 2 } else { 1 },
            binding: store::bind_for(kind, &target, &cancel).unwrap().1,
            marker: 0,
        };
        store::save(&state, kind, &target, &record, None, &cancel).unwrap();
    }
    let builds = state.join("pending-builds");
    super::super::publication::open_private_directory(&builds, true).unwrap();
    fs::write(builds.join("state.lock"), b"").unwrap();
    let key = super::super::publication::root_key(
        &super::super::snapshot::ProjectReadRoot::open(&target).unwrap(),
    )
    .unwrap();
    fs::write(
        builds.join(format!("{key}.json")),
        b"invalid but explicitly discardable",
    )
    .unwrap();
    for name in [
        "unknown.json",
        "user-download.jar",
        "active-lease",
        "recovery-journal",
    ] {
        fs::write(state.join(name), b"keep").unwrap();
    }
    let original = snapshot(root.path());
    let view = inspect(&state, &target, &cancel).unwrap();
    assert_eq!(view.operations.len(), 4);
    assert_eq!(
        view.operations
            .iter()
            .map(|op| &op.id)
            .collect::<BTreeSet<_>>()
            .len(),
        4
    );
    assert_eq!(serde_json::to_value(&view).unwrap()["schema"], 1);
    for json in [false, true] {
        execute_command_with_session(
            Commands::Pending {
                command: PendingCommand::List { json },
            },
            &host(root.path(), false),
        )
        .await
        .unwrap();
        for operation in &view.operations {
            execute_command_with_session(
                Commands::Pending {
                    command: PendingCommand::Show {
                        id: operation.id[..12].into(),
                        json,
                    },
                },
                &host(root.path(), false),
            )
            .await
            .unwrap();
            execute_command_with_session(
                Commands::Pending {
                    command: PendingCommand::Discard {
                        id: operation.id.clone(),
                    },
                },
                &host(root.path(), true),
            )
            .await
            .unwrap();
        }
    }
    assert_eq!(original, snapshot(root.path()));
    let another = root.path().join("another");
    fs::create_dir(&another).unwrap();
    assert!(
        inspect(&state, &another, &cancel)
            .unwrap()
            .select(&view.operations[0].id)
            .is_err()
    );
    for operation in view.operations {
        let before = snapshot(root.path());
        execute_command_with_session(
            Commands::Pending {
                command: PendingCommand::Discard {
                    id: operation.id.clone(),
                },
            },
            &host(root.path(), false),
        )
        .await
        .unwrap();
        let after = snapshot(root.path());
        assert_eq!(before.len(), after.len() + 1);
        for (path, bytes) in &after {
            assert_eq!(before.get(path), Some(bytes));
        }
        assert!(
            inspect(&state, &target, &cancel)
                .unwrap()
                .select(&operation.id)
                .is_err()
        );
    }
    for name in [
        "unknown.json",
        "user-download.jar",
        "active-lease",
        "recovery-journal",
    ] {
        assert_eq!(fs::read(state.join(name)).unwrap(), b"keep");
    }
}
#[test]
fn pending_ids_reject_ambiguity_and_replacement_without_crossing_roots() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("project");
    fs::create_dir(&target).unwrap();
    let state = root.path().join("state");
    let cancel = Cancellation::default();
    let record = OpaqueRecord {
        schema: 1,
        binding: store::bind(&target, &cancel).unwrap().1,
        marker: 0,
    };
    let saved = store::save(&state, Kind::Import, &target, &record, None, &cancel).unwrap();
    let view = inspect(&state, &target, &cancel).unwrap();
    let old = view.operations[0].id.clone();
    let mut ambiguous = view.clone();
    ambiguous.operations[0].id = "a".repeat(64);
    let mut duplicate = ambiguous.operations[0].clone();
    duplicate.id = format!("{}b", "a".repeat(63));
    ambiguous.operations.push(duplicate);
    assert!(
        ambiguous
            .select("a")
            .unwrap_err()
            .to_string()
            .contains("ambiguous")
    );
    assert!(ambiguous.select("").is_err());
    let replacement = OpaqueRecord {
        marker: 1,
        ..record
    };
    store::save(
        &state,
        Kind::Import,
        &target,
        &replacement,
        Some(&saved),
        &cancel,
    )
    .unwrap();
    assert!(!store::discard(&saved.into_cleanup(), &cancel).unwrap());
    assert!(
        inspect(&state, &target, &cancel)
            .unwrap()
            .select(&old)
            .is_err()
    );
}

#[test]
fn provider_selections_have_stable_separate_wire_fields() {
    use empack_core::identity::{ModrinthProjectId, ProviderProjectId};
    let project = ProviderProjectId::Modrinth(ModrinthProjectId::parse("Project1").unwrap());
    let pin = ResolvedPin {
        selection: project.parse_pin("Version1").unwrap(),
        project,
    };
    let selected = PendingProviderSelection::from(&pin);
    assert_eq!(
        serde_json::to_value(selected).unwrap(),
        serde_json::json!({
            "provider":"modrinth", "project":"Project1", "selection":"Version1"
        })
    );
}
