use super::*;
use std::fs;
const DOCUMENT: &str = r#"schema: 2
pack: {name: Example, version: alpha}
runtime: {minecraft: '1.20.1', loader: {kind: vanilla}}
distribution: {targets: [mrpack], archive: zip}
dependencies: {}
layout: {}
extensions: {}
"#;
#[test]
fn capture_is_read_only_and_missing_lock_is_distinct_from_current_resolution() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("empack.yml"), DOCUMENT).unwrap();
    let host = temp.path().join("uncreated-host-state");
    let reader = ProjectReader::new(RecoveryReader::new(host.clone()));
    let snapshot = reader
        .capture(
            &project,
            &[],
            SnapshotLimits::default(),
            &Cancellation::default(),
        )
        .unwrap();
    assert!(snapshot.prior_lock().is_none());
    assert!(snapshot.require_resolved().is_err());
    assert!(!host.exists());
    assert_eq!(fs::read_dir(&project).unwrap().count(), 1);
    assert_eq!(
        fs::read_to_string(project.join("empack.yml")).unwrap(),
        DOCUMENT
    );
}
#[test]
fn malformed_lock_is_an_error_and_comment_changes_invalidate_preparation() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("empack.yml"), DOCUMENT).unwrap();
    let reader = ProjectReader::new(RecoveryReader::new(temp.path().join("private-state")));
    let snapshot = reader
        .capture(
            temp.path(),
            &[],
            SnapshotLimits::default(),
            &Cancellation::default(),
        )
        .unwrap();
    fs::write(
        temp.path().join("empack.yml"),
        format!("# new comment\n{DOCUMENT}"),
    )
    .unwrap();
    assert!(
        snapshot
            .root()
            .revalidate(snapshot.observations(), &Cancellation::default())
            .is_err()
    );
    fs::write(temp.path().join("empack.lock"), "schema: 999\n").unwrap();
    assert!(
        reader
            .capture(
                temp.path(),
                &[],
                SnapshotLimits::default(),
                &Cancellation::default()
            )
            .is_err()
    );
    assert!(!temp.path().join("private-state").exists());
}
