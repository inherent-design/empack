//! Official provider page selection with a fixture desktop opener, not a browser SPA.
#![cfg(unix)]
#[path = "support/desktop_assistance.rs"]
mod desktop_assistance;
use desktop_assistance::{browser_command, browser_fixture};
use std::{fs, time::Duration};

/// Requires the official Modrinth metadata API; the desktop opener and downloaded bytes are fixtures.
#[cfg(unix)]
#[test]
#[ignore = "requires live Modrinth metadata; desktop opener and bytes are fixtures"]
fn live_cli_manual_assistance_opens_exact_page_and_verifies_supplied_bytes() {
    let (project, host) = browser_fixture();
    let mut cmd = browser_command(&project, host.path());
    cmd.args([
        "--yes",
        "build",
        "modrinth",
        "--open-downloads",
        "--wait-downloads",
        "10",
        "--downloads-dir",
    ])
    .arg(host.path().join("downloads"));
    assert_cmd::Command::from_std(cmd)
        .timeout(Duration::from_secs(90))
        .assert()
        .success();
    let opened = fs::read_to_string(host.path().join("opened")).unwrap();
    assert_eq!(
        opened.trim(),
        "https://modrinth.com/resourcepack/faithful-32x/version/lDYpMiqk"
    );
    empack_tests::harness::process::assert_dist_artifact_suffix(project.dir(), ".mrpack");
    assert!(
        !fs::read_dir(host.path().join("state/pending-builds"))
            .unwrap()
            .flatten()
            .any(|entry| entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "json"))
    );
}
