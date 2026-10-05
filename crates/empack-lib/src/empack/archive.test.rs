use super::*;
use tempfile::tempdir;

#[test]
fn test_create_and_extract_zip_round_trip() {
    let tmp = tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(src.join("sub")).unwrap();
    std::fs::write(src.join("a.txt"), "alpha").unwrap();
    std::fs::write(src.join("sub/b.txt"), "beta").unwrap();

    let zip_path = tmp.path().join("out.zip");
    create_archive(&src, &zip_path, ArchiveFormat::Zip).unwrap();

    let extract_dir = tmp.path().join("verify");
    extract_zip(&zip_path, &extract_dir).unwrap();

    assert_eq!(
        std::fs::read_to_string(extract_dir.join("a.txt")).unwrap(),
        "alpha"
    );
    assert_eq!(
        std::fs::read_to_string(extract_dir.join("sub/b.txt")).unwrap(),
        "beta"
    );
}

#[test]
fn test_create_tar_gz() {
    let tmp = tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("file.txt"), "content").unwrap();

    let tar_path = tmp.path().join("out.tar.gz");
    create_archive(&src, &tar_path, ArchiveFormat::TarGz).unwrap();

    let file = std::fs::File::open(&tar_path).unwrap();
    let dec = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(dec);
    let entries: Vec<_> = archive.entries().unwrap().collect();
    assert!(!entries.is_empty());
}

#[test]
fn test_create_and_extract_7z_round_trip() {
    let tmp = tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(src.join("nested")).unwrap();
    let data: Vec<u8> = (0..1024).map(|i| (i % 256) as u8).collect();
    std::fs::write(src.join("data.bin"), &data).unwrap();
    std::fs::write(src.join("nested/settings.txt"), "loader=fabric\n").unwrap();

    let sz_path = tmp.path().join("out.7z");
    create_archive(&src, &sz_path, ArchiveFormat::SevenZ).unwrap();

    let extracted = tmp.path().join("verify");
    sevenz_rust2::decompress_file(&sz_path, &extracted).unwrap();
    assert_eq!(std::fs::read(extracted.join("data.bin")).unwrap(), data);
    assert_eq!(std::fs::read_to_string(extracted.join("nested/settings.txt")).unwrap(), "loader=fabric\n");
    assert!(!extracted.join("src").exists(), "archive paths must be relative to the source root");
}

#[test]
fn test_extract_zip_preserves_nested_directories() {
    let tmp = tempdir().unwrap();
    let zip_path = tmp.path().join("nested.zip");

    let file = std::fs::File::create(&zip_path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    writer.add_directory("config/", options).unwrap();
    writer.start_file("config/settings.toml", options).unwrap();
    std::io::Write::write_all(&mut writer, b"key = true").unwrap();
    writer.add_directory("data/nested/", options).unwrap();
    writer.start_file("data/nested/deep.txt", options).unwrap();
    std::io::Write::write_all(&mut writer, b"deep value").unwrap();
    writer.finish().unwrap();

    let out_dir = tmp.path().join("extracted");
    extract_zip(&zip_path, &out_dir).unwrap();

    assert!(out_dir.join("config").is_dir());
    assert_eq!(
        std::fs::read_to_string(out_dir.join("config/settings.toml")).unwrap(),
        "key = true"
    );
    assert!(out_dir.join("data/nested").is_dir());
    assert_eq!(
        std::fs::read_to_string(out_dir.join("data/nested/deep.txt")).unwrap(),
        "deep value"
    );
}

#[test]
fn test_create_archive_empty_source_returns_error() {
    let tmp = tempdir().unwrap();
    let src = tmp.path().join("empty");
    std::fs::create_dir_all(&src).unwrap();

    let zip_path = tmp.path().join("empty.zip");
    let result = create_archive(&src, &zip_path, ArchiveFormat::Zip);
    assert!(matches!(result, Err(ArchiveError::EmptySource(_))));
}

#[test]
fn test_create_archive_nonexistent_source_returns_error() {
    let tmp = tempdir().unwrap();
    let result = create_archive(
        &tmp.path().join("nonexistent"),
        &tmp.path().join("out.zip"),
        ArchiveFormat::Zip,
    );
    assert!(matches!(result, Err(ArchiveError::SourceNotFound(_))));
}

#[test]
fn test_create_archive_nonexistent_source_tar_gz_returns_error() {
    let tmp = tempdir().unwrap();
    let result = create_archive(
        &tmp.path().join("nonexistent"),
        &tmp.path().join("out.tar.gz"),
        ArchiveFormat::TarGz,
    );
    assert!(matches!(result, Err(ArchiveError::SourceNotFound(_))));
}

#[test]
fn test_create_archive_nonexistent_source_7z_returns_error() {
    let tmp = tempdir().unwrap();
    let result = create_archive(
        &tmp.path().join("nonexistent"),
        &tmp.path().join("out.7z"),
        ArchiveFormat::SevenZ,
    );
    assert!(matches!(result, Err(ArchiveError::SourceNotFound(_))));
}

#[test]
fn test_extract_zip_nonexistent_archive_returns_error() {
    let tmp = tempdir().unwrap();
    let result = extract_zip(&tmp.path().join("nonexistent.zip"), &tmp.path().join("out"));
    assert!(matches!(result, Err(ArchiveError::Io { .. })));
}

#[test]
fn test_create_archive_empty_source_tar_gz_returns_error() {
    let tmp = tempdir().unwrap();
    let src = tmp.path().join("empty");
    std::fs::create_dir_all(&src).unwrap();

    let tar_path = tmp.path().join("empty.tar.gz");
    let result = create_archive(&src, &tar_path, ArchiveFormat::TarGz);
    assert!(matches!(result, Err(ArchiveError::EmptySource(_))));
}

#[test]
fn test_create_archive_empty_source_7z_returns_error() {
    let tmp = tempdir().unwrap();
    let src = tmp.path().join("empty");
    std::fs::create_dir_all(&src).unwrap();

    let sz_path = tmp.path().join("empty.7z");
    let result = create_archive(&src, &sz_path, ArchiveFormat::SevenZ);
    assert!(matches!(result, Err(ArchiveError::EmptySource(_))));
}

#[test]
fn test_create_archive_empty_nested_dirs_returns_error() {
    let tmp = tempdir().unwrap();
    let src = tmp.path().join("nested-empty");
    std::fs::create_dir_all(src.join("a/b/c")).unwrap();

    let zip_path = tmp.path().join("nested-empty.zip");
    let result = create_archive(&src, &zip_path, ArchiveFormat::Zip);
    assert!(matches!(result, Err(ArchiveError::EmptySource(_))));
}

#[test]
fn test_archive_format_extension() {
    assert_eq!(ArchiveFormat::Zip.extension(), "zip");
    assert_eq!(ArchiveFormat::TarGz.extension(), "tar.gz");
    assert_eq!(ArchiveFormat::SevenZ.extension(), "7z");
}

#[test]
fn test_archive_format_display() {
    assert_eq!(format!("{}", ArchiveFormat::Zip), "zip");
    assert_eq!(format!("{}", ArchiveFormat::TarGz), "tar.gz");
    assert_eq!(format!("{}", ArchiveFormat::SevenZ), "7z");
}

#[test]
fn rejected_archive_input_preserves_existing_artifacts_for_every_format() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::create_dir(source.join("empty")).unwrap();
    for format in [ArchiveFormat::Zip, ArchiveFormat::TarGz, ArchiveFormat::SevenZ] {
        let artifact = temp.path().join(format!("old.{}", format.extension()));
        std::fs::write(&artifact, b"prior artifact").unwrap();
        assert!(create_archive(&source, &artifact, format).is_err());
        assert_eq!(std::fs::read(&artifact).unwrap(), b"prior artifact");
    }
}

#[cfg(unix)]
#[test]
fn linked_input_is_rejected_without_reading_or_replacing_the_old_artifact() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("source");
    std::fs::create_dir(&source).unwrap();
    let outside = temp.path().join("outside");
    std::fs::write(&outside, b"outside bytes").unwrap();
    std::os::unix::fs::symlink(&outside, source.join("linked")).unwrap();
    let output = temp.path().join("old.zip");
    std::fs::write(&output, b"old").unwrap();
    assert!(create_archive(&source, &output, ArchiveFormat::Zip).is_err());
    assert_eq!(std::fs::read(&output).unwrap(), b"old");
    assert_eq!(std::fs::read(&outside).unwrap(), b"outside bytes");
}

#[cfg(unix)]
#[test]
fn published_archive_preserves_existing_mode_and_honors_umask_for_new_files() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempdir().unwrap();
    let input = temp.path().join("source");
    std::fs::create_dir(&input).unwrap();
    std::fs::write(input.join("file"), b"content").unwrap();
    let normal = temp.path().join("normal-file");
    std::fs::write(&normal, b"").unwrap();
    let expected = std::fs::metadata(normal).unwrap().permissions().mode() & 0o777;
    for format in [ArchiveFormat::Zip, ArchiveFormat::TarGz, ArchiveFormat::SevenZ] {
        let output = temp.path().join(format!("out.{}", format.extension()));
        create_archive(&input, &output, format).unwrap();
        assert_eq!(std::fs::metadata(&output).unwrap().permissions().mode() & 0o777, expected);
        std::fs::set_permissions(&output, std::fs::Permissions::from_mode(0o640)).unwrap();
        create_archive(&input, &output, format).unwrap();
        assert_eq!(std::fs::metadata(&output).unwrap().permissions().mode() & 0o777, 0o640);
    }
}
