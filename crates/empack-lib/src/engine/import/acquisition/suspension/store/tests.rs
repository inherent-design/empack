use super::*;
use std::fs;
fn fixture(target: &Path) -> Record {
    Record {
        schema: 1,
        binding: bind(target, &Cancellation::default()).unwrap().1,
        archive: [1; 32],
        archive_bytes: 7,
        archive_digests: vec![],
        revision: [2; 32],
        strong: false,
        files: BTreeMap::new(),
    }
}
#[test]
fn stale_import_cannot_resume_but_explicit_cleanup_remains_available() {
    for change in ["created", "document", "replaced", "invalid"] {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("project");
        let state = root.path().join("state");
        if change != "created" {
            fs::create_dir(&target).unwrap();
            fs::write(target.join("empack.yml"), b"original").unwrap();
        }
        let cancel = Cancellation::default();
        let record = fixture(&target);
        let saved = save(&state, &target, &record, None, &cancel).unwrap();
        match change {
            "created" => fs::create_dir(&target).unwrap(),
            "document" => fs::write(target.join("empack.yml"), b"changed").unwrap(),
            "replaced" => {
                fs::rename(&target, root.path().join("old-project")).unwrap();
                fs::create_dir(&target).unwrap();
                fs::write(target.join("empack.yml"), b"original").unwrap();
            }
            "invalid" => {
                fs::write(state.join("pending-imports").join(&saved.name), b"invalid").unwrap()
            }
            _ => unreachable!(),
        }
        let path = state.join("pending-imports").join(&saved.name);
        let before = fs::read(&path).unwrap();
        assert!(read(&state, &target, &cancel).is_err(), "{change}");
        let observed = observe(&state, &target, &cancel).unwrap().unwrap();
        assert_eq!(fs::read(&path).unwrap(), before);
        assert!(discard(&observed, &cancel).unwrap());
        assert!(target.exists());
        assert!(!path.exists());
    }
}
#[test]
fn extension_and_cleanup_compare_exact_observed_bytes() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("project");
    let other = root.path().join("other");
    let state = root.path().join("state");
    let cancel = Cancellation::default();
    assert!(read(&state, &target, &cancel).unwrap().is_none());
    assert!(observe(&state, &target, &cancel).unwrap().is_none());
    assert!(!state.exists());
    let record = fixture(&target);
    let saved = save(&state, &target, &record, None, &cancel).unwrap();
    assert!(save(&state, &target, &record, None, &cancel).is_err());
    let observed = observe(&state, &target, &cancel).unwrap().unwrap();
    let mut next = record;
    next.files.insert("exact-file".into(), [3; 32]);
    let extended = save(&state, &target, &next, Some(&saved), &cancel).unwrap();
    assert!(!discard(&observed, &cancel).unwrap());
    assert!(save(&state, &target, &next, Some(&saved), &cancel).is_err());
    let other_record = fixture(&other);
    let foreign = save(&state, &other, &other_record, None, &cancel).unwrap();
    assert!(save(&state, &target, &next, Some(&foreign), &cancel).is_err());
    assert_eq!(
        read(&state, &target, &cancel)
            .unwrap()
            .unwrap()
            .1
            .files
            .len(),
        1
    );
    fs::remove_file(state.join("pending-imports").join(&extended.name)).unwrap();
    assert!(save(&state, &target, &next, Some(&extended), &cancel).is_err());
    assert!(!target.exists());
}
#[test]
fn bounded_records_reject_unknown_fields() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("project");
    let state = root.path().join("state");
    let cancel = Cancellation::default();
    let record = fixture(&target);
    let saved = save(&state, &target, &record, None, &cancel).unwrap();
    let path = state.join("pending-imports").join(saved.name);
    let mut json = serde_json::to_value(&record).unwrap();
    json["destination"] = serde_json::json!("/untrusted");
    fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
    assert!(read(&state, &target, &cancel).is_err());
    assert!(observe(&state, &target, &cancel).unwrap().is_some());
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(MAX_RECORD + 1)
        .unwrap();
    assert!(read(&state, &target, &cancel).is_err());
    assert!(observe(&state, &target, &cancel).is_err());
    assert!(path.exists());
}
#[cfg(unix)]
#[test]
fn resume_rejects_symlink_targets_and_cleanup_does_not_follow_them() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("project");
    let outside = root.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("sentinel"), b"keep").unwrap();
    let state = root.path().join("state");
    let cancel = Cancellation::default();
    let record = fixture(&target);
    save(&state, &target, &record, None, &cancel).unwrap();
    symlink(&outside, &target).unwrap();
    assert!(read(&state, &target, &cancel).is_err());
    let observed = observe(&state, &target, &cancel).unwrap().unwrap();
    assert!(discard(&observed, &cancel).unwrap());
    assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"keep");
    assert!(target.is_symlink());
}
