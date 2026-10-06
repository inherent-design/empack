use super::*;
pub(super) fn expected() -> ExpectedContent {
    ExpectedContent {
        digests: Some(DigestSet::parse([("md5", "321c3cf486ed509164edec1e1981fec8")]).unwrap()),
        size: Some(7),
        accepted_observation: None,
    }
}
pub(super) fn acquire(bytes: &[u8], expected: &ExpectedContent) -> Result<AcquiredContent> {
    verify_stream(
        &mut &bytes[..],
        expected,
        16,
        SourceEvidencePolicy::Compatibility,
        InitialObservation::RequireEvidence,
        &Cancellation::default(),
    )
}
#[test]
fn weak_source_evidence_stays_weak_and_every_assertion_is_required() {
    let acquired = acquire(b"payload", &expected()).unwrap();
    assert!(
        matches!(acquired.evidence(), IntegrityEvidence::MatchedExpected { expected, .. } if expected.strongest() == DigestAlgorithm::Md5)
    );
    assert_eq!(
        acquired.observed_digests().strongest(),
        DigestAlgorithm::Sha512
    );
    assert!(
        verify_stream(
            &mut &b"payload"[..],
            &expected(),
            16,
            SourceEvidencePolicy::StrongSourceRequired,
            InitialObservation::Accepted,
            &Cancellation::default()
        )
        .is_err()
    );
    let mut conflicting = expected();
    conflicting.digests = Some(
        DigestSet::new(vec![
            ExpectedDigest::Sha256(*acquired.lease().id().bytes()),
            ExpectedDigest::Md5([0; 16]),
        ])
        .unwrap(),
    );
    assert!(acquire(b"payload", &conflicting).is_err());
    assert!(acquire(b"changed", &expected()).is_err());
}
#[test]
fn lease_readers_have_independent_positions_and_retain_verified_bytes() {
    let acquired = acquire(b"payload", &expected()).unwrap();
    let lease = acquired.lease().clone();
    let mut first = lease.open();
    let mut second = lease.open();
    drop(acquired);
    drop(lease);
    let mut bytes = [0; 3];
    first.read_exact(&mut bytes).unwrap();
    assert_eq!(&bytes, b"pay");
    second.seek(SeekFrom::Start(4)).unwrap();
    second.read_exact(&mut bytes).unwrap();
    assert_eq!(&bytes, b"oad");
    first.read_exact(&mut bytes).unwrap();
    assert_eq!(&bytes, b"loa");
    assert!(first.seek(SeekFrom::Current(-20)).is_err());
    first.seek(SeekFrom::End(20)).unwrap();
    assert_eq!(first.read(&mut bytes).unwrap(), 0);
}
#[test]
fn size_bounds_cancellation_and_initial_observation_are_enforced() {
    assert!(
        verify_stream(
            &mut &b"payload"[..],
            &expected(),
            6,
            SourceEvidencePolicy::Compatibility,
            InitialObservation::Accepted,
            &Cancellation::default()
        )
        .is_err()
    );
    let mut wrong_size = expected();
    wrong_size.size = Some(6);
    assert!(acquire(b"payload", &wrong_size).is_err());
    let cancel = Cancellation::default();
    cancel.cancel();
    assert!(
        verify_stream(
            &mut &b"payload"[..],
            &expected(),
            16,
            SourceEvidencePolicy::Compatibility,
            InitialObservation::Accepted,
            &cancel
        )
        .is_err()
    );
    let unknown = ExpectedContent {
        digests: None,
        size: None,
        accepted_observation: None,
    };
    assert!(acquire(b"payload", &unknown).is_err());
    let acquired = verify_stream(
        &mut &b"payload"[..],
        &unknown,
        16,
        SourceEvidencePolicy::Compatibility,
        InitialObservation::Accepted,
        &Cancellation::default(),
    )
    .unwrap();
    assert!(matches!(
        acquired.evidence(),
        IntegrityEvidence::ObservedOnly { .. }
    ));
    let recorded = ExpectedContent {
        accepted_observation: Some(acquired.lease().id()),
        ..unknown
    };
    assert!(acquire(b"payload", &recorded).is_ok());
    assert!(acquire(b"changed", &recorded).is_err());
    let mut bytes = Vec::new();
    acquired
        .lease()
        .copy_verified(&mut bytes, &Cancellation::default())
        .unwrap();
    assert_eq!(bytes, b"payload");
}

#[test]
fn hundreds_of_independent_files_fit_a_normal_descriptor_limit() {
    const CHILD: &str = "EMPACK_CONTENT_POOL_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let executable = std::env::current_exe().unwrap();
        let name =
            "engine::content::tests::hundreds_of_independent_files_fit_a_normal_descriptor_limit";
        #[cfg(unix)]
        let mut command = {
            let mut command = std::process::Command::new("/bin/sh");
            command.args([
                "-c",
                "ulimit -n 256; exec \"$1\" --exact \"$2\" --nocapture",
                "empack-content-fixture",
            ]);
            command.arg(executable).arg(name);
            command
        };
        #[cfg(not(unix))]
        let mut command = {
            let mut command = std::process::Command::new(executable);
            command.args(["--exact", name, "--nocapture"]);
            command
        };
        let output = command.env(CHILD, "1").output().unwrap();
        assert!(
            output.status.success(),
            "descriptor fixture failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let mut pool = ContentPool::new(64 * 600).unwrap();
    let mut retained = Vec::new();
    for index in 0..600 {
        let data = format!("independent payload {index}").into_bytes();
        let expected = ExpectedContent {
            digests: None,
            size: Some(data.len() as u64),
            accepted_observation: None,
        };
        let content = verify_stream(
            &mut data.as_slice(),
            &expected,
            64,
            SourceEvidencePolicy::Compatibility,
            InitialObservation::Accepted,
            &Cancellation::default(),
        )
        .unwrap();
        retained.push(pool.insert(content, &Cancellation::default()).unwrap());
    }
    drop(pool);
    for (index, value) in retained.into_iter().enumerate() {
        let mut output = Vec::new();
        value
            .lease()
            .copy_verified(&mut output, &Cancellation::default())
            .unwrap();
        assert_eq!(output, format!("independent payload {index}").as_bytes());
    }
}
