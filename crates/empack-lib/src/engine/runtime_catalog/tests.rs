use super::*;
use crate::{
    application::process_runtime::Cancellation,
    engine::{
        content::verify_stream,
        resources::ResourceGovernor,
        runtime::{OperationOutcome, OperationRuntime},
    },
};
use std::time::Duration;
fn content(json: serde_json::Value) -> AcquiredContent {
    let bytes = serde_json::to_vec(&json).unwrap();
    verify_stream(
        &mut &bytes[..],
        &ExpectedContent {
            digests: None,
            size: None,
            accepted_observation: None,
        },
        1 << 20,
        SourceEvidencePolicy::Compatibility,
        InitialObservation::Accepted,
        &Cancellation::default(),
    )
    .unwrap()
}
fn game(value: &str) -> GameVersion {
    GameVersion::parse(value).unwrap()
}
fn loader(value: &str) -> LoaderVersion {
    LoaderVersion::parse(value).unwrap()
}
#[test]
fn minecraft_discovery_requires_coherent_latest_and_keeps_explicit_historical_choices() {
    let json = serde_json::json!({"latest":{"release":"26.1"},"versions":[
        {"id":"26.2-snapshot-1","type":"snapshot"}, {"id":"26.1","type":"release"}, {"id":"1.7.10","type":"release"}, {"id":"b1.7.3","type":"old_beta"}
    ]});
    let choices = parse_games(content(json.clone()), 4).unwrap();
    assert_eq!(choices.resolve(None).unwrap(), game("26.1"));
    assert_eq!(
        choices.resolve(Some(&game("b1.7.3"))).unwrap(),
        game("b1.7.3")
    );
    assert_eq!(choices.versions()[0], game("26.1"));
    assert!(choices.resolve(Some(&game("not-published"))).is_err());
    assert!(parse_games(content(json.clone()), 3).is_err());
    let mut bad = json.clone();
    bad["latest"]["release"] = "missing".into();
    assert!(parse_games(content(bad), 4).is_err());
    let mut bad = json.clone();
    bad["versions"][1]["id"] = "1.7.10".into();
    assert!(parse_games(content(bad), 4).is_err());
}
#[test]
fn library_catalogs_bind_game_and_preserve_stable_and_prerelease_choices() {
    for kind in [LoaderKind::Fabric, LoaderKind::Quilt] {
        let json = serde_json::json!([
            {"loader":{"version":"0.20.0-beta.9"},"intermediary":{"version":"1.21.1"}},
            {"loader":{"version":"0.20.0-beta.10"},"intermediary":{"version":"1.21.1"}},
            {"loader":{"version":"0.19.5","stable":true},"intermediary":{"version":"1.21.1"}}
        ]);
        let choices = parse_loaders(content(json.clone()), game("1.21.1"), kind, 3).unwrap();
        assert_eq!(
            choices.resolve(None).unwrap().loader_version,
            Some(loader("0.19.5"))
        );
        assert_eq!(choices.versions()[1], loader("0.20.0-beta.10"));
        assert!(choices.resolve(Some(&loader("0.20.0-beta.9"))).is_ok());
        assert!(choices.resolve(Some(&loader("0.20.0-beta.11"))).is_err());
        assert!(parse_loaders(content(json.clone()), game("1.21.2"), kind, 3).is_err());
        assert!(parse_loaders(content(json), game("1.21.1"), kind, 2).is_err());
    }
}
#[test]
fn forge_catalog_retains_legacy_suffixes_and_neoforge_game_boundaries() {
    let forge = parse_loaders(content(serde_json::json!({"1.7.10":["1.7.10-10.13.2.1291","1.7.10-10.13.2.1300-1.7.10","1.7.10-10.13.4.1614-1.7.10"]})), game("1.7.10"), LoaderKind::Forge, 10).unwrap();
    assert_eq!(
        forge.resolve(None).unwrap().loader_version,
        Some(loader("10.13.4.1614"))
    );
    assert_eq!(
        forge
            .resolve(Some(&loader("10.13.2.1300-1.7.10")))
            .unwrap()
            .loader_version,
        Some(loader("10.13.2.1300"))
    );
    let values = serde_json::json!({"versions":["21.1.1","21.1.2-beta","21.10.1","26.1.0.9","26.1.0.10","26.1.01.99","26.1.0.0-alpha.9+snapshot-6","26.1.0.0-alpha.10+snapshot-6","26.1.0.0-alpha.12+snapshot-7"]});
    let neo = parse_loaders(
        content(values.clone()),
        game("1.21.1"),
        LoaderKind::NeoForge,
        20,
    )
    .unwrap();
    assert_eq!(neo.versions(), &[loader("21.1.1"), loader("21.1.2-beta")]);
    let neo = parse_loaders(
        content(values.clone()),
        game("26.1"),
        LoaderKind::NeoForge,
        20,
    )
    .unwrap();
    assert_eq!(neo.versions(), &[loader("26.1.0.10"), loader("26.1.0.9")]);
    let neo = parse_loaders(
        content(values),
        game("26.1-snapshot-6"),
        LoaderKind::NeoForge,
        20,
    )
    .unwrap();
    assert_eq!(
        neo.versions(),
        &[
            loader("26.1.0.0-alpha.10+snapshot-6"),
            loader("26.1.0.0-alpha.9+snapshot-6")
        ]
    );
    let old = parse_loaders(
        content(serde_json::json!({"versions":["1.20.1-47.1.100","1.20.1-47.1.99","1.20-46.0.0"]})),
        game("1.20.1"),
        LoaderKind::NeoForge,
        4,
    )
    .unwrap();
    assert_eq!(
        old.resolve(None).unwrap().loader_version,
        Some(loader("47.1.100"))
    );
}
#[tokio::test]
async fn runtime_discovery_uses_bounded_owned_transport_and_releases_all_resources() {
    let mut server = mockito::Server::new_async().await;
    let response = server
        .mock("GET", "/games")
        .expect(2)
        .with_body(
            r#"{"latest":{"release":"1.21.1"},"versions":[{"id":"1.21.1","type":"release"}]}"#,
        )
        .create_async()
        .await;
    let denied = server
        .mock("GET", "/fabric/1.21.1")
        .with_status(403)
        .create_async()
        .await;
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 1,
        memory_bytes: 4 << 20,
        scratch_bytes: 1 << 20,
        open_files: 16,
    });
    let runtime = OperationRuntime::new(governor.clone(), 1);
    let mut catalog = RuntimeCatalog::new(HttpAcquisition::for_loopback_tests());
    catalog.endpoints.games = format!("{}/games", server.url());
    catalog.endpoints.fabric = format!("{}/fabric/", server.url());
    let mut handle = runtime
        .start(move |mut scope| async move {
            let limits = RuntimeCatalogLimits {
                transfer: TransferLimits {
                    file_bytes: 1024,
                    transfer_bytes: 1024,
                    deadline: Duration::from_secs(3),
                    redirects: 0,
                },
                entries: 4,
            };
            let games = catalog.games(&mut scope, limits).await.unwrap();
            let selected = games.resolve(None).unwrap();
            let denied = catalog
                .loaders(&mut scope, selected, LoaderKind::Fabric, limits)
                .await;
            assert!(
                denied.is_err(),
                "Provider failure must not invent fallback versions"
            );
            let tiny = RuntimeCatalogLimits {
                transfer: TransferLimits {
                    file_bytes: 8,
                    transfer_bytes: 8,
                    ..limits.transfer
                },
                ..limits
            };
            assert!(catalog.games(&mut scope, tiny).await.is_err());
            Ok(())
        })
        .unwrap();
    let result = handle.wait().await;
    assert!(matches!(&*result, OperationOutcome::Completed(())));
    runtime.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    response.assert_async().await;
    denied.assert_async().await;
}

#[test]
fn catalog_selection_initializes_coherent_intent_and_lock_without_manufacturing_a_pin() {
    use crate::engine::{
        documents::DocumentCodec,
        initialize::{InitializeCandidate, default_templates},
    };
    use empack_core::{
        distribution::Recipe,
        model::{
            DistributionArchive, DistributionIntent, PackMetadata, ProjectIntent, RuntimeIntent,
        },
    };
    let choices = parse_loaders(
        content(serde_json::json!([
            {"loader":{"version":"0.19.5","stable":true},"intermediary":{"version":"1.21.1"}}
        ])),
        game("1.21.1"),
        LoaderKind::Fabric,
        4,
    )
    .unwrap();
    let runtime = choices.resolve(None).unwrap();
    let intent = ProjectIntent {
        source_excludes: Vec::new(),
        metadata: PackMetadata {
            name: "Catalog selection".into(),
            version: "1.0.0".into(),
            author: None,
            description: None,
        },
        runtime: RuntimeIntent {
            minecraft: game("1.21.1"),
            acceptable_versions: vec![],
            loader: LoaderKind::Fabric,
            loader_version: None,
        },
        roots: BTreeMap::new(),
        layout: BTreeMap::new(),
        extensions: BTreeMap::new(),
        distribution: DistributionIntent {
            native: None,
            recipes: NonEmpty::new(vec![Recipe::MODRINTH]).unwrap(),
            archive: DistributionArchive::Zip,
        },
    };
    let candidate = InitializeCandidate::new(intent, runtime.clone(), default_templates()).unwrap();
    assert_eq!(candidate.project().lock().runtime, runtime);
    assert!(
        candidate
            .project()
            .intent()
            .runtime
            .loader_version
            .is_none()
    );
    let intent_bytes = DocumentCodec
        .encode_intent(candidate.project().intent())
        .unwrap();
    let lock_bytes = DocumentCodec.encode_lock(candidate.project()).unwrap();
    let decoded = DocumentCodec
        .decode_intent(&intent_bytes, "empack.yml")
        .unwrap();
    assert!(
        DocumentCodec
            .decode_lock(&lock_bytes, &decoded, "empack.lock")
            .is_ok()
    );
}
