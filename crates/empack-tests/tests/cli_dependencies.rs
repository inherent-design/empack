use empack_tests::harness::process::{TestProject, offline_command};
#[path = "support/files.rs"]
mod files;

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
    use std::{fs, io::ErrorKind, net::TcpListener};
    let project = TestProject::workflow_fixture("headless-add", "fabric", "1.21.1");
    fs::write(project.dir().join("prior-build.zip"), b"previous artifact").unwrap();
    fs::create_dir(project.dir().join("author notes")).unwrap();
    fs::write(
        project.dir().join("author notes/decision.md"),
        b"keep this choice",
    )
    .unwrap();
    let before = files::snapshot(project.dir());
    let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
    proxy.set_nonblocking(true).unwrap();
    let address = format!("http://{}", proxy.local_addr().unwrap());
    for args in [
        vec!["--yes", "add", "xyznonexistentmod12345"],
        vec!["add", "xyznonexistentmod12345"],
        vec!["--yes", "add", "--platform", "modrinth", "rendering engine"],
        vec![
            "--dry-run",
            "add",
            "--platform",
            "modrinth",
            "rendering engine",
        ],
    ] {
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
        let output = command
            .env("NO_PROXY", "")
            .env("no_proxy", "")
            .args(&args)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{args:?}");
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        assert!(
            diagnostic.contains("deliberate choice"),
            "{args:?}: {diagnostic}"
        );
        assert!(
            diagnostic.contains("project URL or --platform with a slug/ID"),
            "{diagnostic}"
        );
        assert_eq!(files::snapshot(project.dir()), before, "{args:?}");
        match proxy.accept() {
            Err(error) if error.kind() == ErrorKind::WouldBlock => {}
            other => panic!("headless search attempted transport: {args:?}: {other:?}"),
        }
    }
}
