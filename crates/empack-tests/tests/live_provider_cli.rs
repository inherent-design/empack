use empack_core::{identity::ProviderProjectId, model::ResolvedIdentity};
use empack_tests::harness::process::{TestProject, read_project};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn snapshot(root: &Path, excluded: &[&str]) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, path: &Path, excluded: &[&str], files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in std::fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let relative = path.strip_prefix(root).unwrap();
            if excluded.iter().any(|name| relative.starts_with(name)) {
                continue;
            }
            if entry.file_type().unwrap().is_dir() {
                visit(root, &entry.path(), excluded, files);
            } else if entry.file_type().unwrap().is_file() {
                files.insert(
                    entry.path().strip_prefix(root).unwrap().into(),
                    std::fs::read(entry.path()).unwrap(),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, root, excluded, &mut files);
    files
}
// Author checks omit host bookkeeping before reading; previews snapshot everything.
fn author_snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    snapshot(root, &[".host-state", ".empack-cache", ".harness-home"])
}
fn execute(project: &TestProject, args: &[&str], credential: Option<&str>) -> std::process::Output {
    let mut command = project.cmd();
    command.args(args);
    if let Some(key) = credential {
        command.env("EMPACK_KEY_CURSEFORGE", key);
    }
    // Never format the command/environment: the CurseForge variant contains a key.
    let mut output = assert_cmd::Command::from_std(command)
        .timeout(std::time::Duration::from_secs(120))
        .output()
        .expect("spawn empack");
    if let Some(key) = credential {
        for stream in [&mut output.stdout, &mut output.stderr] {
            *stream = String::from_utf8_lossy(stream)
                .replace(key, "[credential redacted]")
                .into_bytes();
        }
    }
    output
}
fn checked(project: &TestProject, args: &[&str], credential: Option<&str>) -> std::process::Output {
    let output = execute(project, args, credential);
    assert!(
        output.status.success(),
        "{args:?} failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}
fn run(project: &TestProject, args: &[&str]) -> std::process::Output {
    checked(project, args, None)
}
#[test]
#[ignore = "requires live Modrinth catalog and downloads"]
fn live_cli_equivalent_provider_selectors_converge() {
    selector_journey(
        "modrinth",
        "fabric",
        "sodium",
        "AANobbMI",
        "https://modrinth.com/mod/sodium",
        None,
    );
}
#[test]
#[ignore = "requires explicit CurseForge API credential and live catalog"]
fn live_cli_curseforge_selectors_converge() {
    let key = std::env::var("EMPACK_KEY_CURSEFORGE")
        .expect("CurseForge qualification requires EMPACK_KEY_CURSEFORGE");
    assert!(!key.trim().is_empty(), "CurseForge credential is empty");
    selector_journey(
        "curseforge",
        "neoforge",
        "jei",
        "238222",
        "https://www.curseforge.com/minecraft/mc-mods/jei",
        Some(&key),
    );
}
fn selector_journey(
    platform: &str,
    loader: &str,
    slug: &str,
    id: &str,
    url: &str,
    credential: Option<&str>,
) {
    let project = TestProject::workflow_fixture("provider-add", loader, "1.21.1");
    let run = |args: &[&str]| checked(&project, args, credential);
    run(&["--yes", "add", "--platform", platform, slug]);
    let resolved = read_project(project.dir());
    let canonical = match platform {
        "modrinth" => ProviderProjectId::Modrinth(
            empack_core::identity::ModrinthProjectId::parse(id).unwrap(),
        ),
        "curseforge" => ProviderProjectId::CurseForge(
            empack_core::identity::CurseForgeProjectId::parse(id).unwrap(),
        ),
        _ => unreachable!(),
    };
    let (key, record) = resolved
        .lock()
        .dependencies
        .iter()
        .find(|(_, dep)| dep.identity == ResolvedIdentity::Provider(canonical.clone()))
        .expect("expected canonical provider identity");
    assert!(resolved.intent().roots.contains_key(key));
    assert!(
        record
            .selected
            .as_ref()
            .is_some_and(|pin| pin.project == canonical)
    );
    let intent = std::fs::read(project.dir().join("empack.yml")).unwrap();
    let lock = std::fs::read(project.dir().join("empack.lock")).unwrap();
    for _ in 0..2 {
        run(&["--yes", "sync"]);
        assert_eq!(
            std::fs::read(project.dir().join("empack.yml")).unwrap(),
            intent
        );
        assert_eq!(
            std::fs::read(project.dir().join("empack.lock")).unwrap(),
            lock
        );
    }
    // Re-add through every spelling requires deliberate replacement and retains
    // one logical root for this canonical project. Refusal preserves all inputs.
    for selector in [id, url, slug] {
        let author_before = author_snapshot(project.dir());
        let denied = execute(
            &project,
            &["--yes", "add", "--platform", platform, selector],
            credential,
        );
        assert!(
            !denied.status.success(),
            "same-identity replacement needs --force"
        );
        assert_eq!(author_snapshot(project.dir()), author_before);
        run(&["--yes", "add", "--force", "--platform", platform, selector]);
        let updated = read_project(project.dir());
        assert_eq!(updated.intent().roots.len(), resolved.intent().roots.len());
        assert!(updated.intent().roots.contains_key(key));
        assert_eq!(
            updated.lock().dependencies[key].identity,
            ResolvedIdentity::Provider(canonical.clone())
        );
        assert!(
            updated.lock().dependencies[key]
                .selected
                .as_ref()
                .is_some_and(|pin| pin.project == canonical)
        );
        assert_eq!(
            updated
                .lock()
                .dependencies
                .values()
                .filter(|dep| dep.identity == ResolvedIdentity::Provider(canonical.clone()))
                .count(),
            1
        );
    }
    let before = author_snapshot(project.dir());
    let output = execute(
        &project,
        &[
            "--yes",
            "add",
            "--platform",
            platform,
            "empack-nonexistent-project-xyz12345",
        ],
        credential,
    );
    assert!(!output.status.success());
    assert_eq!(author_snapshot(project.dir()), before);
}

#[test]
#[ignore = "requires live Modrinth catalog and downloads"]
fn live_cli_selected_update_preserves_exact_pin_and_authoring() {
    use empack_core::model::DependencySelection;
    let project = TestProject::workflow_fixture("provider-update", "fabric", "1.21.1");
    run(
        &project,
        &["--yes", "add", "--platform", "modrinth", "sodium"],
    );
    let initial = read_project(project.dir());
    let key = initial.intent().roots.keys().next().unwrap();
    let selected = initial.lock().dependencies[key].selected.clone().unwrap();
    run(&project, &["--yes", "pin", key.as_str()]);
    let pinned = read_project(project.dir());
    assert_eq!(
        pinned.intent().roots[key].selection,
        DependencySelection::ProviderPin(selected.selection.clone())
    );
    let before_preview = snapshot(project.dir(), &[]);
    let before = author_snapshot(project.dir());
    run(
        &project,
        &[
            "--yes",
            "--dry-run",
            "update",
            key.as_str(),
            "--release-policy",
            "any",
        ],
    );
    assert_eq!(
        snapshot(project.dir(), &[]),
        before_preview,
        "preview must not publish author changes"
    );
    let output = run(
        &project,
        &["--yes", "update", key.as_str(), "--release-policy", "any"],
    );
    let rendered = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        rendered.contains("0 selections changed, 1 retained"),
        "{rendered}"
    );
    let updated = read_project(project.dir());
    assert_eq!(updated.intent(), pinned.intent());
    assert_eq!(
        updated.lock().dependencies[key],
        pinned.lock().dependencies[key]
    );
    assert_eq!(
        updated.lock().dependencies[key].selected.as_ref(),
        Some(&selected)
    );
    assert_eq!(
        author_snapshot(project.dir()),
        before,
        "checking an exact pin must preserve author files and content"
    );
    for _ in 0..2 {
        run(&project, &["--yes", "sync"]);
        assert_eq!(author_snapshot(project.dir()), before);
    }
}
