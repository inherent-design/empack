use empack_lib::networking::cache::CachedResponse;
use empack_tests::e2e::{TestProject, configure_fake_packwiz};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};

const SEARCH_URL: &str = concat!(
    "https://api.modrinth.com/v2/search?query=Sodium&facets=",
    "%5B%5B%22project%5Ftype%3Amod%22%5D%2C",
    "%5B%22versions%3A1%2E21%2E1%22%5D%2C",
    "%5B%22categories%3Afabric%22%5D%5D"
);

const MANIFEST: &str = r#"# A preview must retain comments and formatting.
empack:
  minecraft_version: "1.21.1"
  loader: fabric
  dependencies:
    sodium:
      title: Sodium
      type: mod
      platform: modrinth
"#;

fn search_project(status: u16) -> TestProject {
    let project = TestProject::workflow_fixture("smoke", "fabric", "1.21.1");
    std::fs::write(project.dir().join("empack.yml"), MANIFEST).unwrap();
    let response = CachedResponse {
        data: serde_json::to_vec(&serde_json::json!({"hits": [{
            "project_id": "AANobbMI",
            "slug": "sodium",
            "title": "Sodium",
            "downloads": 100,
            "categories": ["fabric"],
            "versions": ["1.21.1"]
        }]}))
        .unwrap(),
        etag: None,
        expires: SystemTime::now() + Duration::from_secs(300),
        status,
    };
    let cache = project.dir().join(".empack-cache/http");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(
        cache.join("http_cache.json"),
        serde_json::to_vec(&HashMap::from([(SEARCH_URL, response)])).unwrap(),
    )
    .unwrap();
    project
}

fn snapshot(project: &TestProject) -> Vec<String> {
    ["empack.yml", "pack/pack.toml", "pack/index.toml"]
        .into_iter()
        .map(|path| std::fs::read_to_string(project.dir().join(path)).unwrap())
        .collect()
}

fn command(project: &TestProject) -> assert_cmd::Command {
    let mut cmd = project.cmd();
    configure_fake_packwiz(&mut cmd, project.dir());
    cmd.env_remove("EMPACK_WORKDIR")
        .env_remove("EMPACK_DRY_RUN")
        .env("EMPACK_COLOR", "never")
        .env("EMPACK_NET_TIMEOUT", "1")
        .env("HTTP_PROXY", "http://127.0.0.1:9")
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .env("ALL_PROXY", "http://127.0.0.1:9")
        .env("NO_PROXY", "");
    let mut cmd = assert_cmd::Command::from_std(cmd);
    cmd.timeout(Duration::from_secs(10));
    cmd
}

#[test]
fn smoke_sync_search_preview_preserves_project_and_shows_add() {
    let project = search_project(200);
    let before = snapshot(&project);
    let result = command(&project)
        .args(["sync", "--dry-run"])
        .assert()
        .success();
    let output = result.get_output();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(text.contains("Add: Sodium"), "{text}");
    assert!(text.contains("--project-id AANobbMI"), "{text}");
    assert_eq!(snapshot(&project), before, "preview changed project files");
}

#[test]
fn smoke_sync_provider_failure_is_nonzero_and_preserves_project() {
    for dry_run in [false, true] {
        let project = search_project(503);
        let before = snapshot(&project);
        let mut cmd = command(&project);
        cmd.arg("sync");
        if dry_run {
            cmd.arg("--dry-run");
        }
        cmd.assert()
            .code(3)
            .stderr(predicates::str::contains("status 503"));
        assert_eq!(snapshot(&project), before);
    }
}

#[test]
fn smoke_sync_empty_project_is_successful_and_preserves_project() {
    let project = TestProject::workflow_fixture("smoke", "fabric", "1.21.1");
    let before = snapshot(&project);
    command(&project).arg("sync").assert().success();
    assert_eq!(snapshot(&project), before);
}

#[test]
fn smoke_dotenv_precedence_reaches_command_execution() {
    for (local, environment, cli, preserved) in [
        (None, None, false, true),
        (Some(false), None, false, false),
        (Some(true), Some(false), false, false),
        (Some(true), Some(false), true, true),
    ] {
        let project = TestProject::workflow_fixture("dotenv", "fabric", "1.21.1");
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
        let mut cmd = command(&project);
        if let Some(value) = environment {
            cmd.env("EMPACK_DRY_RUN", value.to_string());
        }
        cmd.args(["clean", "builds"]);
        if cli {
            cmd.arg("--dry-run");
        }
        cmd.assert().success();
        assert_eq!(
            artifact.exists(),
            preserved,
            "local={local:?}, env={environment:?}, cli={cli}"
        );
    }
}

#[test]
fn smoke_malformed_dotenv_is_a_configuration_error() {
    let project = TestProject::new();
    std::fs::write(project.dir().join(".env"), "EMPACK_DRY_RUN='unterminated\n").unwrap();
    command(&project).arg("version").assert().code(2);
}

#[test]
fn smoke_sync_recognizes_installed_datapacks() {
    for (folder, source) in [
        ("datapacks", "default"),
        ("config/paxi/datapacks", "yaml"),
        ("config/openloader/data", "pack"),
        ("../shared/datapacks", "yaml"),
        ("../shared/datapacks", "pack"),
    ] {
        let project = TestProject::workflow_fixture("datapacks", "fabric", "1.21.1");
        let mut manifest = MANIFEST
            .replace(
                "title: Sodium",
                "status: resolved\n      project_id: AANobbMI\n      title: Sodium",
            )
            .replace("type: mod", "type: datapack");
        if source == "yaml" {
            manifest.push_str(&format!("  datapack_folder: {folder}\n"));
        }
        std::fs::write(project.dir().join("empack.yml"), manifest).unwrap();
        if source == "pack" {
            let path = project.dir().join("pack/pack.toml");
            let content = std::fs::read_to_string(&path).unwrap();
            std::fs::write(
                path,
                format!("{content}\n[options]\ndatapack-folder = \"{folder}\"\n"),
            )
            .unwrap();
        }
        let dir = project.dir().join("pack").join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("sodium.pw.toml"), "name = \"Sodium\"\n").unwrap();
        let before = snapshot(&project);
        command(&project)
            .args(["sync", "--dry-run"])
            .assert()
            .success()
            .stdout(predicates::str::contains("No changes needed"));
        assert_eq!(snapshot(&project), before);
    }
}

#[test]
fn smoke_sync_unreadable_installed_state_fails_closed() {
    for dry_run in [false, true] {
        let project = search_project(200);
        std::fs::write(project.dir().join("pack/mods"), "not a directory").unwrap();
        let before = snapshot(&project);
        let mut cmd = command(&project);
        cmd.arg("sync");
        if dry_run {
            cmd.arg("--dry-run");
        }
        cmd.assert().failure();
        assert_eq!(snapshot(&project), before);
    }
}

#[test]
fn smoke_malformed_dotenv_does_not_block_clap_help_or_version() {
    for filename in [".env", ".env.local"] {
        let project = TestProject::new();
        std::fs::write(
            project.dir().join(filename),
            "EMPACK_DRY_RUN='unterminated\n",
        )
        .unwrap();
        for args in [
            vec!["--help"],
            vec!["--version"],
            vec!["init", "--help"],
            vec!["build", "--help"],
        ] {
            command(&project)
                .args(args)
                .assert()
                .success()
                .stdout(predicates::str::contains("empack"));
        }
    }
}
