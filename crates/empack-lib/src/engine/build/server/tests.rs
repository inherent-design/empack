use super::*;
use crate::engine::{
    artifacts::verify_archive,
    documents::DocumentCodec,
    mrpack::{LockedFileKey, tests::project},
    project::ProjectReader,
    publication::{Publisher, RecoveryReader},
    server_runtime::tests::prepared_fixture,
};
use empack_core::{
    model::{LoaderKind, ResolvedProject},
    requirements::Requirement,
};
use std::{fs, io::Read, path::Path};
fn put(root: &Path, name: &str, bytes: &[u8]) {
    let file = root.join(name);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, bytes).unwrap();
}
pub(in crate::engine::build) fn fixture(root: &Path) -> BuildAcquisitions {
    let original = project(false, false);
    let mut intent = original.intent().clone();
    intent.runtime.loader = LoaderKind::Vanilla;
    intent.runtime.loader_version = None;
    for dependency in intent.roots.values_mut() {
        dependency.requirements.server = Requirement::Required;
    }
    let bytes = DocumentCodec.encode_intent(&intent).unwrap();
    let decoded = DocumentCodec.decode_intent(&bytes, "fixture").unwrap();
    let mut lock = original.lock().clone();
    lock.runtime.loader = LoaderKind::Vanilla;
    lock.runtime.loader_version = None;
    lock.intent_revision = decoded.semantic_revision();
    let mut external = BuildAcquisitions::default();
    for (key, dependency) in &mut lock.dependencies {
        let mut files = dependency.files.as_slice().to_vec();
        for file in &mut files {
            let mut placements = file.placements.as_slice().to_vec();
            for placement in &mut placements {
                placement.requirements.server = Requirement::Required;
            }
            file.placements = empack_core::model::NonEmpty::new(placements).unwrap();
            external.locked.insert(
                LockedFileKey {
                    dependency: key.clone(),
                    slot: file.slot.clone(),
                },
                generated(b"payload", false, &Cancellation::default()).unwrap(),
            );
        }
        dependency.files = empack_core::model::NonEmpty::new(files).unwrap();
    }
    let project = ResolvedProject::validate(intent, lock, decoded.semantic_revision()).unwrap();
    put(root, "empack.yml", &bytes);
    put(
        root,
        "empack.lock",
        &DocumentCodec.encode_lock(&project).unwrap(),
    );
    external
}
fn options(format: DistributionArchive) -> ServerOptions {
    ServerOptions {
        archive: format,
        optional: OptionalPolicy::Preserve,
        templates: TemplateOptions::default(),
        evidence: SourceEvidencePolicy::Compatibility,
        limits: ArchiveLimits::default(),
    }
}
fn capture(root: &Path, host: &Path, name: &str) -> WorkspaceSnapshot {
    ProjectReader::new(RecoveryReader::new(host.join("private")))
        .capture_build(
            root,
            &[path(name).unwrap()],
            SnapshotLimits::default(),
            &Cancellation::default(),
        )
        .unwrap()
}
#[test]
fn both_server_recipes_publish_exact_side_content_and_runtime_in_all_formats() {
    for (format, name) in [
        (DistributionArchive::Zip, "server.zip"),
        (DistributionArchive::TarGz, "server.tar.gz"),
        (DistributionArchive::SevenZip, "server.7z"),
    ] {
        for lightweight in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let host = tempfile::tempdir().unwrap();
            let external = fixture(root.path());
            let cancel = Cancellation::default();
            put(root.path(), "pack/config/example", b"common");
            put(root.path(), "overrides/server/config/example", b"server");
            put(root.path(), "overrides/client/config/example", b"client");
            put(root.path(), "templates/common/banner.bin", &[255, 0, 1]);
            put(root.path(), "templates/server/note.template", b"{{NAME}}");
            put(root.path(), "templates/client/excluded", b"excluded");
            let bootstrap = lightweight.then(|| ServerBootstrap {
                assets: InstallerAssets::fixture(),
                interaction: InstallerInteraction::Headless,
            });
            let empty = BuildAcquisitions::default();
            let build = prepare_server_build(
                capture(root.path(), host.path(), name),
                path(name).unwrap(),
                if lightweight { &empty } else { &external },
                &options(format),
                &prepared_fixture(),
                bootstrap.as_ref(),
                &cancel,
            )
            .unwrap();
            assert!(!root.path().join("dist").exists());
            assert!(!build.uses_user_configuration());
            let expected = build.inventory().clone();
            assert!(expected.contains_key(&path("server.jar").unwrap()));
            assert!(expected.contains_key(&path("install_pack.bat").unwrap()));
            assert!(!expected.contains_key(&path("eula.txt").unwrap()));
            assert!(!expected.contains_key(&path("excluded").unwrap()));
            assert_eq!(
                expected.contains_key(&path("packwiz-installer.jar").unwrap()),
                lightweight
            );
            assert_eq!(
                expected.contains_key(&path("resourcepacks/a.zip").unwrap()),
                !lightweight
            );
            build
                .publish(
                    &Publisher::open(&host.path().join("private")).unwrap(),
                    &cancel,
                )
                .unwrap();
            let mut file = fs::File::open(root.path().join("dist").join(name)).unwrap();
            verify_archive(
                &mut file,
                format,
                &expected,
                ArchiveLimits::default(),
                &cancel,
            )
            .unwrap();
            if format == DistributionArchive::Zip {
                let mut zip = zip::ZipArchive::new(file).unwrap();
                let mut value = String::new();
                zip.by_name("config/example")
                    .unwrap()
                    .read_to_string(&mut value)
                    .unwrap();
                assert_eq!(value, "server");
                value.clear();
                zip.by_name("install_pack.sh")
                    .unwrap()
                    .read_to_string(&mut value)
                    .unwrap();
                assert_eq!(value.contains("--bootstrap-no-update --bootstrap-main-jar packwiz-installer.jar --no-gui -s server"),lightweight);
                assert!(!value.contains("eula=true"));
            }
        }
    }
}
#[test]
fn wrong_runtime_and_colliding_templates_preserve_previous_distribution() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let cancel = Cancellation::default();
    let original = project(false, false);
    put(
        root.path(),
        "empack.yml",
        &DocumentCodec.encode_intent(original.intent()).unwrap(),
    );
    put(
        root.path(),
        "empack.lock",
        &DocumentCodec.encode_lock(&original).unwrap(),
    );
    put(root.path(), "dist/server.zip", b"previous");
    let build = prepare_server_build(
        capture(root.path(), host.path(), "server.zip"),
        path("server.zip").unwrap(),
        &BuildAcquisitions::default(),
        &options(DistributionArchive::Zip),
        &prepared_fixture(),
        None,
        &cancel,
    );
    assert!(
        build
            .err()
            .unwrap()
            .to_string()
            .contains("differs from captured lock")
    );
    let external = fixture(root.path());
    put(root.path(), "templates/server/server.jar", b"replacement");
    assert!(
        prepare_server_build(
            capture(root.path(), host.path(), "server.zip"),
            path("server.zip").unwrap(),
            &external,
            &options(DistributionArchive::Zip),
            &prepared_fixture(),
            None,
            &cancel
        )
        .is_err()
    );
    assert_eq!(
        fs::read(root.path().join("dist/server.zip")).unwrap(),
        b"previous"
    );
}
#[test]
fn captured_user_scripts_are_preserved_and_reported() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let external = fixture(root.path());
    let cancel = Cancellation::default();
    put(root.path(), "templates/server/start.sh", b"custom script");
    let build = prepare_server_build(
        capture(root.path(), host.path(), "server.zip"),
        path("server.zip").unwrap(),
        &external,
        &options(DistributionArchive::Zip),
        &prepared_fixture(),
        None,
        &cancel,
    )
    .unwrap();
    assert!(build.uses_user_configuration());
    build
        .publish(
            &Publisher::open(&host.path().join("private")).unwrap(),
            &cancel,
        )
        .unwrap();
    let mut zip =
        zip::ZipArchive::new(fs::File::open(root.path().join("dist/server.zip")).unwrap()).unwrap();
    let mut value = String::new();
    zip.by_name("start.sh")
        .unwrap()
        .read_to_string(&mut value)
        .unwrap();
    assert_eq!(value, "custom script");
}
#[cfg(unix)]
#[test]
fn generated_shell_scripts_keep_java_path_and_user_arguments_separate() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    put(root.path(), "start.sh", START_SH.as_bytes());
    put(
        root.path(),
        "install_pack.sh",
        install_script(Some(&ServerBootstrap {
            assets: InstallerAssets::fixture(),
            interaction: InstallerInteraction::Headless,
        }))
        .as_bytes(),
    );
    put(
        root.path(),
        "java space/bin/java",
        b"#!/bin/sh\nprintf '%s\n' \"$@\" > arguments\n",
    );
    fs::set_permissions(
        root.path().join("java space/bin/java"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    let java = root.path().join("java space");
    let status = std::process::Command::new("bash")
        .arg(root.path().join("start.sh"))
        .args(["nogui", "an argument"])
        .env("JAVA_HOME", &java)
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(
        fs::read_to_string(root.path().join("arguments")).unwrap(),
        "-jar\nserver.jar\nnogui\nan argument\n"
    );
    let status = std::process::Command::new("bash")
        .arg(root.path().join("install_pack.sh"))
        .arg(java.join("bin/java"))
        .env_remove("JAVA_HOME")
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(
        fs::read_to_string(root.path().join("arguments")).unwrap(),
        "-jar\npackwiz-installer-bootstrap.jar\n--bootstrap-no-update\n--bootstrap-main-jar\npackwiz-installer.jar\n--no-gui\n-s\nserver\npack/pack.toml\n"
    );
    fs::remove_file(root.path().join("arguments")).unwrap();
    put(
        root.path(),
        "install_pack.sh",
        install_script(None).as_bytes(),
    );
    assert!(
        std::process::Command::new("bash")
            .arg(root.path().join("install_pack.sh"))
            .env_remove("JAVA_HOME")
            .status()
            .unwrap()
            .success()
    );
    assert!(!root.path().join("arguments").exists());
}

#[test]
fn computed_runtime_hash_does_not_upgrade_original_source_assurance() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let external = fixture(root.path());
    let mut options = options(DistributionArchive::Zip);
    options.evidence = SourceEvidencePolicy::StrongSourceRequired;
    let result = prepare_server_build(
        capture(root.path(), host.path(), "server.zip"),
        path("server.zip").unwrap(),
        &external,
        &options,
        &prepared_fixture(),
        None,
        &Cancellation::default(),
    );
    assert!(
        result
            .err()
            .unwrap()
            .to_string()
            .contains("strong runtime declaration")
    );
    assert!(!root.path().join("dist").exists());
}

#[cfg(unix)]
#[test]
fn lightweight_start_installs_first_and_failure_blocks_the_server() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    put(root.path(), "start.sh", start_script(true).as_bytes());
    put(
        root.path(),
        "install_pack.sh",
        install_script(Some(&ServerBootstrap {
            assets: InstallerAssets::fixture(),
            interaction: InstallerInteraction::Headless,
        }))
        .as_bytes(),
    );
    let java = root.path().join("java space");
    put(root.path(),"java space/bin/java",b"#!/bin/sh\nprintf '%s\n' \"$*\" >> arguments\nif [ \"$2\" = packwiz-installer-bootstrap.jar ]; then exit \"$EMPACK_TEST_INSTALL_STATUS\"; fi\n");
    fs::set_permissions(java.join("bin/java"), fs::Permissions::from_mode(0o700)).unwrap();
    for (failure, count) in [(false, 2), (true, 1)] {
        put(root.path(), "arguments", b"");
        let status = std::process::Command::new("bash")
            .arg(root.path().join("start.sh"))
            .arg("nogui")
            .env("JAVA_HOME", &java)
            .env(
                "EMPACK_TEST_INSTALL_STATUS",
                if failure { "7" } else { "0" },
            )
            .status()
            .unwrap();
        assert_eq!(status.success(), !failure);
        let arguments = fs::read_to_string(root.path().join("arguments")).unwrap();
        let lines: Vec<_> = arguments.lines().collect();
        assert_eq!(lines.len(), count);
        assert!(lines[0].contains("--bootstrap-main-jar packwiz-installer.jar --no-gui -s server"));
        if !failure {
            assert_eq!(lines[1], "-jar server.jar nogui");
        }
    }
}
#[cfg(windows)]
#[test]
fn native_windows_start_installs_first_and_propagates_failure_without_bash() {
    let root = tempfile::tempdir().unwrap();
    put(root.path(), "start.bat", start_batch(true).as_bytes());
    put(
        root.path(),
        "install_pack.bat",
        install_batch(Some(&ServerBootstrap {
            assets: InstallerAssets::fixture(),
            interaction: InstallerInteraction::Headless,
        }))
        .as_bytes(),
    );
    put(root.path(),"java.cmd",b"@echo off\r\necho %*>>arguments\r\nif \"%~2\"==\"packwiz-installer-bootstrap.jar\" exit /b %EMPACK_TEST_INSTALL_STATUS%\r\nexit /b 0\r\n");
    for (failure, count) in [(false, 2), (true, 1)] {
        put(root.path(), "arguments", b"");
        let status = std::process::Command::new("cmd")
            .args(["/d", "/c", "start.bat", "nogui"])
            .current_dir(root.path())
            .env_remove("JAVA_HOME")
            .env(
                "EMPACK_TEST_INSTALL_STATUS",
                if failure { "7" } else { "0" },
            )
            .status()
            .unwrap();
        assert_eq!(status.success(), !failure);
        let arguments = fs::read_to_string(root.path().join("arguments")).unwrap();
        let lines: Vec<_> = arguments.lines().collect();
        assert_eq!(lines.len(), count);
        assert!(lines[0].contains("--bootstrap-main-jar packwiz-installer.jar --no-gui -s server"));
        if !failure {
            assert_eq!(lines[1], "-jar server.jar nogui");
        }
    }
}
