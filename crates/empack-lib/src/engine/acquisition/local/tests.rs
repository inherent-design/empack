use super::*;
use crate::{
    application::process_runtime::Cancellation,
    engine::{
        resources::ResourceGovernor,
        runtime::{OperationOutcome, OperationRuntime},
    },
};
use empack_core::digest::{DigestSet, ExpectedDigest, IntegrityEvidence};
use std::{fs, io::Read, sync::Arc};

fn request(source: PathBuf) -> LocalFileRequest {
    LocalFileRequest {
        source,
        expected: ExpectedContent {
            digests: None,
            size: None,
            accepted_observation: None,
        },
        maximum: 8 << 30,
        evidence: SourceEvidencePolicy::Compatibility,
        initial: InitialObservation::Accepted,
    }
}
async fn run(
    request: LocalFileRequest,
) -> (
    Arc<OperationOutcome<Result<AcquiredBuildFile>>>,
    ResourceGovernor,
) {
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 1,
        memory_bytes: 1 << 20,
        scratch_bytes: 16,
        open_files: 12,
    });
    let runtime = OperationRuntime::new(governor.clone(), 1);
    let mut handle = runtime
        .start(move |mut scope| async move { Ok(acquire_local_file(&mut scope, request).await) })
        .unwrap();
    let outcome = handle.wait().await;
    runtime.release_completed(handle.id());
    runtime.shutdown().await;
    drop(handle);
    (outcome, governor)
}
fn failed(outcome: &OperationOutcome<Result<AcquiredBuildFile>>) -> String {
    match outcome {
        OperationOutcome::Completed(Err(error)) => format!("{error:#}"),
        _ => panic!("expected acquisition refusal"),
    }
}
#[tokio::test]
async fn selected_native_file_uses_actual_size_and_preserves_observation_and_permissions() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("selected.bin");
    fs::write(&source, b"payload").unwrap();
    let mut permissions = fs::metadata(&source).unwrap().permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o555);
    }
    permissions.set_readonly(true);
    fs::set_permissions(&source, permissions).unwrap();
    // An unrelated directory and dangling link cannot broaden this selected read.
    fs::create_dir(root.path().join("unrelated")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("missing", root.path().join("unrelated-link")).unwrap();
    let (outcome, governor) = run(request(source.clone())).await;
    let OperationOutcome::Completed(Ok(file)) = &*outcome else {
        panic!("{}", failed(&outcome));
    };
    assert!(matches!(
        file.content.evidence(),
        IntegrityEvidence::ObservedOnly { .. }
    ));
    assert!(file.permissions.readonly);
    #[cfg(unix)]
    assert!(file.permissions.executable);
    assert_eq!(governor.status().reserved.scratch_bytes, 7);
    let mut reader = file.content.lease().open();
    let mut bytes = vec![];
    reader.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"payload");
    assert_eq!(fs::read(&source).unwrap(), b"payload");
    drop(outcome);
    assert_eq!(governor.status().reserved.scratch_bytes, 7);
    drop(reader);
    assert_eq!(governor.status().reserved.scratch_bytes, 0);
    assert_eq!(governor.status().reserved.open_files, 0);
}
#[tokio::test]
async fn local_reads_enforce_source_evidence_limits_and_regular_file_selection() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("file");
    fs::write(&source, b"payload").unwrap();
    for mode in [
        "limit",
        "size",
        "digest",
        "decision",
        "strong",
        "directory",
        "relative",
    ] {
        let mut input = request(source.clone());
        match mode {
            "limit" => input.maximum = 3,
            "size" => input.expected.size = Some(3),
            "digest" => {
                input.expected.digests = Some(
                    DigestSet::new(vec![
                        ExpectedDigest::parse("md5", "00000000000000000000000000000000").unwrap(),
                    ])
                    .unwrap(),
                )
            }
            "decision" => input.initial = InitialObservation::RequireEvidence,
            "strong" => input.evidence = SourceEvidencePolicy::StrongSourceRequired,
            "directory" => input.source = root.path().join("folder"),
            "relative" => input.source = "file".into(),
            _ => unreachable!(),
        }
        if mode == "directory" {
            fs::create_dir(&input.source).unwrap();
        }
        let (outcome, governor) = run(input).await;
        assert!(!failed(&outcome).is_empty(), "{mode}");
        assert_eq!(governor.status().reserved.scratch_bytes, 0);
        assert_eq!(governor.status().reserved.open_files, 0);
    }
    let mut input = request(source);
    input.expected.digests = Some(
        DigestSet::new(vec![
            ExpectedDigest::parse("md5", "321c3cf486ed509164edec1e1981fec8").unwrap(),
        ])
        .unwrap(),
    );
    input.initial = InitialObservation::RequireEvidence;
    let (outcome, _) = run(input).await;
    let OperationOutcome::Completed(Ok(file)) = &*outcome else {
        panic!("valid source digest rejected");
    };
    assert!(matches!(
        file.content.evidence(),
        IntegrityEvidence::MatchedExpected { .. }
    ));
}
#[test]
fn source_replacement_or_same_size_edit_between_capture_and_copy_is_rejected() {
    for replacement in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("file");
        fs::write(&source, b"payload").unwrap();
        let captured = SelectedFile::capture(source.clone(), 16, &Cancellation::default()).unwrap();
        if replacement {
            fs::rename(&source, root.path().join("old")).unwrap();
        }
        fs::write(&source, b"changed").unwrap();
        assert!(
            captured
                .verify(request(source), &Cancellation::default())
                .is_err()
        );
    }
}
#[cfg(unix)]
#[tokio::test]
async fn selected_symlink_and_fifo_are_rejected_without_reading_the_target() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("sentinel");
    fs::write(&target, b"payload").unwrap();
    let link = root.path().join("link");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let (outcome, _) = run(request(link)).await;
    assert!(!failed(&outcome).is_empty());
    let fifo = root.path().join("fifo");
    let status = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap();
    assert!(status.success());
    let (outcome, _) = run(request(fifo)).await;
    assert!(!failed(&outcome).is_empty());
    assert_eq!(fs::read(target).unwrap(), b"payload");
}
