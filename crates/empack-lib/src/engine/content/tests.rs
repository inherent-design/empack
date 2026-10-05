use super::*;
fn expected() -> ExpectedContent {
    ExpectedContent {
        digests: Some(DigestSet::parse([("md5", "321c3cf486ed509164edec1e1981fec8")]).unwrap()),
        size: Some(7),
        accepted_observation: None,
    }
}
fn acquire(bytes: &[u8], expected: &ExpectedContent) -> Result<AcquiredContent> {
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
