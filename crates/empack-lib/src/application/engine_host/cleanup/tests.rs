use super::*;
use crate::application::session_mocks::{
    MockCommandSession, MockConfigProvider, MockInteractiveProvider, MockInvocationProvider,
};
use std::fs;
fn session(root: &Path, yes: bool, dry: bool) -> MockCommandSession {
    MockCommandSession::new()
        .with_invocation(MockInvocationProvider::new().with_current_dir(root.to_path_buf()))
        .with_config(MockConfigProvider::new(AppConfig {
            workdir: Some("project".into()),
            state_dir: Some("state".into()),
            cache_dir: Some("cache".into()),
            yes,
            dry_run: dry,
            ..Default::default()
        }))
}
fn cache(root: &Path) -> PathBuf {
    let cache = root.join("content-v1");
    FileContentStore::open(&cache, ContentStoreLimits::default()).unwrap();
    fs::write(
        cache.join(format!("{}.blob", "ab".repeat(32))),
        b"cached bytes",
    )
    .unwrap();
    fs::write(cache.join("neighbor"), b"keep").unwrap();
    cache
}
fn project(root: &Path) {
    fs::create_dir_all(root.join("project/dist/nested")).unwrap();
    fs::write(
        root.join("project/empack.yml"),
        b"invalid document, cleanup needs no document",
    )
    .unwrap();
    fs::write(root.join("project/dist/nested/output.zip"), [0, 255, 128]).unwrap();
    fs::write(root.join("project/source"), b"keep").unwrap();
}
#[tokio::test]
async fn cleanup_previews_every_scope_and_confirms_once_without_requiring_valid_documents() {
    let root = tempfile::tempdir().unwrap();
    project(root.path());
    let cache = cache(root.path());
    let all = Selection::parse(&["all".into()]).unwrap();
    let before = super::super::tests::snapshot(root.path());
    for (yes, dry) in [(true, true), (false, false)] {
        clean_selected(&session(root.path(), yes, dry), all, Some(cache.clone()))
            .await
            .unwrap();
        assert_eq!(before, super::super::tests::snapshot(root.path()));
    }
    let interactive = MockInteractiveProvider::new().with_confirm(true);
    let calls = interactive.confirm_calls.clone();
    let selected = session(root.path(), false, false).with_interactive(interactive);
    clean_selected(&selected, all, Some(cache.clone()))
        .await
        .unwrap();
    assert_eq!(calls.lock().unwrap().len(), 1);
    assert!(!root.path().join("project/dist/nested/output.zip").exists());
    assert!(!cache.join(format!("{}.blob", "ab".repeat(32))).exists());
    assert_eq!(fs::read(cache.join("neighbor")).unwrap(), b"keep");
    assert_eq!(
        fs::read(root.path().join("project/source")).unwrap(),
        b"keep"
    );
    assert_eq!(
        fs::read(root.path().join("project/empack.yml")).unwrap(),
        before[&PathBuf::from("project/empack.yml")]
    );
    assert!(
        root.path().join("state").exists(),
        "Artifact recovery is retained outside cache cleanup"
    );
}
#[tokio::test]
async fn cache_cleanup_does_not_require_a_project_or_create_an_absent_store() {
    let root = tempfile::tempdir().unwrap();
    let selected = Selection::parse(&["cache".into()]).unwrap();
    let missing = root.path().join("absent/content-v1");
    for dry in [true, false] {
        clean_selected(
            &session(root.path(), true, dry),
            selected,
            Some(missing.clone()),
        )
        .await
        .unwrap();
        assert!(super::super::tests::snapshot(root.path()).is_empty());
        assert!(!root.path().join("absent").exists());
    }
    let cache = cache(root.path());
    clean_selected(
        &session(root.path(), true, false),
        selected,
        Some(cache.clone()),
    )
    .await
    .unwrap();
    assert!(!root.path().join("project").exists());
    assert!(!root.path().join("state").exists());
    assert!(!cache.join(format!("{}.blob", "ab".repeat(32))).exists());
    assert_eq!(fs::read(cache.join("neighbor")).unwrap(), b"keep");
}
#[tokio::test]
async fn artifact_defaults_unknown_targets_and_failed_second_preflight_preserve_other_scopes() {
    let root = tempfile::tempdir().unwrap();
    project(root.path());
    let cache = cache(root.path());
    assert!(Selection::parse(&["builds".into(), "typo".into()]).is_err());
    let before = super::super::tests::snapshot(root.path());
    // A malformed canonical cache object must prevent the artifact cleanup from starting.
    fs::create_dir(cache.join(format!("{}.blob", "cd".repeat(32)))).unwrap();
    assert!(
        clean_selected(
            &session(root.path(), true, false),
            Selection::parse(&["all".into()]).unwrap(),
            Some(cache.clone())
        )
        .await
        .is_err()
    );
    assert_eq!(before, super::super::tests::snapshot(root.path()));
    assert!(!root.path().join("state").exists());
    clean_selected(
        &session(root.path(), true, false),
        Selection::parse(&[]).unwrap(),
        None,
    )
    .await
    .unwrap();
    assert!(!root.path().join("project/dist/nested/output.zip").exists());
    assert_eq!(
        fs::read(cache.join(format!("{}.blob", "ab".repeat(32)))).unwrap(),
        b"cached bytes"
    );
}
#[tokio::test]
async fn combined_cleanup_rejects_overlapping_storage_before_approval() {
    let root = tempfile::tempdir().unwrap();
    project(root.path());
    let cache = root.path().join("project/dist/content-v1");
    FileContentStore::open(&cache, ContentStoreLimits::default()).unwrap();
    let before = super::super::tests::snapshot(root.path());
    let error = clean_selected(
        &session(root.path(), true, false),
        Selection::parse(&["all".into()]).unwrap(),
        Some(cache),
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("overlap"));
    assert_eq!(before, super::super::tests::snapshot(root.path()));
}

#[tokio::test]
async fn later_cleanup_failure_reports_earlier_artifact_publication() {
    let root = tempfile::tempdir().unwrap();
    project(root.path());
    let cache = cache(root.path());
    let session = session(root.path(), true, false);
    let engine = engine(session.config().app_config(), root.path())
        .unwrap()
        .with_content_store(
            FileContentStore::open_existing(&cache, ContentStoreLimits::default())
                .unwrap()
                .unwrap(),
        );
    let Preparation::Ready(artifacts) = engine
        .prepare(root.path().join("project"), CleanRequest::Artifacts)
        .await
        .unwrap()
    else {
        panic!("Cleanup must be ready");
    };
    let content = engine
        .prepare_cache_cleanup(CacheCleanRequest::All)
        .await
        .unwrap();
    let changed = cache.join(format!("{}.blob", "ab".repeat(32)));
    fs::write(&changed, b"independent changed cache object").unwrap();
    let error = apply_cleanup(
        &session,
        &engine,
        vec![
            ("Artifact cleanup", artifacts),
            ("Content cache cleanup", content),
        ],
        None,
        None,
        None,
    )
    .await
    .unwrap_err();
    assert!(format!("{error:#}").contains("already completed: Artifact cleanup"));
    assert!(format!("{error:#}").contains("changed after cleanup preparation"));
    assert!(!root.path().join("project/dist/nested/output.zip").exists());
    assert_eq!(
        fs::read(&changed).unwrap(),
        b"independent changed cache object"
    );
    assert_eq!(fs::read(cache.join("neighbor")).unwrap(), b"keep");
    engine.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn selected_artifact_link_cannot_delete_outside_bytes_or_start_cache_cleanup() {
    let root = tempfile::tempdir().unwrap();
    project(root.path());
    let cache = cache(root.path());
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("sentinel"), b"outside").unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("sentinel"),
        root.path().join("project/dist/linked"),
    )
    .unwrap();
    assert!(
        clean_selected(
            &session(root.path(), true, false),
            Selection::parse(&["all".into()]).unwrap(),
            Some(cache.clone())
        )
        .await
        .is_err()
    );
    assert_eq!(
        fs::read(outside.path().join("sentinel")).unwrap(),
        b"outside"
    );
    assert_eq!(
        fs::read(root.path().join("project/dist/nested/output.zip")).unwrap(),
        [0, 255, 128]
    );
    assert_eq!(
        fs::read(cache.join(format!("{}.blob", "ab".repeat(32)))).unwrap(),
        b"cached bytes"
    );
    assert!(!root.path().join("state").exists());
}
