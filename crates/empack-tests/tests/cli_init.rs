//! Initialization contracts that require no external catalog.
#[path = "support/files.rs"]
mod files;
#[path = "support/initialization.rs"]
mod initialization;
#[path = "support/replacement.rs"]
mod replacement;
#[path = "support/runtime_selection.rs"]
mod runtime_selection;
use empack_tests::harness::process::{
    TestProject, assert_project_initialized, assert_project_loader_absent,
    assert_project_minecraft_version, offline_command,
};

#[test]
fn cli_init_missing_loader_reports_usage() {
    let project = TestProject::new();
    let output = project
        .cmd()
        .args(["init", "--yes", "test-pack"])
        .output()
        .expect("failed to spawn");
    assert!(!output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--modloader") || stderr.contains("requires"),
        "stderr did not mention --modloader or requires\n{stderr}"
    );
}

#[test]
fn cli_init_exact_loader_families_preserve_pins_without_catalog_or_java() {
    use std::{io::ErrorKind, net::TcpListener};
    let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
    proxy.set_nonblocking(true).unwrap();
    let address = format!("http://{}", proxy.local_addr().unwrap());
    for (family, game, supplied, stored, java) in [
        ("none", "1.21.1", None, None, "21"),
        ("fabric", "1.21.1", Some("0.16.9"), Some("0.16.9"), "21"),
        ("quilt", "1.21.1", Some("0.27.1"), Some("0.27.1"), "21"),
        (
            "forge",
            "1.7.10",
            Some("10.13.4.1614-1.7.10"),
            Some("10.13.4.1614"),
            "8",
        ),
        ("forge", "1.21.1", Some("52.0.0"), Some("52.0.0"), "21"),
        (
            "neoforge",
            "1.20.1",
            Some("47.1.100"),
            Some("47.1.100"),
            "21",
        ),
        (
            "neoforge",
            "1.21.1",
            Some("21.1.224"),
            Some("21.1.224"),
            "21",
        ),
    ] {
        let project = TestProject::new();
        let command = || {
            let mut command = offline_command(project.dir());
            for key in [
                "HTTP_PROXY",
                "HTTPS_PROXY",
                "ALL_PROXY",
                "http_proxy",
                "https_proxy",
                "all_proxy",
            ] {
                command.env(key, &address);
            }
            command
                .env("NO_PROXY", "")
                .env("no_proxy", "")
                .env("PATH", "");
            command
        };
        let mut args = vec![
            "init",
            "--yes",
            "--modloader",
            family,
            "--mc-version",
            game,
            "--java",
            java,
            "--author",
            "Fixture Author",
            "pack",
        ];
        if let Some(supplied) = supplied {
            args.extend(["--loader-version", supplied]);
        }
        command().args(&args).assert().success();
        let pack = project.dir().join("pack");
        runtime_selection::assert_selection(
            &pack,
            game,
            if family == "none" { "vanilla" } else { family },
            stored,
            java.parse().unwrap(),
        );
        // Declaring Java requirements and exact pins does not run Java or certify
        // upstream availability. The ordinary proxy must observe no acquisition.
        assert!(matches!(proxy.accept(), Err(error) if error.kind() == ErrorKind::WouldBlock));
        let before = files::snapshot(project.dir());
        args[7] = "7";
        let rejected = command().args(&args).output().unwrap();
        assert_eq!(rejected.status.code(), Some(2));
        assert!(
            String::from_utf8_lossy(&rejected.stderr)
                .contains("invalid value '7' for '--java <JAVA>'"),
            "{rejected:?}"
        );
        assert_eq!(files::snapshot(project.dir()), before);
        assert!(matches!(proxy.accept(), Err(error) if error.kind() == ErrorKind::WouldBlock));
    }
}

#[test]
fn cli_init_emits_versioned_automation_and_templates() {
    let project = TestProject::new();
    let output = project.run_output(&[
        "init",
        "--yes",
        "--modloader",
        "none",
        "--mc-version",
        "1.21.1",
        "test-pack",
    ]);
    assert!(output.status.success(), "{:?}", output);

    let pack_dir = project.dir().join("test-pack");
    assert!(pack_dir.join(".gitignore").exists(), ".gitignore not found");
    for name in ["validate", "release"] {
        let bytes = std::fs::read_to_string(pack_dir.join(format!(".github/workflows/{name}.yml")))
            .unwrap();
        let workflow: serde_json::Value = serde_saphyr::from_str(&bytes).unwrap();
        let version = empack_tests::harness::process::expected_automation_version();
        assert_eq!(workflow["env"]["EMPACK_VERSION"], format!("v{version}"));
        assert!(!bytes.contains("__EMPACK_RELEASE_TAG__"));
        assert!(bytes.contains(&format!("python3 .github/scripts/empack-ci.py {name}")));
        assert!(pack_dir.join(".github/scripts/empack-ci.py").is_file());
        assert!(!bytes.contains("packwiz"));
    }
    assert!(
        !pack_dir.join("pack").join(".packwizignore").exists(),
        "initialization must not create foreign control files"
    );
    assert!(
        pack_dir.join("templates").join("server").is_dir(),
        "templates/server/ not found"
    );
    assert!(
        pack_dir.join("templates").join("client").is_dir(),
        "templates/client/ not found"
    );
}

#[test]
fn cli_init_custom_content_routes_preserve_explicit_paths_and_refuse_traversal() {
    use empack_tests::harness::workspace::snapshot;
    use std::fs;

    let project = TestProject::new();
    fs::create_dir_all(project.dir().join("neighbor/worlds/playtest")).unwrap();
    fs::write(
        project.dir().join("neighbor/empack.yml"),
        b"unrelated author",
    )
    .unwrap();
    fs::write(
        project.dir().join("neighbor/worlds/playtest/level.dat"),
        b"player bytes",
    )
    .unwrap();
    let before = snapshot(project.dir());
    for flag in ["--datapack-folder", "--world-folder"] {
        let output = offline_command(project.dir())
            .args([
                "--yes",
                "init",
                "--modloader",
                "none",
                "--mc-version",
                "1.20.1",
                flag,
                "../escape",
                "refused",
            ])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{flag}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("file path contains an empty or traversal component"),
            "{flag}: {output:?}"
        );
        assert_eq!(snapshot(project.dir()), before);
        assert!(!project.dir().join("refused").exists());
    }
    for (datapacks, worlds) in [
        ("datapacks", "saves"),
        ("config/paxi/datapacks Ω", "saved worlds Ω"),
    ] {
        let project = TestProject::new();
        let output = project.run_output(&[
            "init",
            "--yes",
            "--modloader",
            "none",
            "--mc-version",
            "1.20.1",
            "--datapack-folder",
            datapacks,
            "--world-folder",
            worlds,
            "test-pack",
        ]);
        assert!(output.status.success(), "{output:?}");
        let pack_dir = project.dir().join("test-pack");
        assert_project_initialized(&pack_dir);
        assert_project_loader_absent(&pack_dir);
        assert_project_minecraft_version(&pack_dir, "1.20.1");
        let author: serde_json::Value =
            serde_saphyr::from_str(&fs::read_to_string(pack_dir.join("empack.yml")).unwrap())
                .unwrap();
        assert_eq!(
            author["layout"],
            serde_json::json!({"data-pack": datapacks, "world": worlds})
        );
        let before = snapshot(project.dir());
        let output = offline_command(&pack_dir)
            .args(["manifest", "check", "--json"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["author_schema"], 5);
        assert_eq!(snapshot(project.dir()), before);
    }
}

#[test]
fn cli_init_preview_preserves_absent_destination() {
    let project = TestProject::new();
    let output = project.run_output(&[
        "init",
        "--dry-run",
        "--yes",
        "--modloader",
        "none",
        "--mc-version",
        "1.20.1",
        "test-dry-run",
    ]);
    assert!(
        output.status.success(),
        "init --dry-run should exit 0: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!project.dir().join("test-dry-run").exists());
}

#[test]
fn cli_init_vanilla_binds_minecraft_without_loader() {
    let mut identities = std::collections::BTreeSet::new();
    for (destination, name, version, author) in [
        (Some("test-none"), "test-none", "1.0.0", ""),
        (
            Some("authored pack Ω"),
            "Workshop Ω: recipes",
            "2.3.0",
            "Fixture Author",
        ),
        (None, "Current directory", "0.1.0", "Local Author"),
    ] {
        let project = TestProject::new();
        let host = tempfile::tempdir().unwrap();
        let command = |directory: &std::path::Path| {
            let mut command = offline_command(directory);
            command
                .env("EMPACK_STATE_DIR", host.path().join("state"))
                .env("EMPACK_CACHE_DIR", host.path().join("cache"));
            command
        };
        let mut args = vec![
            "init",
            "--yes",
            "--modloader",
            "none",
            "--mc-version",
            "1.21.1",
            "--author",
            author,
        ];
        if !author.is_empty() {
            args.extend(["--pack-name", name, "--pack-version", version]);
        }
        if let Some(destination) = destination {
            args.push(destination);
        }
        command(project.dir()).args(&args).assert().success();
        let pack = destination.map_or_else(
            || project.dir().to_path_buf(),
            |dir| project.dir().join(dir),
        );
        let authors = if author.is_empty() {
            vec![]
        } else {
            vec![author]
        };
        let id = initialization::assert_new_project(&pack, name, version, &authors);
        assert!(
            identities.insert(id.clone()),
            "separate new projects need separate identities"
        );
        let before = files::snapshot(&pack);
        for _ in 0..2 {
            command(&pack).args(["sync", "--yes"]).assert().success();
            assert_eq!(
                files::snapshot(&pack),
                before,
                "empty sync must preserve authored identity and scaffolding"
            );
        }
        let output = command(project.dir()).args(&args).output().unwrap();
        assert!(!output.status.success());
        let diagnostic = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            diagnostic.contains("replacement requires an explicit decision"),
            "{diagnostic}"
        );
        assert_eq!(
            files::snapshot(&pack),
            before,
            "refused second initialization must preserve every file"
        );
        assert_eq!(
            initialization::assert_new_project(&pack, name, version, &authors),
            id
        );
    }
}
