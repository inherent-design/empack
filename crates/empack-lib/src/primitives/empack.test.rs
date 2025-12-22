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
    // Note: We can't easily test the Build transition display in isolation
    // because it requires a BuildOrchestrator instance, which needs providers.
    // The Display implementation is tested through integration tests.
    assert_eq!(StateTransition::Initialize(
        crate::primitives::InitializationConfig {
            name: "Test Pack",
            author: "Test Author",
            version: "1.0.0",
            modloader: "fabric",
            mc_version: "1.20.1",
            loader_version: "0.14.21",
        }
    ).to_string(), "initialize");
    assert_eq!(StateTransition::Synchronize.to_string(), "synchronize");
    assert_eq!(StateTransition::Clean.to_string(), "clean");
}
