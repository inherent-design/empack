use super::*;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Cursor, Write},
};
fn path(name: &str) -> PortableRelPath {
    PortableRelPath::parse(name, PathSyntax::ArchiveMember).unwrap()
}
fn content(bytes: &[u8]) -> FileContent {
    FileContent {
        content: ContentId::from_sha256(Sha256::digest(bytes).into()),
        bytes: bytes.len() as u64,
        permissions: FilePermissions {
            readonly: false,
            executable: false,
        },
    }
}
#[test]
fn all_supported_containers_require_every_expected_file_and_exact_binary_bytes() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("source");
    fs::create_dir(&input).unwrap();
    fs::write(input.join("a.bin"), [0xff, 0, 1]).unwrap();
    let expected = BTreeMap::from([(path("a.bin"), content(&[0xff, 0, 1]))]);
    for (format, extension) in [
        (DistributionArchive::Zip, "zip"),
        (DistributionArchive::TarGz, "tar.gz"),
        (DistributionArchive::SevenZip, "7z"),
    ] {
        let output = temp.path().join(format!("out.{extension}"));
        let mut reader = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(output)
            .unwrap();
        package_directory(&input, &mut reader, format, &Cancellation::default()).unwrap();
        let verified = verify_archive(
            &mut reader,
            format,
            &expected,
            ArchiveLimits::default(),
            &Cancellation::default(),
        )
        .unwrap();
        assert_eq!(verified.members(), 1);
        assert_eq!(verified.unpacked_bytes(), 3);
        assert!(!verified.is_empty());
        let mut missing = expected.clone();
        missing.insert(path("missing"), content(b""));
        assert!(
            verify_archive(
                &mut reader,
                format,
                &missing,
                ArchiveLimits::default(),
                &Cancellation::default()
            )
            .is_err()
        );
        let wrong = BTreeMap::from([(path("a.bin"), content(&[0xff, 0, 2]))]);
        assert!(
            verify_archive(
                &mut reader,
                format,
                &wrong,
                ArchiveLimits::default(),
                &Cancellation::default()
            )
            .is_err()
        );
        assert!(
            verify_archive(
                &mut reader,
                format,
                &expected,
                ArchiveLimits {
                    total_bytes: 2,
                    ..ArchiveLimits::default()
                },
                &Cancellation::default()
            )
            .is_err()
        );
    }
}
fn zip(entries: &[(&str, &[u8], u32)]) -> Cursor<Vec<u8>> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes, mode) in entries {
        writer
            .start_file(
                *name,
                zip::write::SimpleFileOptions::default().unix_permissions(*mode),
            )
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap()
}
#[test]
fn traversal_collision_extra_content_and_executable_loss_are_rejected() {
    let expected = BTreeMap::from([(path("a"), content(b"x"))]);
    for mut archive in [
        zip(&[("../a", b"x", 0o644)]),
        zip(&[("a", b"x", 0o644), ("A", b"x", 0o644)]),
        zip(&[("a", b"x", 0o644), ("extra", b"x", 0o644)]),
    ] {
        assert!(
            verify_archive(
                &mut archive,
                DistributionArchive::Zip,
                &expected,
                ArchiveLimits::default(),
                &Cancellation::default()
            )
            .is_err()
        );
    }
    let mut executable = expected;
    executable
        .get_mut(&path("a"))
        .unwrap()
        .permissions
        .executable = true;
    assert!(
        verify_archive(
            &mut zip(&[("a", b"x", 0o644)]),
            DistributionArchive::Zip,
            &executable,
            ArchiveLimits::default(),
            &Cancellation::default()
        )
        .is_err()
    );
    assert!(
        verify_archive(
            &mut zip(&[("a", b"x", 0o755)]),
            DistributionArchive::Zip,
            &executable,
            ArchiveLimits::default(),
            &Cancellation::default()
        )
        .is_ok()
    );
}

#[test]
fn duplicate_zip_records_cannot_hide_behind_the_parser_name_index() {
    let mut archive = zip(&[("a", b"x", 0o644), ("b", b"x", 0o644)]).into_inner();
    let mut changed = 0;
    for index in 0..archive.len() - 47 {
        if archive[index..].starts_with(b"PK\x01\x02") && archive[index + 46] == b'b' {
            archive[index + 46] = b'a';
            changed += 1;
        }
    }
    assert_eq!(changed, 1);
    let expected = BTreeMap::from([(path("a"), content(b"x"))]);
    let error = verify_archive(
        &mut Cursor::new(archive),
        DistributionArchive::Zip,
        &expected,
        ArchiveLimits::default(),
        &Cancellation::default(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("Duplicate ZIP"));
}

#[test]
fn directories_cannot_alias_files_or_other_directory_spellings() {
    for directory in ["a/", "A/", "dir/../"] {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .add_directory(directory, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer
            .start_file("a", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"x").unwrap();
        assert!(
            verify_archive(
                &mut writer.finish().unwrap(),
                DistributionArchive::Zip,
                &BTreeMap::from([(path("a"), content(b"x"))]),
                ArchiveLimits::default(),
                &Cancellation::default()
            )
            .is_err()
        );
    }
}

#[test]
fn every_encoder_preserves_portable_permissions_and_empty_directories() {
    use crate::engine::{snapshot::SnapshotLimits, staging::MutableStage};
    let cancel = Cancellation::default();
    let source = tempfile::tempdir().unwrap();
    fs::create_dir(source.path().join("empty")).unwrap();
    fs::write(source.path().join("launcher"), b"binary\xff").unwrap();
    let root = crate::engine::snapshot::ProjectReadRoot::open(source.path()).unwrap();
    let snapshot = root
        .capture(
            &[path("empty"), path("launcher")],
            SnapshotLimits::default(),
            &cancel,
        )
        .unwrap();
    let mut stage = MutableStage::from_snapshot(&root, &snapshot, &cancel)
        .unwrap()
        .freeze(SnapshotLimits::default(), &cancel)
        .unwrap();
    let mut file = content(b"binary\xff");
    // Intent carries this bit even on Windows, whose native files cannot express it.
    file.permissions = FilePermissions {
        readonly: true,
        executable: true,
    };
    let expected = BTreeMap::from([(path("launcher"), file)]);
    for format in [
        DistributionArchive::Zip,
        DistributionArchive::TarGz,
        DistributionArchive::SevenZip,
    ] {
        let mut output = tempfile::tempfile().unwrap();
        let receipt = write_archive(
            &mut stage,
            &mut output,
            format,
            &expected,
            ArchiveLimits::default(),
            &cancel,
        )
        .unwrap();
        assert_eq!(receipt.directories(), &BTreeSet::from([path("empty")]));
        assert_eq!(receipt.members(), 1);
        if format == DistributionArchive::TarGz {
            output.rewind().unwrap();
            let mut bytes = Vec::new();
            output.read_to_end(&mut bytes).unwrap();
            let trailer = bytes.len() - 8;
            bytes[trailer] ^= 1;
            assert!(
                verify_archive(
                    &mut Cursor::new(bytes),
                    format,
                    &expected,
                    ArchiveLimits::default(),
                    &cancel
                )
                .is_err()
            );
        }
        let mut limited = tempfile::tempfile().unwrap();
        assert!(
            write_archive(
                &mut stage,
                &mut limited,
                format,
                &expected,
                ArchiveLimits {
                    compressed_bytes: 24,
                    ..ArchiveLimits::default()
                },
                &cancel
            )
            .is_err()
        );
        assert!(limited.metadata().unwrap().len() <= 24);
    }
}
