use super::*;
use crate::engine::{
    artifacts::verify_archive,
    documents::DocumentCodec,
    mrpack::{LockedFileKey, tests::project},
    project::ProjectReader,
    publication::{Publisher, RecoveryReader},
};
use empack_core::model::{GameVersion, LoaderVersion};
use std::{fs, path::Path};
fn put(root: &Path, name: &str, bytes: &[u8]) {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}
fn fixture(root: &Path) -> BuildAcquisitions {
    let initial = project(false, false);
    let mut intent = initial.intent().clone();
    intent.distribution.native = Some(empack_core::model::NativeDistributionIntent {
        pack_id: "test.pack".into(),
        java_major: 21,
        policies: BTreeMap::new(),
    });
    let project = crate::engine::mrpack::tests::explicitly_placed(intent, initial.lock().clone());
    put(
        root,
        "empack.yml",
        &DocumentCodec.encode_intent(project.intent()).unwrap(),
    );
    put(
        root,
        "empack.lock",
        &DocumentCodec.encode_lock(&project).unwrap(),
    );
    let mut acquired = BuildAcquisitions::default();
    for (key, dependency) in &project.lock().dependencies {
        for file in dependency.files.as_slice() {
            acquired.locked.insert(
                LockedFileKey {
                    dependency: key.clone(),
                    slot: file.slot.clone(),
                },
                generated(b"payload", &Cancellation::default()).unwrap(),
            );
        }
    }
    acquired
}
fn options(format: DistributionArchive) -> ClientOptions {
    ClientOptions {
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
fn full_client_archives_include_game_templates_and_exact_launcher_profile() {
    for (format, name) in [
        (DistributionArchive::Zip, "client.zip"),
        (DistributionArchive::TarGz, "client.tar.gz"),
        (DistributionArchive::SevenZip, "client.7z"),
    ] {
        let root = tempfile::tempdir().unwrap();
        let host = tempfile::tempdir().unwrap();
        let external = fixture(root.path());
        let cancel = Cancellation::default();
        put(root.path(), "pack/config/example.txt", b"common");
        put(
            root.path(),
            "overrides/client/config/example.txt",
            b"client",
        );
        put(
            root.path(),
            "overrides/server/config/example.txt",
            b"server",
        );
        put(root.path(), "templates/common/icons/icon.bin", &[255, 0, 1]);
        put(
            root.path(),
            "templates/client/notes.txt.template",
            b"{{NAME}} {{MODLOADER_VERSION}}",
        );
        put(root.path(), "templates/server/server-only", b"excluded");
        let plan = prepare_client_full_build(
            capture(root.path(), host.path(), name),
            path(name).unwrap(),
            &external,
            &options(format),
            &cancel,
        )
        .unwrap();
        assert!(!root.path().join("dist").exists());
        assert!(!plan.uses_user_configuration());
        let expected = plan.inventory().clone();
        assert!(expected.contains_key(&path(".minecraft/config/example.txt").unwrap()));
        assert!(expected.contains_key(&path(".minecraft/resourcepacks/a.zip").unwrap()));
        assert!(!expected.contains_key(&path("server-only").unwrap()));
        plan.publish(
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
            for (name, expected) in [
                (".minecraft/config/example.txt", "client"),
                ("notes.txt", "Test 0.16.0"),
            ] {
                let mut bytes = String::new();
                zip.by_name(name)
                    .unwrap()
                    .read_to_string(&mut bytes)
                    .unwrap();
                assert_eq!(bytes, expected);
            }
            let mut cfg = String::new();
            zip.by_name("instance.cfg")
                .unwrap()
                .read_to_string(&mut cfg)
                .unwrap();
            assert!(cfg.contains("InstanceType=OneSix"));
            assert!(cfg.lines().any(|line| line == "PreLaunchCommand="));
            assert!(cfg.lines().any(|line| line == "WrapperCommand="));
            assert!(!cfg.contains("packwiz-installer-bootstrap.jar"));
            let profile: Value =
                serde_json::from_reader(zip.by_name("mmc-pack.json").unwrap()).unwrap();
            assert_eq!(profile["components"][0]["version"], "1.20.1");
            assert_eq!(
                profile["components"][1]["uid"],
                "net.fabricmc.fabric-loader"
            );
            assert_eq!(profile["components"][1]["version"], "0.16.0");
        }
    }
}
#[test]
fn profile_contract_preserves_all_loader_families_and_rejects_runtime_drift() {
    for (kind, mc, version, uid, selected) in [
        (LoaderKind::Vanilla, "1.21.1", None, None, None),
        (
            LoaderKind::Fabric,
            "1.20.1",
            Some("0.16.0"),
            Some("net.fabricmc.fabric-loader"),
            Some("0.16.0"),
        ),
        (
            LoaderKind::Quilt,
            "1.20.1",
            Some("0.26.3"),
            Some("org.quiltmc.quilt-loader"),
            Some("0.26.3"),
        ),
        (
            LoaderKind::Forge,
            "1.7.10",
            Some("10.13.4.1614-1.7.10"),
            Some("net.minecraftforge"),
            Some("10.13.4.1614"),
        ),
        (
            LoaderKind::NeoForge,
            "1.20.1",
            Some("47.1.106"),
            Some("net.neoforged"),
            Some("47.1.106"),
        ),
        (
            LoaderKind::NeoForge,
            "1.21.1",
            Some("21.1.209"),
            Some("net.neoforged"),
            Some("21.1.209"),
        ),
    ] {
        let runtime = RuntimeResolution {
            minecraft: GameVersion::parse(mc).unwrap(),
            loader: kind,
            loader_version: version.map(|version| LoaderVersion::parse(version).unwrap()),
        };
        let bytes = profile(&runtime).unwrap();
        verify_profile(&bytes, &runtime).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            value["components"].as_array().unwrap().len(),
            if kind == LoaderKind::Vanilla { 1 } else { 2 }
        );
        if let Some(uid) = uid {
            assert_eq!(value["components"][1]["uid"], uid);
            assert_eq!(value["components"][1]["version"], selected.unwrap());
        }
        for field in ["version", "disabled", "important"] {
            let mut corrupt = value.clone();
            corrupt["components"][0][field] = match field {
                "version" => json!("different"),
                "disabled" => json!(true),
                _ => json!(false),
            };
            assert!(verify_profile(&serde_json::to_vec(&corrupt).unwrap(), &runtime).is_err());
        }
        let mut duplicate = value.clone();
        let component = duplicate["components"][0].clone();
        duplicate["components"]
            .as_array_mut()
            .unwrap()
            .push(component);
        assert!(verify_profile(&serde_json::to_vec(&duplicate).unwrap(), &runtime).is_err());
    }
}
#[test]
fn client_preparation_preserves_user_templates_and_old_artifacts_on_failure() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let external = fixture(root.path());
    let cancel = Cancellation::default();
    put(root.path(), "dist/client.zip", b"previous");
    put(
        root.path(),
        "templates/client/instance.cfg",
        b"[General]\nConfigVersion=1.2\nInstanceType=OneSix\nname=Custom\n",
    );
    put(root.path(), "pack/config/current", b"initial");
    let prepared = prepare_client_full_build(
        capture(root.path(), host.path(), "client.zip"),
        path("client.zip").unwrap(),
        &external,
        &options(DistributionArchive::Zip),
        &cancel,
    )
    .unwrap();
    assert!(prepared.uses_user_configuration());
    put(root.path(), "pack/config/current", b"changed");
    assert!(
        prepared
            .publish(
                &Publisher::open(&host.path().join("private")).unwrap(),
                &cancel
            )
            .is_err()
    );
    assert_eq!(
        fs::read(root.path().join("dist/client.zip")).unwrap(),
        b"previous"
    );
    let rebuilt = prepare_client_full_build(
        capture(root.path(), host.path(), "client.zip"),
        path("client.zip").unwrap(),
        &external,
        &options(DistributionArchive::Zip),
        &cancel,
    )
    .unwrap();
    rebuilt
        .publish(
            &Publisher::open(&host.path().join("private")).unwrap(),
            &cancel,
        )
        .unwrap();
    let previous = fs::read(root.path().join("dist/client.zip")).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&previous)).unwrap();
    let mut current = String::new();
    zip.by_name(".minecraft/config/current")
        .unwrap()
        .read_to_string(&mut current)
        .unwrap();
    assert_eq!(current, "changed");
    let mut settings = String::new();
    zip.by_name("instance.cfg")
        .unwrap()
        .read_to_string(&mut settings)
        .unwrap();
    assert!(settings.contains("name=Custom"));
    put(root.path(),"templates/client/mmc-pack.json",br#"{"formatVersion":1,"components":[{"uid":"net.minecraft","version":"wrong","important":true}]}"#);
    assert!(
        prepare_client_full_build(
            capture(root.path(), host.path(), "client.zip"),
            path("client.zip").unwrap(),
            &external,
            &options(DistributionArchive::Zip),
            &cancel
        )
        .is_err()
    );
    fs::remove_file(root.path().join("templates/client/mmc-pack.json")).unwrap();
    put(
        root.path(),
        "templates/client/.minecraft/config/current",
        b"conflicting template",
    );
    assert!(
        prepare_client_full_build(
            capture(root.path(), host.path(), "client.zip"),
            path("client.zip").unwrap(),
            &external,
            &options(DistributionArchive::Zip),
            &cancel
        )
        .is_err()
    );
    assert_eq!(
        fs::read(root.path().join("dist/client.zip")).unwrap(),
        previous
    );
}

#[test]
fn native_clients_preserve_exact_releases_and_install_into_prism() {
    use crate::engine::{
        instance::*,
        release::{DecodedRelease, ReleaseSource, trust::SelectedSnapshot},
    };
    for (format, name) in [
        (DistributionArchive::Zip, "light.zip"),
        (DistributionArchive::TarGz, "light.tar.gz"),
        (DistributionArchive::SevenZip, "light.7z"),
    ] {
        let root = tempfile::tempdir().unwrap();
        let host = tempfile::tempdir().unwrap();
        let acquired = fixture(root.path());
        put(root.path(), "pack/config/example.txt", b"common");
        put(
            root.path(),
            "overrides/client/config/example.txt",
            b"selected",
        );
        let cancel = Cancellation::default();
        let plan = prepare_client_build(
            capture(root.path(), host.path(), name),
            path(name).unwrap(),
            &acquired,
            &options(format),
            &cancel,
        )
        .unwrap();
        assert_eq!(plan.game().inventory().target(), Recipe::PRISM_REFERENCES);
        assert!(
            !plan
                .inventory()
                .contains_key(&path(".minecraft/resourcepacks/a.zip").unwrap())
        );
        assert!(
            !plan
                .inventory()
                .contains_key(&path(".minecraft/config/example.txt").unwrap())
        );
        assert!(
            plan.inventory()
                .contains_key(&path(".minecraft/.empack-consumer/release.json").unwrap())
        );
        assert!(
            !plan
                .inventory()
                .keys()
                .any(|p| p.as_str().contains("packwiz"))
        );
        let expected = plan.inventory().clone();
        plan.publish(
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
            let instance = tempfile::tempdir().unwrap();
            let mut zip = zip::ZipArchive::new(file).unwrap();
            zip.extract(instance.path()).unwrap();
            let assets = instance.path().join(".minecraft/.empack-consumer");
            let release =
                DecodedRelease::decode(&fs::read(assets.join("release.json")).unwrap()).unwrap();
            let ini = fs::read_to_string(instance.path().join("instance.cfg")).unwrap();
            assert!(ini.contains("instance prepare"));
            assert!(
                ini.lines().any(|line| line.starts_with("WrapperCommand=")
                    && line.contains("instance launch --"))
            );
            assert!(ini.contains(release.id()));
            assert!(ini.contains("--layout prism --side client"));
            let provider = release
                .document()
                .files
                .iter()
                .find(|f| f.destination == "resourcepacks/a.zip")
                .unwrap();
            assert!(matches!(provider.source, ReleaseSource::Url { .. }));
            assert!(provider.asset_path().is_none());
            // Acquisition supplies exactly the bytes named by the exported release.
            let supplied = release
                .document()
                .files
                .iter()
                .filter(|file| matches!(file.source, ReleaseSource::Url { .. }))
                .map(|file| {
                    (
                        file.key.clone(),
                        acquired.locked.values().next().unwrap().content.clone(),
                    )
                })
                .collect();
            let prepare = || {
                crate::engine::instance::plan(
                    instance.path(),
                    InstanceSelection {
                        release: SelectedRelease::Snapshot(
                            SelectedSnapshot::select(
                                release.bytes(),
                                release.id(),
                                &semver::Version::parse("0.6.0-beta").unwrap(),
                            )
                            .unwrap(),
                        ),
                        side: InstanceSide::Client,
                        layout: Some(InstanceLayout::Prism),
                        choices: vec![],
                        action: InstanceAction::Prepare,
                    },
                    RecoveryReader::new(host.path().join("instance-state")),
                    SnapshotLimits::default(),
                    &cancel,
                )
                .unwrap()
            };
            prepare()
                .stage(&supplied, &BTreeMap::new(), Some(&assets), &cancel)
                .unwrap()
                .publish(
                    &Publisher::open(&host.path().join("instance-state")).unwrap(),
                    &cancel,
                )
                .unwrap();
            assert_eq!(
                fs::read(instance.path().join(".minecraft/resourcepacks/a.zip")).unwrap(),
                b"payload"
            );
            assert_eq!(
                fs::read(instance.path().join(".minecraft/config/example.txt")).unwrap(),
                b"selected"
            );
            fs::write(
                instance.path().join(".minecraft/config/example.txt"),
                b"user configuration",
            )
            .unwrap();
            prepare()
                .stage(&BTreeMap::new(), &BTreeMap::new(), None, &cancel)
                .unwrap()
                .publish(
                    &Publisher::open(&host.path().join("instance-state")).unwrap(),
                    &cancel,
                )
                .unwrap();
            assert_eq!(
                fs::read(instance.path().join(".minecraft/config/example.txt")).unwrap(),
                b"user configuration"
            );
        }
    }
}
#[test]
fn native_input_and_future_installed_collisions_preserve_existing_distribution() {
    for destination in [
        "pack/.empack-consumer/release.json",
        "pack/.empack-layout",
        "pack/.EMPACK-CONSUMER/unrelated",
        "templates/client/.minecraft/resourcepacks/a.zip",
        "templates/client/.minecraft/resourcepacks",
        "templates/client/.minecraft/.empack-consumer/release.json",
    ] {
        let root = tempfile::tempdir().unwrap();
        let host = tempfile::tempdir().unwrap();
        let acquired = fixture(root.path());
        put(root.path(), destination, b"user input");
        put(root.path(), "dist/client.zip", b"prior");
        let result = prepare_client_build(
            capture(root.path(), host.path(), "client.zip"),
            path("client.zip").unwrap(),
            &acquired,
            &options(DistributionArchive::Zip),
            &Cancellation::default(),
        );
        assert!(result.is_err(), "collision accepted: {destination}");
        assert_eq!(
            fs::read(root.path().join("dist/client.zip")).unwrap(),
            b"prior"
        );
    }
}
