use crate::primitives::{BuildTarget, MarkerKind, PackState, StateTransition, TransitionKind};

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
    assert_eq!(PackState::Uninitialized.to_string(), "uninitialized");
    assert_eq!(PackState::Configured.to_string(), "configured");
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
    assert_eq!(StateTransition::RefreshIndex.to_string(), "refresh-index");
    assert_eq!(StateTransition::Clean.to_string(), "clean");
}

#[test]
fn test_transition_kind_display() {
    assert_eq!(TransitionKind::Initialize.to_string(), "initialize");
    assert_eq!(TransitionKind::RefreshIndex.to_string(), "refresh-index");
    assert_eq!(TransitionKind::Build.to_string(), "build");
    assert_eq!(TransitionKind::Clean.to_string(), "clean");
}

#[test]
fn test_marker_kind_display() {
    assert_eq!(MarkerKind::Building.to_string(), "building");
    assert_eq!(MarkerKind::Cleaning.to_string(), "cleaning");
}

#[test]
fn test_state_transition_kind_method() {
    assert_eq!(
        StateTransition::Initialize(crate::primitives::InitializationConfig {
            name: "Test",
            author: "Test",
            version: "1.0.0",
            modloader: "fabric",
            mc_version: "1.20.1",
            loader_version: "0.14.21",
        }).kind(),
        TransitionKind::Initialize,
    );
    assert_eq!(StateTransition::RefreshIndex.kind(), TransitionKind::RefreshIndex);
    assert_eq!(StateTransition::Clean.kind(), TransitionKind::Clean);

    // Build variant -- use a mock session to construct BuildOrchestrator
    let mock_session = crate::application::session_mocks::MockCommandSession::new();
    let orchestrator = crate::empack::builds::BuildOrchestrator::new(&mock_session, crate::empack::archive::ArchiveFormat::Zip).unwrap();
    assert_eq!(
        StateTransition::Build(orchestrator, vec![]).kind(),
        TransitionKind::Build,
    );
}
