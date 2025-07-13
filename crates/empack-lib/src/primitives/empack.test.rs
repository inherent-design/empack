use super::*;

#[test]
fn test_build_target_display() {
    assert_eq!(BuildTarget::Mrpack.to_string(), "mrpack");
    assert_eq!(BuildTarget::ClientFull.to_string(), "client-full");
}

#[test]
fn test_build_target_parse() {
    assert_eq!(
        "mrpack".parse::<BuildTarget>().unwrap(),
        BuildTarget::Mrpack
    );
    assert_eq!(
        "client".parse::<BuildTarget>().unwrap(),
        BuildTarget::Client
    );
    assert_eq!(
        "server".parse::<BuildTarget>().unwrap(),
        BuildTarget::Server
    );
}

#[test]
fn test_execution_order() {
    let mut targets = vec![
        BuildTarget::ServerFull,
        BuildTarget::Mrpack,
        BuildTarget::Client,
    ];
    BuildTarget::sort_by_execution_order(&mut targets);
    assert_eq!(
        targets,
        vec![
            BuildTarget::Mrpack,
            BuildTarget::Client,
            BuildTarget::ServerFull,
        ]
    );
}

#[test]
fn test_expand_all() {
    let all_targets = BuildTarget::expand_all();
    assert_eq!(
        all_targets,
        vec![
            BuildTarget::Mrpack,
            BuildTarget::Client,
            BuildTarget::Server,
        ]
    );
}

#[test]
fn test_state_display() {
    assert_eq!(ModpackState::Uninitialized.to_string(), "uninitialized");
    assert_eq!(ModpackState::Configured.to_string(), "configured");
}

#[test]
fn test_transition_display() {
    let build_transition =
        StateTransition::Build(vec![BuildTarget::Mrpack, BuildTarget::Client]);
    assert_eq!(build_transition.to_string(), "build [mrpack, client]");
}
