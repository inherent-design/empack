use super::*;
use crate::engine::mrpack::LockedFileKey;
use crate::engine::{
    resources::ResourceGovernor,
    runtime::{OperationOutcome, OperationRuntime},
};
use empack_core::{
    digest::{DigestSet, ExpectedDigest},
    model::{DependencyKey, FileSlot},
};
use sha2::Digest;
use std::fs;

fn key(name: &str) -> AcquisitionKey {
    AcquisitionKey::Locked(LockedFileKey {
        dependency: DependencyKey::parse(name).unwrap(),
        slot: FileSlot::parse("main").unwrap(),
    })
}
fn expected(bytes: &[u8]) -> ExpectedContent {
    ExpectedContent {
        digests: Some(
            DigestSet::new(vec![ExpectedDigest::Sha256(
                sha2::Sha256::digest(bytes).into(),
            )])
            .unwrap(),
        ),
        size: Some(bytes.len() as u64),
        accepted_observation: None,
    }
}
fn limits() -> DiscoveryLimits {
    DiscoveryLimits {
        entries: 32,
        requirements: 16,
        file_bytes: 1024,
        total_bytes: 8192,
        ..Default::default()
    }
}
#[test]
fn discovery_hashes_once_and_uses_assertions_instead_of_names() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("renamed.bin"), b"payload").unwrap();
    fs::write(root.path().join("expected.jar"), b"wrong!!").unwrap();
    fs::create_dir(root.path().join("directory.jar")).unwrap();
    fs::write(root.path().join("large"), vec![0; 1025]).unwrap();
    let expected_a = expected(b"payload");
    let mut wrong_size = expected_a.clone();
    wrong_size.size = Some(8);
    let mut wrong_secondary = expected_a.clone();
    wrong_secondary.digests = Some(
        DigestSet::new(vec![
            ExpectedDigest::Sha256(sha2::Sha256::digest(b"payload").into()),
            ExpectedDigest::Md5([0; 16]),
        ])
        .unwrap(),
    );
    let requirements = BTreeMap::from([
        (key("a"), expected_a.clone()),
        (key("b"), expected_a),
        (key("wrong-size"), wrong_size),
        (key("wrong-secondary"), wrong_secondary),
        (
            key("unknown"),
            ExpectedContent {
                digests: None,
                size: Some(7),
                accepted_observation: None,
            },
        ),
    ]);
    let result = scan(
        vec![root.path().to_owned(), root.path().to_owned()],
        requirements,
        SourceEvidencePolicy::Compatibility,
        limits(),
        &Cancellation::default(),
    )
    .unwrap();
    assert_eq!(result.inspected_files, 2);
    assert_eq!(result.bytes_read, 14);
    assert_eq!(result.skipped_files, 2);
    assert_eq!(
        result.unique_files(),
        BTreeMap::from([
            (key("a"), root.path().join("renamed.bin")),
            (key("b"), root.path().join("renamed.bin")),
        ])
    );
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 4);
}
#[test]
fn weak_evidence_and_prior_observation_keep_their_policy() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("one"), b"payload").unwrap();
    fs::write(root.path().join("two"), b"payload").unwrap();
    let weak = ExpectedContent {
        digests: Some(
            DigestSet::new(vec![
                ExpectedDigest::parse("md5", "321c3cf486ed509164edec1e1981fec8").unwrap(),
            ])
            .unwrap(),
        ),
        size: None,
        accepted_observation: None,
    };
    let prior = ExpectedContent {
        digests: None,
        size: None,
        accepted_observation: Some(ContentId::from_sha256(
            sha2::Sha256::digest(b"payload").into(),
        )),
    };
    for policy in [
        SourceEvidencePolicy::Compatibility,
        SourceEvidencePolicy::StrongSourceRequired,
    ] {
        let result = scan(
            vec![root.path().to_owned()],
            BTreeMap::from([(key("weak"), weak.clone()), (key("prior"), prior.clone())]),
            policy,
            limits(),
            &Cancellation::default(),
        )
        .unwrap();
        if policy == SourceEvidencePolicy::Compatibility {
            assert_eq!(result.matches.len(), 2);
            assert!(
                result
                    .matches
                    .values()
                    .all(|candidates| candidates.len() == 1)
            );
            assert!(
                result
                    .unique_files()
                    .values()
                    .all(|path| path == &root.path().join("one"))
            );
            assert_eq!(result.bytes_read, 14);
        } else {
            assert!(result.matches.is_empty());
            assert_eq!(result.bytes_read, 0);
        }
    }
}
#[test]
fn incomplete_scans_do_not_return_partial_associations() {
    let root = tempfile::tempdir().unwrap();
    for name in ["one", "two"] {
        fs::write(root.path().join(name), b"payload").unwrap();
    }
    for mode in ["bytes", "entries", "deadline", "cancelled", "associations"] {
        let mut bound = limits();
        let cancel = Cancellation::default();
        match mode {
            "bytes" => bound.total_bytes = 7,
            "entries" => bound.entries = 1,
            "deadline" => bound.deadline = Duration::ZERO,
            "cancelled" => cancel.cancel(),
            "associations" => bound.entries = 2,
            _ => unreachable!(),
        }
        assert!(
            scan(
                vec![root.path().to_owned()],
                BTreeMap::from([
                    (key("a"), expected(b"payload")),
                    (key("b"), expected(b"payload")),
                    (key("c"), expected(b"payload")),
                ]),
                SourceEvidencePolicy::Compatibility,
                bound,
                &cancel
            )
            .is_err(),
            "{mode}"
        );
    }
}
#[cfg(unix)]
#[test]
fn discovery_skips_links_and_special_files_without_following_or_blocking() {
    use std::os::unix::{ffi::OsStrExt, fs::symlink};
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("secret"), b"payload").unwrap();
    symlink(
        outside.path().join("secret"),
        root.path().join("linked.jar"),
    )
    .unwrap();
    fs::write(root.path().join("real"), b"payload").unwrap();
    fs::hard_link(root.path().join("real"), root.path().join("hard")).unwrap();
    let fifo = std::ffi::CString::new(root.path().join("pipe").as_os_str().as_bytes()).unwrap();
    // SAFETY: the temporary fixture owns the path; the C string is terminated.
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    let result = scan(
        vec![root.path().to_owned()],
        BTreeMap::from([(key("a"), expected(b"payload"))]),
        SourceEvidencePolicy::Compatibility,
        limits(),
        &Cancellation::default(),
    )
    .unwrap();
    assert_eq!(result.bytes_read, 7);
    assert_eq!(result.inspected_files, 1);
    assert_eq!(result.skipped_files, 2);
    assert_eq!(result.unique_files().len(), 1);
    assert_eq!(fs::read(outside.path().join("secret")).unwrap(), b"payload");
}
#[tokio::test]
async fn discovery_retires_admitted_workers_and_retains_bounded_results() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("renamed"), b"payload").unwrap();
    let path = root.path().to_owned();
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 1,
        memory_bytes: 2 << 20,
        open_files: 8,
        ..Default::default()
    });
    let runtime = OperationRuntime::new(governor.clone(), 1);
    let mut handle = runtime
        .start(move |mut scope| async move {
            Ok(discover_downloads(
                &mut scope,
                vec![path],
                BTreeMap::from([(key("a"), expected(b"payload"))]),
                SourceEvidencePolicy::Compatibility,
                limits(),
            )
            .await)
        })
        .unwrap();
    let result = handle.wait().await;
    let OperationOutcome::Completed(Ok(found)) = &*result else {
        panic!("discovery failed");
    };
    assert_eq!(found.unique_files().len(), 1);
    assert_eq!(governor.status().reserved.jobs, 0);
    assert!(governor.status().reserved.memory_bytes > 0);
    runtime.release_completed(handle.id());
    runtime.shutdown().await;
    drop((handle, result));
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}
