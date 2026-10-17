//! The same versioned report is available to CLI and external library consumers.
use clap::Parser;
use empack_lib::{
    application::{Commands, cli::Cli},
    engine::status::{WorkspaceStatus, inspect_workspace},
};
#[test]
fn status_public_report_round_trips_without_creating_control_state() {
    let temp = tempfile::tempdir().unwrap();
    let state = temp.path().join("absent-state");
    let report = inspect_workspace(temp.path(), &state, None, &Default::default()).unwrap();
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["schema"], 1);
    assert_eq!(json["content_verified"], false);
    assert_eq!(json["findings"][0]["code"], "workspace-empty");
    let decoded: WorkspaceStatus = serde_json::from_value(json).unwrap();
    assert_eq!(decoded.root, report.root);
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
    let parsed =
        Cli::try_parse_from(["empack", "status", "--context", "author", "--json"]).unwrap();
    assert!(
        matches!(parsed.command, Some(Commands::Status { context: Some(ref c), json: true }) if c == "author")
    );
}
