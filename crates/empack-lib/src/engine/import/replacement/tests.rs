use super::*;
use crate::engine::{
    build::{BuildAcquisitions, prepare_mrpack},
    content::SourceEvidencePolicy,
    import::{
        ImportCandidate,
        acquisition::tests::{interpret, mr, source},
    },
    mrpack::OptionalConversion,
    project::ProjectReader,
    publication::RecoveryReader,
    runtime::OperationOutcome,
    snapshot::SnapshotLimits,
};
use std::{fs, io::Read, sync::Arc};

async fn candidate() -> ImportCandidate {
    let archive = source(
        "modrinth.index.json",
        mr(vec![]),
        &[
            ("overrides/config/a", b"shared"),
            ("client-overrides/config/a", b"client"),
            ("server-overrides/config/a", b"server"),
        ],
    );
    let outcome = interpret(archive, "http://127.0.0.1:1".into(), |_| {}).await;
    let Some(OperationOutcome::Completed(Ok(candidate))) = Arc::into_inner(outcome) else {
        panic!("candidate interpretation failed");
    };
    candidate
}
#[tokio::test]
async fn forced_replacement_is_read_only_until_verified_publication_and_reexports_both_sides() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let host = temp.path().join("state");
    fs::create_dir_all(project.join("pack/mods")).unwrap();
    fs::create_dir_all(project.join("templates")).unwrap();
    fs::create_dir_all(project.join("dist")).unwrap();
    fs::write(project.join("empack.yml"), "broken: [").unwrap();
    fs::write(project.join("pack/mods/stale.jar"), "old bytes").unwrap();
    for name in ["README", "templates/user.template", "dist/previous.zip"] {
        fs::write(project.join(name), "unrelated").unwrap();
    }
    let cancel = Cancellation::default();
    let reader = ProjectReader::new(RecoveryReader::new(host.clone()));
    let captured = reader
        .capture_replacement(&project, SnapshotLimits::default(), &cancel)
        .unwrap();
    let rejected = prepare_import_replacement(
        captured,
        candidate().await,
        ImportReplacementPolicy::RejectExisting,
        &cancel,
    );
    assert!(matches!(rejected, Err(error) if error.to_string().contains("explicit decision")));
    let captured = reader
        .capture_replacement(&project, SnapshotLimits::default(), &cancel)
        .unwrap();
    let prepared = prepare_import_replacement(
        captured,
        candidate().await,
        ImportReplacementPolicy::ReplaceManagedContent,
        &cancel,
    )
    .unwrap();
    assert!(!host.exists());
    assert_eq!(
        fs::read_to_string(project.join("empack.yml")).unwrap(),
        "broken: ["
    );
    assert_eq!(
        fs::read_to_string(project.join("pack/mods/stale.jar")).unwrap(),
        "old bytes"
    );
    assert!(!project.join("overrides").exists());
    assert!(prepared.plan().changes().iter().any(|change| matches!(change, empack_core::files::FileChange::Remove { target: ManagedPath::Content { path, .. }, .. } if path.as_str() == "mods/stale.jar")));
    let receipt = prepared
        .publish(&Publisher::open(&host).unwrap(), &cancel)
        .unwrap();
    assert!(!project.join("pack/mods/stale.jar").exists());
    assert_eq!(
        fs::read(project.join("overrides/common/config/a")).unwrap(),
        b"shared"
    );
    assert_eq!(
        fs::read(project.join("overrides/client/config/a")).unwrap(),
        b"client"
    );
    assert_eq!(
        fs::read(project.join("overrides/server/config/a")).unwrap(),
        b"server"
    );
    for name in ["README", "templates/user.template", "dist/previous.zip"] {
        assert_eq!(fs::read_to_string(project.join(name)).unwrap(), "unrelated");
    }
    let workspace = reader
        .capture_build(&project, &[], SnapshotLimits::default(), &cancel)
        .unwrap();
    assert_eq!(
        workspace.require_resolved().unwrap().lock(),
        receipt.project.lock()
    );
    let export = prepare_mrpack(
        &workspace,
        &BuildAcquisitions::default(),
        SourceEvidencePolicy::Compatibility,
        OptionalConversion::RejectMetadataLoss,
        &cancel,
    )
    .unwrap();
    let mut output = tempfile::tempfile().unwrap();
    export.write(&mut output, &cancel).unwrap();
    let mut zip = zip::ZipArchive::new(output).unwrap();
    for (name, expected) in [
        ("client-overrides/config/a", "client"),
        ("server-overrides/config/a", "server"),
    ] {
        let mut bytes = String::new();
        zip.by_name(name)
            .unwrap()
            .read_to_string(&mut bytes)
            .unwrap();
        assert_eq!(bytes, expected);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_ne!(
            fs::metadata(project.join("overrides/common/config/a"))
                .unwrap()
                .permissions()
                .mode()
                & 0o100,
            0
        );
    }
}
#[tokio::test]
async fn source_changes_after_preparation_block_the_whole_replacement() {
    for change in ["document", "membership", "policy", "cancel"] {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        let host = temp.path().join("state");
        fs::create_dir_all(project.join("pack")).unwrap();
        fs::write(project.join("empack.yml"), "old").unwrap();
        fs::write(project.join("pack/old.txt"), "old content").unwrap();
        let cancel = Cancellation::default();
        let reader = ProjectReader::new(RecoveryReader::new(host.clone()));
        let captured = reader
            .capture_replacement(&project, SnapshotLimits::default(), &cancel)
            .unwrap();
        let prepared = prepare_import_replacement(
            captured,
            candidate().await,
            ImportReplacementPolicy::ReplaceManagedContent,
            &cancel,
        )
        .unwrap();
        match change {
            "document" => fs::write(project.join("empack.yml"), "edited").unwrap(),
            "membership" => fs::write(project.join("pack/new.txt"), "new").unwrap(),
            "policy" => fs::write(project.join("pack/.packwizignore"), "*.jar").unwrap(),
            _ => cancel.cancel(),
        }
        assert!(
            prepared
                .publish(&Publisher::open(&host).unwrap(), &cancel)
                .is_err(),
            "{change}"
        );
        assert_eq!(
            fs::read_to_string(project.join("pack/old.txt")).unwrap(),
            "old content"
        );
        assert!(!project.join("empack.lock").exists());
        assert!(!project.join("overrides").exists());
    }
}
#[tokio::test]
async fn empty_existing_directory_can_initialize_without_a_manifest_or_backend() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let host = temp.path().join("state");
    fs::create_dir(&project).unwrap();
    let cancel = Cancellation::default();
    let reader = ProjectReader::new(RecoveryReader::new(host.clone()));
    let captured = reader
        .capture_replacement(&project, SnapshotLimits::default(), &cancel)
        .unwrap();
    let prepared = prepare_import_replacement(
        captured,
        candidate().await,
        ImportReplacementPolicy::RejectExisting,
        &cancel,
    )
    .unwrap();
    assert_eq!(fs::read_dir(&project).unwrap().count(), 0);
    prepared
        .publish(&Publisher::open(&host).unwrap(), &cancel)
        .unwrap();
    assert!(
        reader
            .capture_build(&project, &[], SnapshotLimits::default(), &cancel)
            .unwrap()
            .require_resolved()
            .is_ok()
    );
}
#[cfg(unix)]
#[test]
fn replacement_capture_rejects_symlink_ancestors_without_touching_outside_files() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let outside = temp.path().join("outside");
    fs::create_dir_all(project.join("pack")).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("sentinel"), "untouched").unwrap();
    std::os::unix::fs::symlink(&outside, project.join("pack/config")).unwrap();
    let reader = ProjectReader::new(RecoveryReader::new(temp.path().join("state")));
    assert!(
        reader
            .capture_replacement(
                &project,
                SnapshotLimits::default(),
                &Cancellation::default()
            )
            .is_err()
    );
    assert_eq!(
        fs::read_to_string(outside.join("sentinel")).unwrap(),
        "untouched"
    );
    assert!(!temp.path().join("state").exists());
}

#[tokio::test]
async fn ignored_pack_files_and_policy_are_not_owned_by_managed_replacement() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let host = temp.path().join("state");
    fs::create_dir_all(project.join("pack/private")).unwrap();
    fs::write(project.join("empack.yml"), "old").unwrap();
    fs::write(project.join("pack/backup.zip"), "personal backup").unwrap();
    fs::write(project.join("pack/private/notes"), "private notes").unwrap();
    fs::write(project.join("pack/.packwizignore"), "private/\n").unwrap();
    fs::write(project.join("pack/stale.jar"), "managed").unwrap();
    let cancel = Cancellation::default();
    let reader = ProjectReader::new(RecoveryReader::new(host.clone()));
    let captured = reader
        .capture_replacement(&project, SnapshotLimits::default(), &cancel)
        .unwrap();
    let prepared = prepare_import_replacement(
        captured,
        candidate().await,
        ImportReplacementPolicy::ReplaceManagedContent,
        &cancel,
    )
    .unwrap();
    prepared
        .publish(&Publisher::open(&host).unwrap(), &cancel)
        .unwrap();
    assert_eq!(
        fs::read_to_string(project.join("pack/backup.zip")).unwrap(),
        "personal backup"
    );
    assert_eq!(
        fs::read_to_string(project.join("pack/private/notes")).unwrap(),
        "private notes"
    );
    assert_eq!(
        fs::read_to_string(project.join("pack/.packwizignore")).unwrap(),
        "private/\n"
    );
    assert!(!project.join("pack/stale.jar").exists());
}
#[cfg(unix)]
#[test]
fn ignored_payloads_are_skipped_before_opening_or_rejecting_links() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    fs::create_dir_all(project.join("pack/private")).unwrap();
    fs::write(project.join("pack/.packwizignore"), "private/\n").unwrap();
    fs::write(project.join("pack/private/large"), vec![0; 4096]).unwrap();
    std::os::unix::fs::symlink(
        temp.path().join("missing-outside"),
        project.join("pack/backup.zip"),
    )
    .unwrap();
    let reader = ProjectReader::new(RecoveryReader::new(temp.path().join("state")));
    let limits = SnapshotLimits {
        file_bytes: 1024,
        total_bytes: 2048,
        ..Default::default()
    };
    assert!(
        reader
            .capture_replacement(&project, limits, &Cancellation::default())
            .is_ok()
    );
}

#[tokio::test]
async fn incoming_ignored_destination_needs_actual_absence_and_cannot_replace_unowned_bytes() {
    use crate::engine::import::acquisition::tests::remote;
    for existing in [false, true] {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/payload")
            .with_body("payload")
            .create_async()
            .await;
        let archive = source(
            "modrinth.index.json",
            mr(vec![remote(
                format!("{}/payload", server.url()),
                "backup.zip",
                b"payload",
            )]),
            &[],
        );
        let outcome = interpret(archive, server.url(), |_| {}).await;
        let Some(OperationOutcome::Completed(Ok(candidate))) = Arc::into_inner(outcome) else {
            panic!("missing candidate");
        };
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        let host = temp.path().join("state");
        fs::create_dir_all(project.join("pack")).unwrap();
        if existing {
            fs::write(project.join("pack/backup.zip"), "personal backup").unwrap();
        }
        let cancel = Cancellation::default();
        let snapshot = ProjectReader::new(RecoveryReader::new(host.clone()))
            .capture_replacement(&project, SnapshotLimits::default(), &cancel)
            .unwrap();
        let prepared = prepare_import_replacement(
            snapshot,
            candidate,
            ImportReplacementPolicy::ReplaceManagedContent,
            &cancel,
        );
        if existing {
            assert!(
                matches!(prepared, Err(error) if error.to_string().contains("not captured as owned"))
            );
            assert_eq!(
                fs::read_to_string(project.join("pack/backup.zip")).unwrap(),
                "personal backup"
            );
            assert!(!project.join("empack.yml").exists());
        } else {
            prepared
                .unwrap()
                .publish(&Publisher::open(&host).unwrap(), &cancel)
                .unwrap();
            assert_eq!(
                fs::read_to_string(project.join("pack/backup.zip")).unwrap(),
                "payload"
            );
        }
    }
}
