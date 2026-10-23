#[path = "support/files.rs"]
mod files;

use empack_tests::harness::process::{TestProject, offline_command};
use predicates::prelude::*;

macro_rules! cli_bad_flag_value {
    ($name:ident, args: [$($arg:expr),+], stderr_contains: $expected:expr) => {
        #[test]
        fn $name() {
            let project = TestProject::new();
            let mut cmd = offline_command(project.dir());
            $(cmd.arg($arg);)+
            cmd.assert()
                .failure()
                .stderr(predicate::str::contains($expected));
        }
    };
}

cli_bad_flag_value!(
    cli_matrix_bad_archive_format,
    args: ["build", "--format", "csv", "modrinth"],
    stderr_contains: "invalid value 'csv'"
);

cli_bad_flag_value!(
    cli_matrix_bad_platform,
    args: ["add", "--platform", "github", "sodium"],
    stderr_contains: "invalid value 'github'"
);

cli_bad_flag_value!(
    cli_matrix_bad_project_type,
    args: ["add", "--type", "invalid-content", "sodium"],
    stderr_contains: "invalid value 'invalid-content'"
);

macro_rules! cli_requires_modpack {
    ($name:ident, args: [$($arg:expr),+]) => {
        #[test]
        fn $name() {
            let project = TestProject::new();
            let output = offline_command(project.dir())
                .args([$($arg),+])
                .output()
                .expect("failed to spawn");
            assert!(
                !output.status.success(),
                "command should fail in empty directory"
            );
        }
    };
}

cli_requires_modpack!(cli_matrix_add_requires_modpack, args: ["add", "sodium"]);
cli_requires_modpack!(cli_matrix_remove_requires_modpack, args: ["remove", "sodium"]);
cli_requires_modpack!(cli_matrix_sync_requires_modpack, args: ["sync"]);
cli_requires_modpack!(cli_matrix_build_requires_modpack, args: ["build", "modrinth"]);
// clean in an empty directory exits 0 ("nothing to clean" is valid)

fn configuration_command(root: &std::path::Path, layer: usize) -> assert_cmd::Command {
    let mut command = offline_command(root);
    command
        .env_remove("EMPACK_STATE_DIR")
        .env_remove("EMPACK_CACHE_DIR")
        .env_remove("EMPACK_WORKDIR");
    if layer >= 2 {
        for (key, stem) in [
            ("EMPACK_WORKDIR", "pack"),
            ("EMPACK_STATE_DIR", "state"),
            ("EMPACK_CACHE_DIR", "cache"),
        ] {
            command.env(key, format!("{stem} Ω layer 2"));
        }
    }
    if layer == 3 {
        for (flag, stem) in [
            ("--workdir", "pack"),
            ("--state-dir", "state"),
            ("--cache-dir", "cache"),
        ] {
            command.arg(flag).arg(format!("{stem} Ω layer 3"));
        }
    }
    command
}

#[test]
fn cli_configuration_precedence_selects_workspace_and_invocation_relative_storage() {
    use std::{
        fs,
        io::{Cursor, Write},
    };
    let mut jar = zip::ZipWriter::new(Cursor::new(Vec::new()));
    jar.start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    jar.write_all(br#"{"schemaVersion":1,"id":"fixture","version":"1"}"#)
        .unwrap();
    let jar = jar.finish().unwrap().into_inner();
    let input = "inputs Ω with spaces/fixture.jar";
    for layer in 0..4 {
        let project = TestProject::new();
        let root = project.dir();
        for index in 0..4 {
            let pack = root.join(format!("pack Ω layer {index}"));
            fs::create_dir_all(pack.join("inputs Ω with spaces")).unwrap();
            fs::write(pack.join("notes.txt"), b"unselected author work\n").unwrap();
            // --workdir does not change where dotenv or relative inputs are read.
            fs::write(pack.join(".env"), b"EMPACK_DRY_RUN='unterminated\n").unwrap();
            fs::write(pack.join(input), b"not the invocation's input jar").unwrap();
        }
        fs::create_dir(root.join("inputs Ω with spaces")).unwrap();
        fs::write(root.join(input), &jar).unwrap();
        for (index, filename) in [(0, ".env"), (1, ".env.local")] {
            if index <= layer {
                fs::write(root.join(filename), format!(
                    "EMPACK_WORKDIR='pack Ω layer {index}'\nEMPACK_STATE_DIR='state Ω layer {index}'\nEMPACK_CACHE_DIR='cache Ω layer {index}'\n"
                )).unwrap();
            }
        }
        let before = files::snapshot(root);
        let init = [
            "init",
            "--yes",
            "--modloader",
            "none",
            "--mc-version",
            "1.21.1",
            "--pack-name",
            "Arguments Ω with spaces",
            "--pack-version",
            "1",
        ];
        configuration_command(root, layer)
            .args(init)
            .arg("--dry-run")
            .assert()
            .success();
        assert_eq!(files::snapshot(root), before, "preview at layer {layer}");
        for index in 0..4 {
            assert!(!root.join(format!("state Ω layer {index}")).exists());
            assert!(!root.join(format!("cache Ω layer {index}")).exists());
        }
        configuration_command(root, layer)
            .args(init)
            .assert()
            .success();
        let selected = root.join(format!("pack Ω layer {layer}"));
        let author: serde_json::Value =
            serde_saphyr::from_str(&fs::read_to_string(selected.join("empack.yml")).unwrap())
                .unwrap();
        assert_eq!(
            author["pack"]["name"].as_str(),
            Some("Arguments Ω with spaces")
        );
        let initialized = files::snapshot(root);
        configuration_command(root, layer)
            .args(["--yes", "--dry-run", "add", input])
            .assert()
            .success();
        assert_eq!(
            files::snapshot(root),
            initialized,
            "add preview at layer {layer}"
        );
        configuration_command(root, layer)
            .args(["--yes", "add", input])
            .assert()
            .success();
        assert_eq!(
            fs::read(selected.join("pack/mods/fixture.jar")).unwrap(),
            jar
        );
        assert_eq!(fs::read(root.join(input)).unwrap(), jar);
        assert!(root.join(format!("state Ω layer {layer}")).is_dir());
        assert!(
            root.join(format!("cache Ω layer {layer}/content-v1"))
                .is_dir()
        );
        for index in 0..4 {
            let prefix = std::path::PathBuf::from(format!("pack Ω layer {index}"));
            if index != layer {
                let retained: std::collections::BTreeMap<_, _> = before
                    .iter()
                    .filter(|(path, _)| path.starts_with(&prefix))
                    .map(|(path, bytes)| {
                        (
                            path.strip_prefix(&prefix).unwrap().to_path_buf(),
                            bytes.clone(),
                        )
                    })
                    .collect();
                assert_eq!(
                    files::snapshot(&root.join(&prefix)),
                    retained,
                    "unselected layer {index}"
                );
                assert!(!root.join(format!("state Ω layer {index}")).exists());
                assert!(!root.join(format!("cache Ω layer {index}")).exists());
            }
            assert_eq!(
                fs::read(root.join(&prefix).join(input)).unwrap(),
                b"not the invocation's input jar"
            );
        }
        assert!(!root.join(".empack-cache").exists());
        assert!(!root.join(".host-state").exists());
        assert!(!root.join("empack.yml").exists());
        assert!(!selected.join(format!("state Ω layer {layer}")).exists());
        assert!(!selected.join(format!("cache Ω layer {layer}")).exists());
        for (path, bytes) in &before {
            assert_eq!(fs::read(root.join(path)).unwrap(), *bytes, "{path:?}");
        }
    }
}

#[test]
fn cli_invalid_storage_roots_refuse_before_project_creation() {
    for flag in ["--state-dir", "--cache-dir"] {
        let project = TestProject::new();
        std::fs::write(project.dir().join("notes.txt"), b"author notes").unwrap();
        let before = files::snapshot(project.dir());
        offline_command(project.dir())
            .args([
                flag,
                "",
                "--yes",
                "init",
                "new pack",
                "--modloader",
                "none",
                "--mc-version",
                "1.21.1",
            ])
            .assert()
            .code(2)
            .stderr(
                predicate::str::contains("a value is required").and(predicate::str::contains(flag)),
            );
        assert_eq!(files::snapshot(project.dir()), before);
        for absent in ["new pack", ".host-state", ".empack-cache"] {
            assert!(!project.dir().join(absent).exists());
        }
    }
}

#[test]
fn cli_dry_run_precedence_reaches_publication() {
    for (local, environment, cli, preserved) in [
        (None, None, false, true),
        (Some(false), None, false, false),
        (Some(true), Some(false), false, false),
        (Some(true), Some(false), true, true),
    ] {
        let project = TestProject::new();
        offline_command(project.dir())
            .args([
                "init",
                "--yes",
                "--modloader",
                "none",
                "--mc-version",
                "1.21.1",
                "--pack-name",
                "precedence",
                "--pack-version",
                "1",
            ])
            .assert()
            .success();
        std::fs::write(project.dir().join(".env"), "EMPACK_DRY_RUN=true\n").unwrap();
        if let Some(value) = local {
            std::fs::write(
                project.dir().join(".env.local"),
                format!("EMPACK_DRY_RUN={value}\n"),
            )
            .unwrap();
        }
        std::fs::create_dir(project.dir().join("dist")).unwrap();
        let artifact = project.dir().join("dist/keep.zip");
        std::fs::write(&artifact, "artifact").unwrap();
        let manifest = std::fs::read(project.dir().join("empack.yml")).unwrap();
        let lock = std::fs::read(project.dir().join("empack.lock")).unwrap();
        let before = files::snapshot(project.dir());
        let mut cmd = offline_command(project.dir());
        if let Some(value) = environment {
            cmd.env("EMPACK_DRY_RUN", value.to_string());
        }
        cmd.args(["clean", "builds", "--yes"]);
        if cli {
            cmd.arg("--dry-run");
        }
        cmd.assert().success();
        if preserved {
            assert_eq!(files::snapshot(project.dir()), before);
        }
        assert_eq!(
            std::fs::read(project.dir().join("empack.yml")).unwrap(),
            manifest
        );
        assert_eq!(
            std::fs::read(project.dir().join("empack.lock")).unwrap(),
            lock
        );
        assert_eq!(
            artifact.exists(),
            preserved,
            "local={local:?}, env={environment:?}, cli={cli}"
        );
    }
}
