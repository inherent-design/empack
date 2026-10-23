use empack_tests::harness::process::{TestProject, offline_command};

#[test]
fn cli_add_uninitialized_leaves_project_absent() {
    let project = TestProject::new();
    let output = offline_command(project.dir())
        .args(["--yes", "add", "--platform", "modrinth", "sodium"])
        .output()
        .expect("spawn empack");
    assert!(!output.status.success());
    assert!(!project.dir().join("empack.yml").exists());
    assert!(!project.dir().join("pack").exists());
}
#[test]
fn cli_headless_search_requires_explicit_selection() {
    let project = TestProject::workflow_fixture("headless-add", "fabric", "1.21.1");
    let before = std::fs::read(project.dir().join("empack.yml")).unwrap();
    let output = offline_command(project.dir())
        .args(["--yes", "add", "xyznonexistentmod12345"])
        .output()
        .expect("spawn empack");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("deliberate choice"));
    assert_eq!(
        std::fs::read(project.dir().join("empack.yml")).unwrap(),
        before
    );
}
