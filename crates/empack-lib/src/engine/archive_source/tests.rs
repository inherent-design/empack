use super::*;
use empack_core::digest::{DigestSet, ExpectedDigest};
use std::io::{Cursor, Write};
fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        zip.start_file(
            *name,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored)
                .unix_permissions(0o755),
        )
        .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn content(bytes: &[u8]) -> AcquiredContent {
    verify_stream(
        &mut &bytes[..],
        &ExpectedContent {
            digests: None,
            size: None,
            accepted_observation: None,
        },
        bytes.len() as u64,
        SourceEvidencePolicy::Compatibility,
        InitialObservation::Accepted,
        &Cancellation::default(),
    )
    .unwrap()
}
fn expected() -> ExpectedContent {
    ExpectedContent {
        digests: Some(
            DigestSet::new(vec![
                ExpectedDigest::parse("md5", "321c3cf486ed509164edec1e1981fec8").unwrap(),
            ])
            .unwrap(),
        ),
        size: Some(7),
        accepted_observation: None,
    }
}
fn path(value: &str) -> PortableRelPath {
    PortableRelPath::parse(value, PathSyntax::ArchiveMember).unwrap()
}
#[test]
fn reads_verified_members_with_portable_attributes_and_reuses_the_archive() {
    let bytes = content(&archive(&[
        ("asset.bin", b"payload"),
        ("other.bin", b"payload"),
    ]));
    let cancel = Cancellation::default();
    let mut zip = ZipContentSource::open(&bytes, ArchiveLimits::default(), &cancel).unwrap();
    drop(bytes);
    assert_eq!(zip.files().count(), 2);
    for member in ["asset.bin", "other.bin"] {
        let (content, permissions) = zip
            .acquire(
                &path(member),
                &expected(),
                SourceEvidencePolicy::Compatibility,
                InitialObservation::RequireEvidence,
                &cancel,
            )
            .unwrap();
        assert!(permissions.executable);
        let mut bytes = Vec::new();
        content.lease().open().read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"payload");
    }
    assert!(
        zip.acquire(
            &path("missing"),
            &expected(),
            SourceEvidencePolicy::Compatibility,
            InitialObservation::RequireEvidence,
            &cancel
        )
        .is_err()
    );
}
#[test]
fn metadata_and_raw_duplicate_checks_cover_unselected_members_before_extraction() {
    let original = archive(&[("asset.bin", b"payload"), ("other.bin", b"payload")]);
    let mut duplicate = original.clone();
    for index in 0..duplicate.len() - 8 {
        if duplicate[index..].starts_with(b"other.bin") {
            duplicate[index..index + 9].copy_from_slice(b"asset.bin");
        }
    }
    for bytes in [
        duplicate,
        archive(&[("asset.bin", b"payload"), ("../escape", b"x")]),
        archive(&[("asset.bin", b"payload"), ("ASSET.bin", b"x")]),
        archive(&[("asset.bin", b"payload"), ("asset.bin/child", b"x")]),
    ] {
        assert!(
            ZipContentSource::open(
                &content(&bytes),
                ArchiveLimits::default(),
                &Cancellation::default()
            )
            .is_err()
        );
    }
    for limits in [
        ArchiveLimits {
            compressed_bytes: 1,
            ..ArchiveLimits::default()
        },
        ArchiveLimits {
            entries: 1,
            ..ArchiveLimits::default()
        },
        ArchiveLimits {
            file_bytes: 6,
            ..ArchiveLimits::default()
        },
        ArchiveLimits {
            total_bytes: 13,
            ..ArchiveLimits::default()
        },
    ] {
        assert!(
            ZipContentSource::open(&content(&original), limits, &Cancellation::default()).is_err()
        );
    }
}
#[test]
fn failed_digest_and_crc_reads_never_return_content_or_reset_the_extraction_allowance() {
    let bytes = archive(&[("asset.bin", b"payload")]);
    let limits = ArchiveLimits {
        total_bytes: 7,
        ..ArchiveLimits::default()
    };
    let cancel = Cancellation::default();
    let mut zip = ZipContentSource::open(&content(&bytes), limits, &cancel).unwrap();
    let mut wrong = expected();
    wrong.digests = Some(DigestSet::new(vec![ExpectedDigest::Md5([0; 16])]).unwrap());
    assert!(
        zip.acquire(
            &path("asset.bin"),
            &wrong,
            SourceEvidencePolicy::Compatibility,
            InitialObservation::RequireEvidence,
            &cancel
        )
        .is_err()
    );
    assert!(
        zip.acquire(
            &path("asset.bin"),
            &expected(),
            SourceEvidencePolicy::Compatibility,
            InitialObservation::RequireEvidence,
            &cancel
        )
        .is_err()
    );
    let mut corrupt = bytes;
    let offset = corrupt
        .windows(7)
        .position(|bytes| bytes == b"payload")
        .unwrap();
    corrupt[offset] ^= 1;
    let mut zip = ZipContentSource::open(&content(&corrupt), limits, &cancel).unwrap();
    assert!(
        zip.acquire(
            &path("asset.bin"),
            &expected(),
            SourceEvidencePolicy::Compatibility,
            InitialObservation::RequireEvidence,
            &cancel
        )
        .is_err()
    );
}

#[test]
fn linked_and_nonportable_directory_members_are_refused_without_panicking() {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.add_symlink(
        "link",
        "../outside",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    let link = zip.finish().unwrap().into_inner();
    assert!(
        ZipContentSource::open(
            &content(&link),
            ArchiveLimits::default(),
            &Cancellation::default()
        )
        .is_err()
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.add_directory("dir/", zip::write::SimpleFileOptions::default())
        .unwrap();
    let mut directory = zip.finish().unwrap().into_inner();
    for index in 0..directory.len() - 3 {
        if directory[index..].starts_with(b"dir/") {
            directory[index + 3] = b'\\';
        }
    }
    assert!(
        ZipContentSource::open(
            &content(&directory),
            ArchiveLimits::default(),
            &Cancellation::default()
        )
        .is_err()
    );
}

#[test]
fn whole_archive_verification_checks_crc_and_consumes_one_bounded_read_pass() {
    let cancel = Cancellation::default();
    let bytes = archive(&[("one", b"payload"), ("two", b"second")]);
    let limits = ArchiveLimits {
        total_bytes: 13,
        ..Default::default()
    };
    let mut zip = ZipContentSource::open(&content(&bytes), limits, &cancel).unwrap();
    zip.verify_members(&cancel).unwrap();
    assert!(
        zip.verify_members(&cancel).is_err(),
        "another full read cannot reset the allowance"
    );
    let mut damaged = bytes.clone();
    let index = damaged.windows(6).position(|b| b == b"second").unwrap();
    damaged[index] ^= 1;
    let mut zip = ZipContentSource::open(&content(&damaged), limits, &cancel).unwrap();
    assert!(zip.verify_members(&cancel).is_err());
    assert_eq!(
        zip.extracted, 13,
        "CRC failure charges the failed member too"
    );
    assert!(zip.verify_members(&cancel).is_err());
    let mut zip = ZipContentSource::open(&content(&bytes), limits, &cancel).unwrap();
    cancel.cancel();
    assert!(zip.verify_members(&cancel).is_err());
    assert_eq!(zip.extracted, 0);
}

#[test]
fn member_observations_keep_the_original_container_assurance_without_digest_upgrades() {
    use empack_core::{digest::IntegrityEvidence, identity::*, model::*};
    use sha2::Digest;
    let bytes = archive(&[("World/level.dat", b"payload"), ("World/other", b"payload")]);
    let strong = DigestSet::new(vec![ExpectedDigest::Sha256(
        sha2::Sha256::digest(&bytes).into(),
    )])
    .unwrap();
    let weak = DigestSet::new(vec![ExpectedDigest::Md5(md5::Md5::digest(&bytes).into())]).unwrap();
    let cancel = Cancellation::default();
    for assertions in [strong, weak] {
        let expected_archive = ExpectedContent {
            digests: Some(assertions.clone()),
            size: Some(bytes.len() as u64),
            accepted_observation: None,
        };
        let content = verify_stream(
            &mut bytes.as_slice(),
            &expected_archive,
            bytes.len() as u64,
            SourceEvidencePolicy::Compatibility,
            InitialObservation::RequireEvidence,
            &cancel,
        )
        .unwrap();
        let mut source =
            ZipContentSource::open(&content, ArchiveLimits::default(), &cancel).unwrap();
        let source = &mut source;
        let expected_member = ExpectedContent {
            digests: None,
            size: Some(7),
            accepted_observation: None,
        };
        let strong = assertions.strongest() == empack_core::digest::DigestAlgorithm::Sha256;
        let result = source.acquire(
            &path("World/level.dat"),
            &expected_member,
            SourceEvidencePolicy::StrongSourceRequired,
            InitialObservation::Accepted,
            &cancel,
        );
        assert_eq!(result.is_ok(), strong);
        let member = if strong {
            result.unwrap().0
        } else {
            source
                .acquire(
                    &path("World/level.dat"),
                    &expected_member,
                    SourceEvidencePolicy::Compatibility,
                    InitialObservation::Accepted,
                    &cancel,
                )
                .unwrap()
                .0
        };
        assert!(matches!(
            member.evidence(),
            IntegrityEvidence::ObservedOnly { .. }
        ));
        let source = ProviderArchiveSource {
            pin: ResolvedPin {
                project: ProviderProjectId::CurseForge(CurseForgeProjectId::parse("123").unwrap()),
                selection: PinSelector::CurseForgeFile(CurseForgeFileId::parse("456").unwrap()),
            },
            slot: FileSlot::parse("world.zip").unwrap(),
            expected: expected_archive,
            alternatives: vec![],
        };
        assert_eq!(
            member
                .provider_member_policy(
                    &source,
                    &path("World/level.dat"),
                    SourceEvidencePolicy::StrongSourceRequired
                )
                .is_ok(),
            strong
        );
        assert!(
            member
                .provider_member_policy(
                    &source,
                    &path("World/other"),
                    SourceEvidencePolicy::StrongSourceRequired
                )
                .is_err()
        );
        // Re-reading the same member bytes does not reconstruct private extraction evidence.
        let standalone = super::tests::content(b"payload");
        assert!(
            standalone
                .provider_member_policy(
                    &source,
                    &path("World/level.dat"),
                    SourceEvidencePolicy::StrongSourceRequired
                )
                .is_err()
        );
        let mut pool = crate::engine::content::ContentPool::new(7).unwrap();
        let retained = pool.insert(member, &cancel).unwrap();
        assert_eq!(
            retained
                .provider_member_policy(
                    &source,
                    &path("World/level.dat"),
                    SourceEvidencePolicy::StrongSourceRequired
                )
                .is_ok(),
            strong
        );
    }
}
