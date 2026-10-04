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
        std::fs::write(dir.join("sodium.pw.toml"), "name = \"Sodium\"\n[update.modrinth]\nmod-id = \"AANobbMI\"\nversion = \"installed\"\n").unwrap();
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

#[test]
fn smoke_sync_rejects_non_string_datapack_option_without_changes() {
    let project = search_project(200);
    let path = project.dir().join("pack/pack.toml");
    let content = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        format!("{content}\n[options]\ndatapack-folder = 42\n"),
    )
    .unwrap();
    let before = snapshot(&project);
    command(&project)
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "options.datapack-folder must be a string",
        ));
    assert_eq!(snapshot(&project), before);
}

#[test]
fn smoke_sync_rejects_identity_drift_without_mutation() {
    for (platform, id, version, success) in [
        ("modrinth", "AANobbMI", None, true),
        ("modrinth", "AANobbMI", Some("installed"), true),
        ("modrinth", "different-project", None, false),
        ("curseforge", "12345", None, false),
    ] {
        for dry_run in [true, false] {
            let project = TestProject::workflow_fixture("drift", "fabric", "1.21.1");
            let mut manifest = format!(
                "empack:\n  minecraft_version: '1.21.1'\n  loader: fabric\n  dependencies:\n    sodium:\n      status: resolved\n      title: Sodium\n      platform: {platform}\n      project_id: '{id}'\n"
            );
            if let Some(version) = version {
                manifest.push_str(&format!("      version: '{version}'\n"));
            }
            std::fs::write(project.dir().join("empack.yml"), manifest).unwrap();
            let mods = project.dir().join("pack/mods");
            std::fs::create_dir_all(&mods).unwrap();
            let metadata =
                "name = 'Sodium'\n[update.modrinth]\nmod-id = 'AANobbMI'\nversion = 'installed'\n";
            std::fs::write(mods.join("sodium.pw.toml"), metadata).unwrap();
            let before = snapshot(&project);
            let mut cmd = command(&project);
            cmd.arg("sync");
            if dry_run {
                cmd.arg("--dry-run");
            }
            if success {
                cmd.assert().success();
            } else {
                cmd.assert().failure().stderr(predicates::str::contains(
                    "Automatic replacement is not supported",
                ));
            }
            assert_eq!(snapshot(&project), before);
            assert_eq!(
                std::fs::read_to_string(mods.join("sodium.pw.toml")).unwrap(),
                metadata
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn smoke_interrupt_preserves_marker_and_excludes_concurrent_mutation() {
    use std::os::unix::fs::PermissionsExt;
    let project = TestProject::workflow_fixture("interrupt", "fabric", "1.21.1");
    let tool = project.dir().join("slow-packwiz");
    std::fs::write(
        &tool,
        "#!/bin/sh\nprintf started > process-started\nsleep 30 &\nwait\n",
    )
    .unwrap();
    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut cmd = project.cmd();
    cmd.env("EMPACK_PACKWIZ_BIN", &tool)
        .env("EMPACK_PROCESS_TIMEOUT_SECS", "10")
        .args(["build", "mrpack"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let mut child = cmd.spawn().unwrap();
    let started = std::time::Instant::now();
    while !project.dir().join("process-started").exists() {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("build exited before fake tool: {status}");
        }
        if started.elapsed() > Duration::from_secs(8) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("build did not reach fake tool");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let marker = project.dir().join(".empack-state");
    assert!(marker.exists());
    command(&project)
        .args(["clean", "builds"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("Project is busy"));
    assert!(
        std::process::Command::new("kill")
            .args(["-INT", &child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let interrupted = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if interrupted.elapsed() > Duration::from_secs(5) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("interrupted command did not terminate");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(status.code(), Some(130));
    assert_eq!(std::fs::read_to_string(marker).unwrap(), "building");
    command(&project)
        .args(["clean", "builds"])
        .assert()
        .success();
}

#[test]
fn smoke_version_does_not_resolve_managed_tooling() {
    let project = TestProject::new();
    command(&project)
        .env_remove("EMPACK_PACKWIZ_BIN")
        .env("PATH", "")
        .arg("version")
        .assert()
        .success();
    assert!(!project.dir().join(".empack-cache/bin").exists());
}

#[test]
fn smoke_sync_matches_aliases_and_retains_transitive_metadata() {
    let project = TestProject::workflow_fixture("identity", "fabric", "1.21.1");
    std::fs::write(project.dir().join("empack.yml"), "empack:\n  minecraft_version: '1.21.1'\n  loader: fabric\n  dependencies:\n    renderer-alias:\n      status: resolved\n      title: Renderer\n      platform: modrinth\n      project_id: AANobbMI\n").unwrap();
    let mods = project.dir().join("pack/mods");
    std::fs::create_dir_all(&mods).unwrap();
    let root = "name = 'Renderer'\n[update.modrinth]\nmod-id = 'AANobbMI'\nversion = 'v1'\n";
    let dependency = "name = 'Library'\n[update.modrinth]\nmod-id = 'required'\nversion = 'v2'\n";
    std::fs::write(mods.join("canonical.pw.toml"), root).unwrap();
    std::fs::write(mods.join("required.pw.toml"), dependency).unwrap();
    let before = snapshot(&project);
    for _ in 0..2 {
        command(&project).arg("sync").assert().success();
        assert_eq!(
            std::fs::read_to_string(mods.join("canonical.pw.toml")).unwrap(),
            root
        );
        assert_eq!(
            std::fs::read_to_string(mods.join("required.pw.toml")).unwrap(),
            dependency
        );
        assert_eq!(snapshot(&project), before);
    }
}

#[test]
fn smoke_sync_detects_backend_success_without_reconciliation() {
    let project = TestProject::workflow_fixture("postcondition", "fabric", "1.21.1");
    std::fs::write(project.dir().join("empack.yml"), "empack:\n  minecraft_version: '1.21.1'\n  loader: fabric\n  dependencies:\n    missing:\n      status: resolved\n      title: Missing\n      platform: modrinth\n      project_id: AANobbMI\n").unwrap();
    // The generic fake returns success but creates no dependency metadata.
    command(&project)
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "Backend reported success but installed dependencies",
        ));
}

#[cfg(unix)]
#[test]
fn smoke_pinned_add_then_sync_preserves_required_content_and_updates_pin() {
    use std::os::unix::fs::PermissionsExt;
    for (platform, pin_flag, first, second) in [
        ("modrinth", "--version-id", "v1", "v2"),
        ("curseforge", "--file-id", "101", "102"),
    ] {
        let project = TestProject::workflow_fixture("root-closure", "fabric", "1.21.1");
        let responses = if platform == "modrinth" {
            vec![
                (
                    "https://api.modrinth.com/v2/project/12345".into(),
                    serde_json::json!({"id":"12345","title":"Root","project_type":"mod"}),
                ),
                (
                    format!("https://api.modrinth.com/v2/version/{first}"),
                    serde_json::json!({"id":first,"project_id":"12345"}),
                ),
            ]
        } else {
            vec![
                (
                    "https://api.curseforge.com/v1/mods/12345".into(),
                    serde_json::json!({"data":{"id":12345,"name":"Root","classId":6}}),
                ),
                (
                    format!("https://api.curseforge.com/v1/mods/12345/files/{first}"),
                    serde_json::json!({"data":{"id":first.parse::<u64>().unwrap(),"modId":12345}}),
                ),
            ]
        };
        cache_responses(&project, responses);
        let tool = project.dir().join("install-fixture");
        std::fs::write(&tool, r#"#!/bin/sh
provider=$1
shift
[ "$1" = add ] || exit 0
shift
while [ "$#" -gt 0 ]; do
  case "$1" in
    --project-id|--addon-id) id=$2; shift 2;;
    --version-id|--file-id) pin=$2; shift 2;;
    *) shift;;
  esac
done
printf 'add\n' >> ../backend-calls
mkdir -p mods
if [ "$provider" = modrinth ]; then
  printf "name = 'Root'\n[update.modrinth]\nmod-id = '%s'\nversion = '%s'\n" "$id" "$pin" > mods/canonical-root.pw.toml
else
  printf "name = 'Root'\n[update.curseforge]\nproject-id = '%s'\nfile-id = '%s'\n" "$id" "$pin" > mods/canonical-root.pw.toml
fi
printf "name = 'Required library'\n[update.modrinth]\nmod-id = 'required'\nversion = 'r1'\n" > mods/required.pw.toml
"#).unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        command(&project)
            .env("EMPACK_PACKWIZ_BIN", &tool)
            .args(["add", "12345", "--platform", platform, pin_flag, first])
            .assert()
            .success();
        let manifest = project.dir().join("empack.yml");
        let original = std::fs::read_to_string(&manifest).unwrap();
        assert!(
            original.contains(first),
            "explicit pin must survive: {original}"
        );
        for _ in 0..2 {
            command(&project)
                .env("EMPACK_PACKWIZ_BIN", &tool)
                .arg("sync")
                .assert()
                .success();
            assert!(project.dir().join("pack/mods/required.pw.toml").exists());
        }
        assert_eq!(
            std::fs::read_to_string(project.dir().join("backend-calls")).unwrap(),
            "add\n"
        );
        std::fs::write(&manifest, original.replace(first, second)).unwrap();
        for _ in 0..2 {
            command(&project)
                .env("EMPACK_PACKWIZ_BIN", &tool)
                .arg("sync")
                .assert()
                .success();
        }
        assert_eq!(
            std::fs::read_to_string(project.dir().join("backend-calls")).unwrap(),
            "add\nadd\n"
        );
        assert!(
            std::fs::read_to_string(project.dir().join("pack/mods/canonical-root.pw.toml"))
                .unwrap()
                .contains(second)
        );
        assert!(project.dir().join("pack/mods/required.pw.toml").exists());
    }
}

fn record_local_removal_fixture(project: &TestProject, path: &str) {
    use empack_lib::application::session::{FileSystemProvider, LiveFileSystemProvider};
    use empack_lib::empack::config::{DependencyEntry, DependencyStatus, LocalDependencyRecord};
    LiveFileSystemProvider
        .config_manager(project.dir().to_path_buf())
        .add_dependency_entry(
            "local",
            DependencyEntry::Local(LocalDependencyRecord {
                status: DependencyStatus::Local,
                title: "Local".into(),
                project_type: empack_lib::primitives::ProjectType::Mod,
                path: path.into(),
                source_url: None,
                sha256: "unused-for-removal".into(),
            }),
        )
        .unwrap();
}

#[test]
fn smoke_local_removal_cannot_delete_a_directory() {
    for path in ["pack", "pack/mods"] {
        let project = TestProject::workflow_fixture("local-remove", "fabric", "1.21.1");
        std::fs::create_dir_all(project.dir().join("pack/mods")).unwrap();
        let sentinel = project.dir().join("pack/mods/keep.jar");
        std::fs::write(&sentinel, b"keep").unwrap();
        record_local_removal_fixture(&project, path);
        let before = snapshot(&project);
        command(&project)
            .args(["remove", "local"])
            .assert()
            .failure();
        assert_eq!(std::fs::read(&sentinel).unwrap(), b"keep");
        assert_eq!(snapshot(&project), before);
    }
}

#[cfg(unix)]
#[test]
fn smoke_local_removal_cannot_follow_a_symlinked_ancestor() {
    let project = TestProject::workflow_fixture("local-remove-link", "fabric", "1.21.1");
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("sentinel.jar"), b"keep outside").unwrap();
    std::os::unix::fs::symlink(outside.path(), project.dir().join("pack/linked")).unwrap();
    record_local_removal_fixture(&project, "pack/linked/sentinel.jar");
    let before = snapshot(&project);
    command(&project)
        .args(["remove", "local"])
        .assert()
        .failure();
    assert_eq!(
        std::fs::read(outside.path().join("sentinel.jar")).unwrap(),
        b"keep outside"
    );
    assert_eq!(snapshot(&project), before);
}

#[cfg(unix)]
#[test]
fn smoke_remove_resolves_alias_title_and_stem_without_wrong_target_deletion() {
    use empack_lib::application::session::{FileSystemProvider, LiveFileSystemProvider};
    use empack_lib::empack::config::{DependencyEntry, DependencyRecord, DependencyStatus};
    use empack_lib::primitives::{ProjectPlatform, ProjectType};
    use std::os::unix::fs::PermissionsExt;
    for query in ["renderer-alias", "Renderer title", "actual-renderer"] {
        let project = TestProject::workflow_fixture("removal", "fabric", "1.21.1");
        let manager = LiveFileSystemProvider.config_manager(project.dir().to_path_buf());
        manager
            .add_dependency_entry(
                "renderer-alias",
                DependencyEntry::Resolved(DependencyRecord {
                    environment: None,
                    status: DependencyStatus::Resolved,
                    title: "Renderer title".into(),
                    platform: ProjectPlatform::Modrinth,
                    project_id: "project-P".into(),
                    project_type: ProjectType::Mod,
                    version: None,
                }),
            )
            .unwrap();
        std::fs::create_dir_all(project.dir().join("pack/mods")).unwrap();
        for (stem, id) in [
            ("actual-renderer", "project-P"),
            ("renderer-alias", "project-Q"),
        ] {
            std::fs::write(
                project.dir().join(format!("pack/mods/{stem}.pw.toml")),
                format!("name = '{stem}'\n[update.modrinth]\nmod-id = '{id}'\nversion = 'v1'\n"),
            )
            .unwrap();
        }
        let tool = project.dir().join("remove-fixture");
        std::fs::write(&tool, "#!/bin/sh\nif [ \"$1\" = remove ]; then\n  rm -- \"mods/$3.pw.toml\"\nelse\n  exit 0\nfi\n").unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        command(&project)
            .env("EMPACK_PACKWIZ_BIN", &tool)
            .args(["remove", query])
            .assert()
            .success();
        assert!(
            !project
                .dir()
                .join("pack/mods/actual-renderer.pw.toml")
                .exists()
        );
        assert!(
            project
                .dir()
                .join("pack/mods/renderer-alias.pw.toml")
                .exists()
        );
        assert!(manager.find_dependency("renderer-alias").unwrap().is_none());
        for _ in 0..2 {
            command(&project)
                .env("EMPACK_PACKWIZ_BIN", &tool)
                .arg("sync")
                .assert()
                .success();
        }
        assert!(
            project
                .dir()
                .join("pack/mods/renderer-alias.pw.toml")
                .exists()
        );
        assert!(
            !project
                .dir()
                .join("pack/mods/actual-renderer.pw.toml")
                .exists()
        );
    }
}

#[cfg(unix)]
#[test]
fn smoke_remove_backend_noop_retains_manifest_intent() {
    use empack_lib::application::session::{FileSystemProvider, LiveFileSystemProvider};
    use empack_lib::empack::config::{DependencyEntry, DependencyRecord, DependencyStatus};
    use empack_lib::primitives::{ProjectPlatform, ProjectType};
    let project = TestProject::workflow_fixture("removal-noop", "fabric", "1.21.1");
    let manager = LiveFileSystemProvider.config_manager(project.dir().to_path_buf());
    manager
        .add_dependency_entry(
            "alias",
            DependencyEntry::Resolved(DependencyRecord {
                environment: None,
                status: DependencyStatus::Resolved,
                title: "Renderer".into(),
                platform: ProjectPlatform::Modrinth,
                project_id: "P".into(),
                project_type: ProjectType::Mod,
                version: None,
            }),
        )
        .unwrap();
    std::fs::create_dir_all(project.dir().join("pack/mods")).unwrap();
    std::fs::write(
        project.dir().join("pack/mods/actual.pw.toml"),
        "name = 'Renderer'\n[update.modrinth]\nmod-id = 'P'\nversion = 'v1'\n",
    )
    .unwrap();
    let tool = project.dir().join("noop-tool");
    std::fs::write(&tool, "#!/bin/sh\nexit 0\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
    let before = snapshot(&project);
    command(&project)
        .env("EMPACK_PACKWIZ_BIN", &tool)
        .args(["remove", "alias"])
        .assert()
        .failure();
    assert_eq!(snapshot(&project), before);
}

fn cache_responses(project: &TestProject, responses: Vec<(String, serde_json::Value)>) {
    let cache = project.dir().join(".empack-cache/http");
    std::fs::create_dir_all(&cache).unwrap();
    let entries: HashMap<_, _> = responses
        .into_iter()
        .map(|(url, data)| {
            (
                url,
                CachedResponse {
                    data: serde_json::to_vec(&data).unwrap(),
                    etag: None,
                    expires: SystemTime::now() + Duration::from_secs(300),
                    status: 200,
                },
            )
        })
        .collect();
    std::fs::write(
        cache.join("http_cache.json"),
        serde_json::to_vec(&entries).unwrap(),
    )
    .unwrap();
}

#[cfg(unix)]
#[test]
fn smoke_add_slug_id_and_url_persist_one_identity_and_type() {
    use empack_lib::application::session::{FileSystemProvider, LiveFileSystemProvider};
    use empack_lib::empack::config::DependencyEntry;
    use std::os::unix::fs::PermissionsExt;
    for (project_type, folder) in [
        ("mod", "mods"),
        ("resourcepack", "resourcepacks"),
        ("shader", "shaderpacks"),
    ] {
        for selector in [
            "pretty".to_string(),
            "CANONICAL".to_string(),
            format!("https://modrinth.com/{project_type}/pretty"),
        ] {
            let project = TestProject::workflow_fixture("canonical-add", "fabric", "1.21.1");
            cache_responses(&project, ["pretty", "CANONICAL"].into_iter().map(|selector| (
                format!("https://api.modrinth.com/v2/project/{selector}"),
                serde_json::json!({"id":"CANONICAL", "title":"Pretty", "project_type":project_type}),
            )).collect());
            let tool = project.dir().join("add-fixture");
            std::fs::write(&tool, format!("#!/bin/sh\n[ \"$2\" = add ] || exit 0\nprintf 'add\\n' >> ../calls\nmkdir -p {folder}\nprintf \"name = 'Pretty'\\n[update.modrinth]\\nmod-id = 'CANONICAL'\\nversion = 'v1'\\n\" > {folder}/pretty.pw.toml\n")).unwrap();
            std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
            command(&project)
                .env("EMPACK_PACKWIZ_BIN", &tool)
                .args(["add", &selector, "--platform", "modrinth"])
                .assert()
                .success();
            let manager = LiveFileSystemProvider.config_manager(project.dir().to_path_buf());
            let (_, entry) = manager.find_dependency("pretty").unwrap().unwrap();
            let DependencyEntry::Resolved(record) = entry else {
                panic!("resolved record required")
            };
            assert_eq!(record.project_id, "CANONICAL");
            assert_eq!(
                empack_lib::application::sync::project_type_arg(record.project_type),
                project_type
            );
            for _ in 0..2 {
                command(&project)
                    .env("EMPACK_PACKWIZ_BIN", &tool)
                    .arg("sync")
                    .assert()
                    .success();
            }
            assert_eq!(
                std::fs::read_to_string(project.dir().join("calls")).unwrap(),
                "add\n"
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn smoke_platform_removal_rejects_symlinked_metadata_ancestors() {
    let project = TestProject::workflow_fixture("confined-metadata", "fabric", "1.21.1");
    let outside = tempfile::tempdir().unwrap();
    let metadata = outside.path().join("target.pw.toml");
    let bytes = "name = 'Target'\n[update.modrinth]\nmod-id = 'P'\nversion = 'v1'\n";
    std::fs::write(&metadata, bytes).unwrap();
    std::os::unix::fs::symlink(outside.path(), project.dir().join("pack/mods")).unwrap();
    let before = snapshot(&project);
    command(&project)
        .args(["remove", "target"])
        .assert()
        .failure();
    assert_eq!(snapshot(&project), before);
    assert_eq!(std::fs::read_to_string(metadata).unwrap(), bytes);
}
