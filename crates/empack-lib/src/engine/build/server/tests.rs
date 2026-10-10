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
    intent.distribution.native = Some(empack_core::model::NativeDistributionIntent {
        pack_id: "server.fixture".into(),
        java_major: 21,
        policies: BTreeMap::new(),
    });
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
            let build = prepare_server_build(
                capture(root.path(), host.path(), name),
                path(name).unwrap(),
                &external,
                &options(format),
                &prepared_fixture(),
                lightweight,
                &cancel,
            )
            .unwrap();
            assert!(!root.path().join("dist").exists());
            assert!(!build.uses_user_configuration());
            let expected = build.inventory().clone();
            assert_eq!(
                expected.contains_key(&path("game/server.jar").unwrap()),
                !lightweight
            );
            assert!(expected.contains_key(&path("install_pack.bat").unwrap()));
            assert!(!expected.contains_key(&path("eula.txt").unwrap()));
            assert!(!expected.contains_key(&path("excluded").unwrap()));
            assert_eq!(
                expected.contains_key(&path(".empack-consumer/release.json").unwrap()),
                lightweight
            );
            assert_eq!(
                expected.contains_key(&path("game/resourcepacks/a.zip").unwrap()),
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
                if !lightweight {
                    zip.by_name("game/config/example")
                        .unwrap()
                        .read_to_string(&mut value)
                        .unwrap();
                    assert_eq!(value, "server");
                } else {
                    let mut bytes = Vec::new();
                    zip.by_name(".empack-consumer/release.json")
                        .unwrap()
                        .read_to_end(&mut bytes)
                        .unwrap();
                    let release = crate::engine::release::DecodedRelease::decode(&bytes).unwrap();
                    let file = release
                        .document()
                        .files
                        .iter()
                        .find(|f| f.destination == "config/example")
                        .unwrap();
                    let mut bytes = Vec::new();
                    zip.by_name(&format!(".empack-consumer/{}", file.asset_path().unwrap()))
                        .unwrap()
                        .read_to_end(&mut bytes)
                        .unwrap();
                    assert_eq!(bytes, b"server");
                    assert!(!zip.file_names().any(|p| p.contains("packwiz")));
                }
                value.clear();
                zip.by_name("install_pack.sh")
                    .unwrap()
                    .read_to_string(&mut value)
                    .unwrap();
                assert_eq!(value.contains("instance prepare"), lightweight);
                assert!(!value.contains("eula=true"));
                if lightweight {
                    use crate::engine::{
                        instance::*,
                        release::{DecodedRelease, ReleaseSource, trust::SelectedSnapshot},
                    };
                    let instance = tempfile::tempdir().unwrap();
                    zip.extract(instance.path()).unwrap();
                    let assets = instance.path().join(".empack-consumer");
                    let release =
                        DecodedRelease::decode(&fs::read(assets.join("release.json")).unwrap())
                            .unwrap();
                    let supplied = release
                        .document()
                        .files
                        .iter()
                        .filter(|f| matches!(f.source, ReleaseSource::Url { .. }))
                        .map(|f| {
                            (
                                f.key.clone(),
                                external.locked.values().next().unwrap().content.clone(),
                            )
                        })
                        .collect();
                    assert!(!instance.path().join("game/server.jar").exists());
                    let mut runtime_before = Vec::new();
                    prepared_fixture().files()[&path("server.jar").unwrap()]
                        .content
                        .lease()
                        .open()
                        .read_to_end(&mut runtime_before)
                        .unwrap();
                    let plan = crate::engine::instance::plan(
                        instance.path(),
                        InstanceSelection {
                            require_subscription: false,
                            conflicts: Vec::new(),
                            release: SelectedRelease::Snapshot(
                                SelectedSnapshot::select(
                                    release.bytes(),
                                    release.id(),
                                    &semver::Version::parse("0.6.0-beta").unwrap(),
                                )
                                .unwrap(),
                            ),
                            side: InstanceSide::Server,
                            layout: Some(InstanceLayout::Game),
                            choices: vec![],
                            action: InstanceAction::Prepare,
                        },
                        RecoveryReader::new(host.path().join("instance-state")),
                        SnapshotLimits::default(),
                        &cancel,
                    )
                    .unwrap();
                    plan.stage(&supplied, &BTreeMap::new(), Some(&assets), &cancel)
                        .unwrap()
                        .publish(
                            &Publisher::open(&host.path().join("instance-state")).unwrap(),
                            &cancel,
                        )
                        .unwrap();
                    assert_eq!(
                        fs::read(instance.path().join("game/config/example")).unwrap(),
                        b"server"
                    );
                    assert_eq!(
                        fs::read(instance.path().join("game/resourcepacks/a.zip")).unwrap(),
                        b"payload"
                    );
                    assert_eq!(
                        fs::read(instance.path().join("game/server.jar")).unwrap(),
                        runtime_before
                    );
                    assert!(!instance.path().join("game/eula.txt").exists());
                }
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
        false,
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
    put(
        root.path(),
        "templates/server/game/server.jar",
        b"replacement",
    );
    assert!(
        prepare_server_build(
            capture(root.path(), host.path(), "server.zip"),
            path("server.zip").unwrap(),
            &external,
            &options(DistributionArchive::Zip),
            &prepared_fixture(),
            false,
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
        false,
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
    fs::create_dir(root.path().join("game")).unwrap();
    put(root.path(), "start.sh", START_SH.as_bytes());
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
    let status = std::process::Command::new("bash")
        .arg(root.path().join("start.sh"))
        .args(["nogui", "an argument"])
        .env("JAVA_HOME", root.path().join("java space"))
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(
        fs::read_to_string(root.path().join("game/arguments")).unwrap(),
        "-jar\nserver.jar\nnogui\nan argument\n"
    );
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
        false,
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
fn native_start_prepares_first_and_failure_blocks_the_server() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::Builder::new()
        .prefix("native server ")
        .tempdir()
        .unwrap();
    fs::create_dir(root.path().join("game")).unwrap();
    put(
        root.path(),
        "start.sh",
        runtime_start(&ServerLaunch::Jar(path("server.jar").unwrap()), true, false).as_bytes(),
    );
    let id = "a".repeat(64);
    put(
        root.path(),
        "install_pack.sh",
        install_script(Some(&id)).as_bytes(),
    );
    put(
        root.path(),
        "bin/empack",
        br##"#!/bin/sh
if [ "$5" = prepare ]; then
  printf '%s\n' "$@" > "$EMPACK_TEST_ARGS"
  exit "$EMPACK_TEST_INSTALL_STATUS"
fi
printf '%s\n' "$@" > "$EMPACK_TEST_ARGS.launch"
[ "$5" = launch ] && [ "$6" = --server ] && [ "$7" = -- ] || exit 91
root="$2"
shift 7
cd "$root/game" || exit 92
program="$1"
shift
exec "$program" -jar server.jar "$@"
"##,
    );
    put(
        root.path(),
        "java space/bin/java",
        b"#!/bin/sh\nprintf '%s\n' \"$@\" > server-arguments\nexit \"$EMPACK_TEST_RUNTIME_STATUS\"\n",
    );
    for name in ["bin/empack", "java space/bin/java"] {
        fs::set_permissions(root.path().join(name), fs::Permissions::from_mode(0o700)).unwrap();
    }
    let paths = std::env::join_paths(std::iter::once(root.path().join("bin")).chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .unwrap();
    for (code, runtime_code) in [(0, 0), (0, 9), (7, 0)] {
        let arguments = root.path().join("game/server-arguments");
        if arguments.exists() {
            fs::remove_file(&arguments).unwrap();
        }
        let status = std::process::Command::new("bash")
            .arg(root.path().join("start.sh"))
            .args(["nogui", "an argument"])
            .env("JAVA_HOME", root.path().join("java space"))
            .env("PATH", &paths)
            .env("EMPACK_TEST_ARGS", root.path().join("instance-arguments"))
            .env("EMPACK_TEST_INSTALL_STATUS", code.to_string())
            .env("EMPACK_TEST_RUNTIME_STATUS", runtime_code.to_string())
            .status()
            .unwrap();
        assert_eq!(
            status.code(),
            Some(if code == 0 { runtime_code } else { code })
        );
        assert_eq!(arguments.exists(), code == 0);
        let selected = fs::read_to_string(root.path().join("instance-arguments")).unwrap();
        let expected = format!(
            "--workdir\n{}\n--yes\ninstance\nprepare\n{}/.empack-consumer/release.json\n--sha256\n{id}\n--layout\ngame\n--side\nserver\n",
            root.path().display(),
            root.path().display()
        );
        assert_eq!(selected, expected);
        if code == 0 {
            let launch = fs::read_to_string(root.path().join("instance-arguments.launch")).unwrap();
            assert_eq!(
                launch,
                format!(
                    "--workdir\n{}\n--yes\ninstance\nlaunch\n--server\n--\n{}\nnogui\nan argument\n",
                    root.path().display(),
                    root.path().join("java space/bin/java").display()
                )
            );
            assert_eq!(
                fs::read_to_string(arguments).unwrap(),
                "-jar\nserver.jar\nnogui\nan argument\n"
            );
        }
    }
}
#[cfg(windows)]
#[test]
fn native_windows_start_prepares_first_and_propagates_failure_without_bash() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("game")).unwrap();
    put(
        root.path(),
        "start.bat",
        start_batch(true)
            .replace("empack --workdir", "call fake-empack.cmd --workdir")
            .as_bytes(),
    );
    // CALL lets a batch fixture return like the empack executable it replaces.
    let install = install_batch(Some(&"a".repeat(64)))
        .replace("empack --workdir", "call fake-empack.cmd --workdir");
    put(root.path(), "install_pack.bat", install.as_bytes());
    put(
        root.path(),
        "fake-empack.cmd",
        b"@echo off\r\nif %5==launch goto launch\r\necho %* > instance-arguments\r\nexit /b %EMPACK_TEST_INSTALL_STATUS%\r\n:launch\r\necho %* > launch-arguments\r\ncd game || exit /b 92\r\ncall java.cmd nogui\r\nexit /b %errorlevel%\r\n",
    );
    put(
        root.path(),
        "game/java.cmd",
        b"@echo off\r\necho %* > server-arguments\r\nexit /b 0\r\n",
    );
    for code in [0, 7] {
        let args = root.path().join("game/server-arguments");
        if args.exists() {
            fs::remove_file(&args).unwrap();
        }
        let status = std::process::Command::new("cmd")
            .args(["/d", "/c", "start.bat", "nogui"])
            .current_dir(root.path())
            .env_remove("JAVA_HOME")
            .env("EMPACK_TEST_INSTALL_STATUS", code.to_string())
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(code));
        assert_eq!(args.exists(), code == 0);
        let arguments = fs::read_to_string(root.path().join("instance-arguments")).unwrap();
        assert!(arguments.contains("instance prepare"));
        assert!(arguments.contains("--layout game --side server"));
        if code == 0 {
            let launch = fs::read_to_string(root.path().join("launch-arguments")).unwrap();
            assert!(launch.contains("instance launch -- java -jar server.jar nogui"));
        }
    }
}

#[cfg(unix)]
#[test]
fn typed_runtime_launch_preserves_argument_files_and_historical_jars() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("game")).unwrap();
    put(
        root.path(),
        "java/bin/java",
        b"#!/bin/sh\nprintf '%s\\n' \"$@\" > arguments.txt\n",
    );
    std::fs::set_permissions(
        root.path().join("java/bin/java"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    for (launch, expected) in [
        (
            ServerLaunch::Arguments {
                unix: path("libraries/loader/unix_args.txt").unwrap(),
                windows: path("libraries/loader/win_args.txt").unwrap(),
            },
            "@user_jvm_args.txt\n@libraries/loader/unix_args.txt\nuser argument\n",
        ),
        (
            ServerLaunch::Jar(path("forge-1.12.2-14.23.5.2860.jar").unwrap()),
            "-jar\nforge-1.12.2-14.23.5.2860.jar\nuser argument\n",
        ),
    ] {
        put(
            root.path(),
            "start.sh",
            runtime_start(&launch, false, false).as_bytes(),
        );
        let status = std::process::Command::new("bash")
            .arg(root.path().join("start.sh"))
            .arg("user argument")
            .env("JAVA_HOME", root.path().join("java"))
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(
            std::fs::read_to_string(root.path().join("game/arguments.txt")).unwrap(),
            expected
        );
        let windows = runtime_start(&launch, false, true);
        assert!(windows.contains("DisableDelayedExpansion"));
        if matches!(launch, ServerLaunch::Arguments { .. }) {
            assert!(windows.contains("@libraries/loader/win_args.txt"));
            assert!(!windows.contains("unix_args.txt"));
        }
    }
}

#[test]
fn subscribed_server_scripts_require_enrollment_and_check_before_runtime() {
    let recipe = Recipe::SERVER_BUNDLED
        .with_update_authority(empack_core::distribution::UpdateAuthority::Empack)
        .unwrap();
    for windows in [false, true] {
        let launch = consumer_script(
            runtime_start(
                &ServerLaunch::Jar(path("server.jar").unwrap()),
                true,
                windows,
            ),
            recipe,
        );
        assert!(launch.contains("instance launch --server --check-updates --"));
        let install = consumer_script(
            if windows {
                install_batch(Some("digest"))
            } else {
                install_script(Some("digest"))
            },
            recipe,
        );
        assert!(install.contains("--side server --require-subscription"));
    }
}
