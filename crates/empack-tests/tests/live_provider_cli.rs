use empack_core::{identity::ProviderProjectId, model::ResolvedIdentity};
use empack_tests::harness::process::{TestProject, read_project};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, path: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in std::fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                visit(root, &entry.path(), files);
            } else if entry.file_type().unwrap().is_file() {
                files.insert(
                    entry.path().strip_prefix(root).unwrap().into(),
                    std::fs::read(entry.path()).unwrap(),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}
fn run(project: &TestProject, args: &[&str]) -> std::process::Output {
    let output = project
        .cmd()
        .env("EMPACK_PACKWIZ_BIN", project.dir().join("missing-backend"))
        .args(args)
        .output()
        .expect("spawn empack");
    assert!(
        output.status.success(),
        "{args:?} failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}
#[test]
#[ignore = "requires live Modrinth catalog and downloads"]
fn live_cli_equivalent_provider_selectors_converge() {
    let project = TestProject::workflow_fixture("provider-add", "fabric", "1.21.1");
    run(
        &project,
        &["--yes", "add", "--platform", "modrinth", "sodium"],
    );
    let resolved = read_project(project.dir());
    let (key,record)=resolved.lock().dependencies.iter().find(|(_,dep)| matches!(&dep.identity,ResolvedIdentity::Provider(ProviderProjectId::Modrinth(id)) if id.as_str()=="AANobbMI")).expect("canonical Sodium identity");
    assert!(resolved.intent().roots.contains_key(key));
    assert!(record.selected.is_some());
    let intent = std::fs::read(project.dir().join("empack.yml")).unwrap();
    let lock = std::fs::read(project.dir().join("empack.lock")).unwrap();
    for _ in 0..2 {
        run(&project, &["--yes", "sync"]);
        assert_eq!(
            std::fs::read(project.dir().join("empack.yml")).unwrap(),
            intent
        );
        assert_eq!(
            std::fs::read(project.dir().join("empack.lock")).unwrap(),
            lock
        );
    }
    // Equivalent selectors cannot introduce a second logical root.
    for selector in ["AANobbMI", "https://modrinth.com/mod/sodium"] {
        let denied = project
            .cmd()
            .args(["--yes", "add", "--platform", "modrinth", selector])
            .output()
            .unwrap();
        assert!(
            !denied.status.success(),
            "same-identity replacement needs --force"
        );
        assert_eq!(
            std::fs::read(project.dir().join("empack.yml")).unwrap(),
            intent
        );
        run(
            &project,
            &[
                "--yes",
                "add",
                "--force",
                "--platform",
                "modrinth",
                selector,
            ],
        );
        assert_eq!(
            read_project(project.dir()).intent().roots.len(),
            resolved.intent().roots.len()
        );
    }
    let intent = std::fs::read(project.dir().join("empack.yml")).unwrap();
    let lock = std::fs::read(project.dir().join("empack.lock")).unwrap();
    let before = snapshot(&project.dir().join("pack"));
    let output = project
        .cmd()
        .args([
            "--yes",
            "add",
            "--platform",
            "modrinth",
            "empack-nonexistent-project-xyz12345",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(snapshot(&project.dir().join("pack")), before);
    assert_eq!(
        std::fs::read(project.dir().join("empack.yml")).unwrap(),
        intent
    );
    assert_eq!(
        std::fs::read(project.dir().join("empack.lock")).unwrap(),
        lock
    );
}
