use super::*;
use crate::engine::{
    instance::{InstanceLayout, InstanceRecord, InstanceSide},
    release::{DecodedRelease, ReleaseDocument, ReleaseLoader, ReleaseRuntime},
};
use std::fs;

fn report(root: &Path, state: &Path) -> WorkspaceStatus {
    inspect_workspace(root, state, None, &Cancellation::default()).unwrap()
}
fn installed(root: &Path, layout: InstanceLayout, side: InstanceSide) {
    let release = DecodedRelease::encode(ReleaseDocument {
        schema: 1,
        require_subscription: false,
        server_launch: None,
        pack: "fixture-pack".into(),
        version: "2.3.0".into(),
        minimum_engine: "0.7.0-beta".into(),
        runtime: ReleaseRuntime {
            minecraft: "1.21.1".into(),
            loader: ReleaseLoader::Vanilla,
            java_major: 21,
        },
        choices: vec![],
        files: vec![],
    })
    .unwrap();
    let record = InstanceRecord {
        initialized_worlds: Vec::new(),
        schema: 1,
        root: super::super::publication::root_key(&ProjectReadRoot::open(root).unwrap()).unwrap(),
        pack: "fixture-pack".into(),
        release: release.id().into(),
        side,
        layout,
        choices: vec![],
        history: vec![],
        asset_bases: vec![],
        local_overrides: vec![],
    };
    fs::create_dir_all(root.join(".empack/releases")).unwrap();
    fs::write(
        root.join(".empack/instance.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join(format!(".empack/releases/{}.json", release.id())),
        release.bytes(),
    )
    .unwrap();
}
#[test]
fn status_detects_empty_mixed_and_explicit_contexts_without_creating_state() {
    let temp = tempfile::tempdir().unwrap();
    let state = temp.path().join("unused-state");
    let first = report(temp.path(), &state);
    assert!(first.contexts.is_empty());
    assert_eq!(first.findings[0].code, "workspace-empty");
    assert!(!state.exists());
    fs::create_dir_all(temp.path().join("dist/releases")).unwrap();
    fs::write(temp.path().join("empack.yml"), b"malformed").unwrap();
    let mixed = report(temp.path(), &state);
    assert!(mixed.selected.is_none());
    assert!(mixed.findings.iter().any(|f| f.code == "context-ambiguous"));
    assert_eq!(mixed.contexts[0].state, RecordState::Invalid);
    assert_eq!(mixed.contexts[1].state, RecordState::Unavailable);
    let selected = inspect_workspace(
        temp.path(),
        &state,
        Some(WorkspaceContext::Instance),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(selected.selected, Some(WorkspaceContext::Instance));
    assert_eq!(
        selected
            .contexts
            .iter()
            .find(|c| c.context == WorkspaceContext::Instance)
            .unwrap()
            .state,
        RecordState::Missing
    );
    assert!(
        !selected
            .findings
            .iter()
            .any(|f| f.code == "context-ambiguous")
    );
    assert!(!state.exists());
}
#[test]
fn status_reports_native_instance_version_side_and_root_binding() {
    for (layout, side) in [
        (InstanceLayout::Game, InstanceSide::Server),
        (InstanceLayout::Prism, InstanceSide::Client),
    ] {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("absent-state");
        installed(root.path(), layout, side);
        let value = report(root.path(), &state);
        let instance = &value.contexts[0];
        assert_eq!(instance.state, RecordState::Valid);
        assert_eq!(instance.version.as_deref(), Some("2.3.0"));
        assert_eq!(instance.game_directory.as_deref(), Some(layout.directory()));
        assert!(instance.update_owner.starts_with("snapshot"));
        assert!(value.text().contains("fixture-pack 2.3.0"));
        assert!(!state.exists());
        let path = root.path().join(".empack/instance.json");
        let mut record: InstanceRecord = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        record.root = "0".repeat(64);
        fs::write(path, serde_json::to_vec(&record).unwrap()).unwrap();
        assert_eq!(
            report(root.path(), &state).contexts[0].state,
            RecordState::Invalid
        );
    }
}
#[test]
fn status_reports_pending_presence_without_validating_or_rewriting_it() {
    use crate::engine::continuation_store::{self as store, Binding, BoundRecord, Kind};
    #[derive(Serialize, Deserialize)]
    struct Record {
        binding: Binding,
    }
    impl BoundRecord for Record {
        fn binding(&self) -> &Binding {
            &self.binding
        }
        fn schema(&self) -> u32 {
            1
        }
    }
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("project");
    fs::create_dir(&root).unwrap();
    let state = parent.path().join("state");
    let cancel = Cancellation::default();
    let (_, binding) = store::bind(&root, &cancel).unwrap();
    store::save(
        &state,
        Kind::Import,
        &root,
        &Record { binding },
        None,
        &cancel,
    )
    .unwrap();
    let before = tree(parent.path());
    let value = report(&root, &state);
    assert!(
        value
            .findings
            .iter()
            .any(|f| f.code == "pending-input" && f.next == ["init", "--continue"])
    );
    assert_eq!(before, tree(parent.path()));
}
#[cfg(unix)]
#[test]
fn status_rejects_symlinked_document_and_ancestor_without_following_them() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let sentinel = outside.path().join("sentinel");
    fs::write(&sentinel, b"outside").unwrap();
    symlink(&sentinel, root.path().join("empack.yml")).unwrap();
    symlink(outside.path(), root.path().join(".empack")).unwrap();
    let value = report(root.path(), &root.path().join("absent-state"));
    assert!(
        value
            .contexts
            .iter()
            .all(|c| c.state == RecordState::Invalid)
    );
    assert_eq!(fs::read(&sentinel).unwrap(), b"outside");
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 1);
}

fn tree(path: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    fn walk(path: &Path, result: &mut std::collections::BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                walk(&entry.path(), result);
            } else {
                // Windows mandatory byte-range locks forbid reading active coordination
                // files. Their content is empty by contract; verify that through metadata.
                let bytes = if matches!(
                    entry.file_name().to_str(),
                    Some("instance-run.lock" | "operation.lock")
                ) {
                    assert_eq!(entry.metadata().unwrap().len(), 0);
                    Vec::new()
                } else {
                    fs::read(entry.path()).unwrap()
                };
                result.insert(entry.path(), bytes);
            }
        }
    }
    let mut result = Default::default();
    walk(path, &mut result);
    result
}

#[test]
fn status_keeps_runtime_retirement_evidence_and_never_releases_the_lease() {
    let parent = tempfile::tempdir().unwrap();
    let project = parent.path().join("project");
    fs::create_dir(&project).unwrap();
    installed(&project, InstanceLayout::Game, InstanceSide::Server);
    let state = parent.path().join("state");
    let publisher = Publisher::open(&state).unwrap();
    let native = ProjectReadRoot::open(&project).unwrap();
    let mut lease = publisher.lease_instance(&native).unwrap();
    lease.begin().unwrap();
    let before = tree(parent.path());
    let value = report(&project, &state);
    assert!(
        value
            .findings
            .iter()
            .any(|f| f.code == "runtime-retirement-unconfirmed")
    );
    assert_eq!(before, tree(parent.path()));
    assert!(publisher.lease_instance(&native).is_err());
    lease.complete().unwrap();
}

#[test]
fn subscription_without_installation_reports_missing_instance_and_valid_enrollment() {
    use crate::engine::instance::subscription::SubscriptionRecord;
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("absent-state");
    let key = ed25519_dalek::SigningKey::from_bytes(&[7; 32]);
    let subscription = SubscriptionRecord {
        schema: 1,
        root: super::super::publication::root_key(&ProjectReadRoot::open(root.path()).unwrap())
            .unwrap(),
        pack: "fixture-pack".into(),
        channel: "stable".into(),
        url: "https://example.invalid/channels/stable.json".into(),
        keys: vec![crate::engine::release::hex(key.verifying_key().as_bytes())],
        floor: None,
        observed: None,
    };
    fs::create_dir(root.path().join(".empack")).unwrap();
    fs::write(
        root.path().join(".empack/subscription.json"),
        serde_json::to_vec(&subscription).unwrap(),
    )
    .unwrap();
    let before = tree(root.path());
    let value = report(root.path(), &state);
    assert_eq!(value.selected, Some(WorkspaceContext::Instance));
    assert_eq!(value.contexts[0].state, RecordState::Missing);
    assert!(
        value.contexts[0]
            .update_owner
            .contains("publisher channel stable")
    );
    assert!(
        !value
            .findings
            .iter()
            .any(|f| f.code.starts_with("subscription-"))
    );
    assert_eq!(tree(root.path()), before);
    fs::write(root.path().join(".empack/instance.json"), b"broken").unwrap();
    assert_eq!(
        report(root.path(), &state).contexts[0].state,
        RecordState::Invalid
    );
}
