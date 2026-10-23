//! Shared scenario; each caller declares whether runtime acquisition is live.
use empack_tests::harness::archive::zip_member;
use empack_tests::harness::process::{TestProject, assert_dist_artifact_suffix, offline_command};

pub fn export_prepare(targets: &[&str], live: bool) {
    use empack_core::{model::PlacementIntent, requirements::Requirement};
    use std::io::Write;
    let project = TestProject::workflow_fixture("local-content", "fabric", "1.21.1");
    project.configure_native_distribution("local.content", 21);
    let command = || {
        if live {
            let mut cmd = assert_cmd::Command::from_std(project.cmd());
            cmd.timeout(std::time::Duration::from_secs(240));
            cmd
        } else {
            offline_command(project.dir())
        }
    };
    let source = project.dir().join("local.zip");
    let mut archive = zip::ZipWriter::new(std::fs::File::create(&source).unwrap());
    archive
        .start_file("pack.mcmeta", zip::write::SimpleFileOptions::default())
        .unwrap();
    archive
        .write_all(br#"{"pack":{"pack_format":34,"description":"test assets"}}"#)
        .unwrap();
    archive
        .start_file(
            "assets/test/data.bin",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
    archive.write_all(b"tracked local resource bytes").unwrap();
    archive.finish().unwrap();
    let bytes = std::fs::read(&source).unwrap();
    command()
        .args(["--yes", "add", "--type", "resourcepack", "local.zip"])
        .assert()
        .success();
    // This fixture deliberately distributes the resource file to both environments.
    let manifest = project.dir().join("empack.yml");
    let mut intent = empack_lib::testing::read_authoring(&manifest)
        .unwrap()
        .intent()
        .clone();
    let root = intent.roots.values_mut().next().unwrap();
    root.requirements.server = Requirement::Required;
    if let PlacementIntent::Explicit(placements) = &mut root.placement {
        let mut values = placements.clone().into_vec();
        for placement in &mut values {
            placement.requirements.server = Requirement::Required;
        }
        *placements = empack_core::model::NonEmpty::new(values).unwrap();
    }
    empack_lib::testing::write_authoring(&manifest, &intent).unwrap();
    command().args(["--yes", "sync"]).assert().success();
    for &target in targets {
        command()
            .args([
                "--yes",
                "build",
                "--ad-hoc",
                target,
                "--delivery",
                "references",
            ])
            .timeout(std::time::Duration::from_secs(240))
            .assert()
            .success();
        let suffix = if target == "modrinth" {
            ".mrpack".to_string()
        } else {
            format!("-{target}-references.zip")
        };
        let artifact = assert_dist_artifact_suffix(project.dir(), &suffix);
        if target == "modrinth" {
            assert_eq!(
                zip_member(&artifact, "overrides/resourcepacks/local.zip"),
                bytes
            );
        } else {
            let instance = tempfile::tempdir().unwrap();
            zip::ZipArchive::new(std::fs::File::open(&artifact).unwrap())
                .unwrap()
                .extract(instance.path())
                .unwrap();
            let (descriptor, layout, side, game) = if target == "prism" {
                (
                    ".minecraft/.empack-consumer/release.json",
                    "prism",
                    "client",
                    ".minecraft",
                )
            } else {
                (".empack-consumer/release.json", "game", "server", "game")
            };
            let descriptor = instance.path().join(descriptor);
            let release = empack_lib::engine::release::DecodedRelease::decode(
                &std::fs::read(&descriptor).unwrap(),
            )
            .unwrap();
            command()
                .arg("--workdir")
                .arg(instance.path())
                .args(["--yes", "instance", "prepare"])
                .arg(&descriptor)
                .args(["--sha256", release.id(), "--layout", layout, "--side", side])
                .assert()
                .success();
            assert_eq!(
                std::fs::read(instance.path().join(game).join("resourcepacks/local.zip")).unwrap(),
                bytes,
                "{target}"
            );
        }
    }
    std::fs::create_dir(project.dir().join("vendor")).unwrap();
    std::fs::write(project.dir().join("vendor/local.zip"), &bytes).unwrap();
    let mut changed = empack_lib::testing::read_authoring(&manifest)
        .unwrap()
        .intent()
        .clone();
    changed.roots.values_mut().next().unwrap().source = empack_core::model::SourceIntent::Local(
        empack_core::path::PortableRelPath::parse(
            "vendor/local.zip",
            empack_core::path::PathSyntax::ProjectContent,
        )
        .unwrap(),
    );
    empack_lib::testing::write_authoring(&manifest, &changed).unwrap();
    command()
        .args([
            "--dry-run",
            "build",
            "--ad-hoc",
            "prism",
            "--delivery",
            "references",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("intent"));
    // Project-relative authoring sources outside pack are valid after explicit reconciliation.
    command().args(["--yes", "sync"]).assert().success();
    assert_eq!(
        std::fs::read(project.dir().join("pack/resourcepacks/local.zip")).unwrap(),
        bytes
    );
    command()
        .args(["--yes", "build", "modrinth"])
        .assert()
        .success();
    assert_eq!(
        zip_member(
            &assert_dist_artifact_suffix(project.dir(), ".mrpack"),
            "overrides/resourcepacks/local.zip"
        ),
        bytes
    );
}
