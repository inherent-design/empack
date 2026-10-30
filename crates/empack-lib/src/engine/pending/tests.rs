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
            schema: if matches!(kind, Kind::Synchronization | Kind::Instance) {
                2
            } else {
                1
            },
            binding: store::bind_for(&state, kind, &target, &cancel).unwrap().1,
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
    for kind in [
        PendingKind::Build,
        PendingKind::Import,
        PendingKind::Sync,
        PendingKind::Instance,
    ] {
        assert_eq!(
            view.operations.iter().filter(|op| op.kind == kind).count(),
            1
        );
    }
    assert_eq!(
        view.operations
            .iter()
            .map(|op| &op.id)
            .collect::<BTreeSet<_>>()
            .len(),
        4
    );
    assert_eq!(serde_json::to_value(&view).unwrap()["schema"], 1);
    for operation in &view.operations {
        assert_eq!(operation.state, "unavailable");
        assert!(!operation.requirements_complete);
        assert!(operation.inputs.is_empty());
        let diagnostic = match operation.kind {
            PendingKind::Build => "Invalid pending record",
            PendingKind::Import => "Missing retained import associations",
            PendingKind::Sync => "Missing synchronization mode",
            PendingKind::Instance => "Missing release payload",
        };
        assert!(operation.diagnostic.as_ref().unwrap().contains(diagnostic));
        assert_eq!(
            operation.next,
            vec!["pending", "resume", operation.id.as_str()]
        );
    }
    for json in [false, true] {
        let (session, messages) = host(root.path(), false).record_messages();
        execute_command_with_session(
            Commands::Pending {
                command: PendingCommand::List { json },
            },
            &session,
        )
        .await
        .unwrap();
        let captured = messages.lock().unwrap().clone();
        assert_eq!(captured.len(), 1);
        if json {
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&captured[0]).unwrap(),
                serde_json::to_value(&view).unwrap()
            );
        } else {
            assert_eq!(captured[0], view.text());
            assert!(captured[0].contains("Metadata only"));
        }
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
fn pending_selectors_require_unique_hex_prefix_without_choosing_first_match() {
    let first = "a".repeat(64);
    let second = format!("{}b", "a".repeat(63));
    let report = PendingReport {
        schema: 1,
        root: "root".into(),
        operations: vec![
            empty(first.clone(), PendingKind::Build, Path::new("root")),
            empty(second.clone(), PendingKind::Import, Path::new("root")),
        ],
    };
    for prefix in ["a", &first[..63]] {
        assert!(
            report
                .select(prefix)
                .unwrap_err()
                .to_string()
                .contains("ambiguous")
        );
    }
    assert_eq!(report.select(&first).unwrap().kind, PendingKind::Build);
    assert_eq!(report.select(&second).unwrap().kind, PendingKind::Import);
    for invalid in ["", "not-hex", &"a".repeat(65)] {
        assert!(
            report
                .select(invalid)
                .unwrap_err()
                .to_string()
                .contains("hexadecimal identity")
        );
    }
    assert!(
        report
            .select("b")
            .unwrap_err()
            .to_string()
            .contains("No pending operation matches")
    );
    let unique = PendingReport {
        operations: vec![report.operations[0].clone()],
        ..report
    };
    assert_eq!(unique.select("a").unwrap().id, first);
}

#[test]
fn pending_replacement_rejects_stale_cleanup_and_cross_root_selection() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("project");
    fs::create_dir(&target).unwrap();
    let state = root.path().join("state");
    let cancel = Cancellation::default();
    let record = OpaqueRecord {
        schema: 1,
        binding: store::bind(&state, &target, &cancel).unwrap().1,
        marker: 0,
    };
    let saved = store::save(&state, Kind::Import, &target, &record, None, &cancel).unwrap();
    let view = inspect(&state, &target, &cancel).unwrap();
    let old = view.operations[0].id.clone();
    let another = root.path().join("another");
    fs::create_dir(&another).unwrap();
    assert!(
        inspect(&state, &another, &cancel)
            .unwrap()
            .select(&old)
            .is_err()
    );
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
    let replacement = inspect(&state, &target, &cancel).unwrap();
    let current = replacement.operations[0].id.clone();
    assert_ne!(current, old);
    let preserved = snapshot(root.path());
    assert!(!store::discard(&saved.into_cleanup(), &cancel).unwrap());
    assert_eq!(snapshot(root.path()), preserved);
    assert_eq!(
        inspect(&state, &target, &cancel)
            .unwrap()
            .select(&current)
            .unwrap()
            .id,
        current
    );
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
