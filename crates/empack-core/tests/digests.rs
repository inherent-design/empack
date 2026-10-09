use empack_core::digest::*;

#[test]
fn declarations_reject_missing_unknown_malformed_and_conflicting_digests() {
    assert_eq!(DigestSet::parse([]), Err(DigestError::Empty));
    for (algorithm, width) in [("md5", 32), ("sha1", 40), ("sha256", 64), ("sha512", 128)] {
        let valid = "a".repeat(width);
        let other = "b".repeat(width);
        assert!(DigestSet::parse([(algorithm, valid.as_str())]).is_ok());
        for malformed in [
            "a".repeat(width - 1),
            "g".repeat(width),
            "a".repeat(width + 1),
        ] {
            assert!(DigestSet::parse([(algorithm, malformed.as_str())]).is_err());
        }
        assert!(matches!(
            DigestSet::parse([(algorithm, valid.as_str()), (algorithm, other.as_str())]),
            Err(DigestError::Conflict(_))
        ));
        assert_eq!(
            DigestSet::parse([
                (algorithm, valid.as_str()),
                (algorithm, valid.to_uppercase().as_str())
            ])
            .unwrap()
            .values()
            .len(),
            1
        );
    }
    assert!(matches!(
        DigestSet::parse([("sha257", "a")]),
        Err(DigestError::UnsupportedAlgorithm(_))
    ));
}

#[test]
fn every_declaration_must_match_even_when_a_stronger_one_matches() {
    let expected = DigestSet::parse([
        ("md5", "00000000000000000000000000000000"),
        (
            "sha256",
            "0000000000000000000000000000000000000000000000000000000000000000",
        ),
    ])
    .unwrap();
    assert_eq!(expected.strongest(), DigestAlgorithm::Sha256);
    assert!(expected.check(&[ExpectedDigest::Sha256([0; 32])]).is_err());
    assert!(
        expected
            .check(&[
                ExpectedDigest::Sha256([0; 32]),
                ExpectedDigest::Md5([1; 16])
            ])
            .is_err()
    );
    assert!(
        expected
            .check(&[
                ExpectedDigest::Sha256([0; 32]),
                ExpectedDigest::Md5([0; 16])
            ])
            .is_ok()
    );
    assert!(
        expected
            .check(&[
                ExpectedDigest::Sha256([0; 32]),
                ExpectedDigest::Md5([0; 16]),
                ExpectedDigest::Md5([1; 16])
            ])
            .is_err()
    );
}

#[test]
fn content_address_does_not_upgrade_source_evidence() {
    let expected = DigestSet::parse([("md5", "00000000000000000000000000000000")]).unwrap();
    let evidence = IntegrityEvidence::MatchedExpected {
        expected,
        actual: ContentId::from_sha256([42; 32]),
    };
    let IntegrityEvidence::MatchedExpected { expected, actual } = evidence else {
        unreachable!()
    };
    assert_eq!(expected.strongest(), DigestAlgorithm::Md5);
    assert_eq!(actual.bytes(), &[42; 32]);
}
