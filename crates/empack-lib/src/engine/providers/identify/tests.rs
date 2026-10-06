use super::*;
use crate::engine::{
    content::{InitialObservation, SourceEvidencePolicy, verify_stream},
    resources::ResourceGovernor,
    runtime::{OperationOutcome, OperationRuntime},
};
use mockito::{Matcher, Server};
use serde_json::json;
use std::io::Cursor;
fn content() -> AcquiredContent {
    let bytes = b"payload\n \t";
    verify_stream(
        &mut &bytes[..],
        &ExpectedContent {
            digests: None,
            size: None,
            accepted_observation: None,
        },
        bytes.len() as u64,
        SourceEvidencePolicy::Compatibility,
        InitialObservation::Accepted,
        &Cancellation::default(),
    )
    .unwrap()
}
fn digest(algorithm: DigestAlgorithm) -> String {
    content()
        .observed_digests()
        .values()
        .iter()
        .find(|v| v.algorithm() == algorithm)
        .unwrap()
        .hex()
}
fn version() -> Value {
    json!({"id":"version1","project_id":"project1","files":[{"filename":"main.jar","primary":false,"size":10,"hashes":{"sha1":digest(DigestAlgorithm::Sha1),"sha512":digest(DigestAlgorithm::Sha512)},"url":"https://example.com/main.jar"}],"game_versions":["1.20.1"],"loaders":["fabric"],"dependencies":[]})
}
fn cf_file(project: u64, file: u64) -> Value {
    json!({"id":file,"modId":project,"gameId":432,"fileFingerprint":murmur2::murmur2(b"payload",1),"fileName":"main.jar","fileLength":10,"hashes":[{"algo":2,"value":digest(DigestAlgorithm::Md5)}],"downloadUrl":null,"gameVersions":["1.20.1","Fabric"],"dependencies":[]})
}
fn cf_response(files: Vec<Value>) -> Value {
    json!({"data":{"isCacheBuilt":true,"exactMatches":files.into_iter().map(|file| json!({"id":file["modId"],"file":file})).collect::<Vec<_>>()}})
}
fn limits() -> IdentificationLimits {
    IdentificationLimits {
        catalog: CatalogLimits {
            response_bytes: 4096,
            transfer_bytes: 16384,
            deadline: Duration::from_secs(3),
        },
        file_bytes: 1024,
        matches: 8,
    }
}
type Outcome = Arc<OperationOutcome<Result<Identification>>>;
async fn identify(
    server: &Server,
    providers: Vec<ProviderKind>,
    key: Option<String>,
    limits: IdentificationLimits,
) -> (Outcome, ResourceGovernor) {
    let catalog = ProviderCatalog::for_loopback_tests(&server.url(), key);
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 1,
        memory_bytes: 4 << 20,
        open_files: 4,
        ..Default::default()
    });
    let runtime = OperationRuntime::new(governor.clone(), 1);
    let mut handle = runtime
        .start(move |mut scope| async move {
            Ok(catalog
                .identify_file(
                    &mut scope,
                    content(),
                    NonEmpty::new(providers).unwrap(),
                    limits,
                )
                .await)
        })
        .unwrap();
    let outcome = handle.wait().await;
    runtime.shutdown().await;
    (outcome, governor)
}
#[test]
fn streaming_fingerprint_matches_reference_across_word_and_buffer_boundaries() {
    for length in (0..65).chain([32767, 32768, 32769, 131075]) {
        let bytes: Vec<_> = (0..length).map(|index| (index % 251) as u8).collect();
        let expected: Vec<_> = bytes
            .iter()
            .copied()
            .filter(|v| !matches!(v, 9 | 10 | 13 | 32))
            .collect();
        assert_eq!(
            fingerprint(
                &mut Cursor::new(bytes),
                Instant::now() + Duration::from_secs(5),
                &Cancellation::default()
            )
            .unwrap(),
            murmur2::murmur2(&expected, 1)
        );
    }
    let cancel = Cancellation::default();
    cancel.cancel();
    assert!(
        fingerprint(
            &mut Cursor::new(b"data"),
            Instant::now() + Duration::from_secs(1),
            &cancel
        )
        .is_err()
    );
    assert!(
        fingerprint(
            &mut Cursor::new(b"data"),
            Instant::now(),
            &Cancellation::default()
        )
        .is_err()
    );
}
#[tokio::test]
async fn modrinth_identity_uses_verified_bytes_not_names_or_primary_flags() {
    let mut server = Server::new_async().await;
    let mut v = version();
    let mut second = v["files"][0].clone();
    second["filename"] = json!("copy.jar");
    v["files"].as_array_mut().unwrap().push(second);
    let hash = server
        .mock(
            "GET",
            format!("/version_file/{}", digest(DigestAlgorithm::Sha512)).as_str(),
        )
        .match_query(Matcher::UrlEncoded("algorithm".into(), "sha512".into()))
        .with_body(v.to_string())
        .expect(1)
        .create_async()
        .await;
    server
        .mock("GET", "/project/project1")
        .with_body(
            json!({"id":"project1","slug":"project","title":"Project","project_type":"mod"})
                .to_string(),
        )
        .create_async()
        .await;
    let (outcome, governor) = identify(
        &server,
        vec![ProviderKind::Modrinth, ProviderKind::Modrinth],
        None,
        limits(),
    )
    .await;
    let OperationOutcome::Completed(Ok(Identification::Exact(found))) = &*outcome else {
        panic!("expected exact match")
    };
    assert_eq!(found.content, content().lease().id());
    assert_eq!(found.matching_files.as_slice(), &["main.jar", "copy.jar"]);
    assert_eq!(found.resolution.pin.project.to_string(), "project1");
    hash.assert_async().await;
    assert!(governor.status().reserved.memory_bytes > 0);
    drop(outcome);
    assert_eq!(governor.status().reserved.memory_bytes, 0);
}
#[tokio::test]
async fn unknown_is_distinct_from_auth_failures_and_inconsistent_hash_results() {
    for (status, expected_unknown) in [(404, true), (401, false), (403, false), (302, false)] {
        let mut server = Server::new_async().await;
        server
            .mock("GET", Matcher::Any)
            .with_status(status)
            .create_async()
            .await;
        let (outcome, _) = identify(&server, vec![ProviderKind::Modrinth], None, limits()).await;
        assert_eq!(
            matches!(
                &*outcome,
                OperationOutcome::Completed(Ok(Identification::Unknown))
            ),
            expected_unknown
        );
        if !expected_unknown {
            assert!(matches!(&*outcome, OperationOutcome::Completed(Err(_))));
        }
    }
    let mut server = Server::new_async().await;
    let mut bad = version();
    bad["files"][0]["hashes"]["sha512"] = json!("00".repeat(64));
    server
        .mock("GET", Matcher::Regex("/version_file/.*".into()))
        .with_body(bad.to_string())
        .create_async()
        .await;
    server
        .mock("GET", "/project/project1")
        .with_body(
            json!({"id":"project1","slug":"project","title":"Project","project_type":"mod"})
                .to_string(),
        )
        .create_async()
        .await;
    assert!(matches!(
        &*identify(&server, vec![ProviderKind::Modrinth], None, limits())
            .await
            .0,
        OperationOutcome::Completed(Err(_))
    ));
}
#[tokio::test]
async fn curseforge_fingerprints_only_nominate_original_digest_checked_candidates() {
    let mut server = Server::new_async().await;
    let mut collision = cf_file(123, 1);
    collision["hashes"][0]["value"] = json!("00".repeat(16));
    let lookup = server
        .mock("POST", "/fingerprints/432")
        .match_header("x-api-key", "fixture-key")
        .match_body(Matcher::Json(
            json!({"fingerprints":[murmur2::murmur2(b"payload",1)]}),
        ))
        .with_body(cf_response(vec![collision, cf_file(123, 2)]).to_string())
        .create_async()
        .await;
    server
        .mock("GET", "/mods/123")
        .with_body(
            json!({"data":{"id":123,"gameId":432,"classId":6,"slug":"project","name":"Project"}})
                .to_string(),
        )
        .create_async()
        .await;
    let (outcome, _) = identify(
        &server,
        vec![ProviderKind::CurseForge],
        Some("fixture-key".into()),
        limits(),
    )
    .await;
    let OperationOutcome::Completed(Ok(Identification::Exact(found))) = &*outcome else {
        panic!("expected verified fingerprint candidate")
    };
    assert_eq!(
        found.resolution.pin.selection,
        found.resolution.pin.project.parse_pin("2").unwrap()
    );
    assert_eq!(
        found.resolution.files.as_slice()[0]
            .expected
            .digests
            .as_ref()
            .unwrap()
            .values()[0]
            .algorithm(),
        DigestAlgorithm::Md5
    );
    lookup.assert_async().await;
}
#[tokio::test]
async fn duplicate_provider_matches_remain_ambiguous_and_bounded() {
    let mut server = Server::new_async().await;
    server
        .mock("POST", "/fingerprints/432")
        .with_body(cf_response(vec![cf_file(123, 1), cf_file(456, 2)]).to_string())
        .create_async()
        .await;
    for id in [123, 456] {
        server.mock("GET",format!("/mods/{id}").as_str()).with_body(json!({"data":{"id":id,"gameId":432,"classId":6,"slug":"project","name":"Project"}}).to_string()).create_async().await;
    }
    let (outcome, _) = identify(
        &server,
        vec![ProviderKind::CurseForge],
        Some("fixture-key".into()),
        limits(),
    )
    .await;
    let OperationOutcome::Completed(Ok(Identification::Ambiguous(found))) = &*outcome else {
        panic!("expected ambiguity")
    };
    assert_eq!(found.as_slice().len(), 2);
    let mut small = limits();
    small.matches = 1;
    assert!(matches!(
        &*identify(
            &server,
            vec![ProviderKind::CurseForge],
            Some("fixture-key".into()),
            small
        )
        .await
        .0,
        OperationOutcome::Completed(Err(_))
    ));
    assert!(matches!(
        &*identify(&server, vec![ProviderKind::CurseForge], None, limits())
            .await
            .0,
        OperationOutcome::Completed(Err(_))
    ));
}
#[test]
fn incomplete_or_foreign_fingerprint_records_cannot_become_unknown() {
    let original = cf_response(vec![cf_file(123, 1)]);
    for (pointer, value) in [
        ("/data/isCacheBuilt", json!(false)),
        ("/data/exactMatches/0/file/modId", json!(999)),
        ("/data/exactMatches/0/file/fileFingerprint", json!(0)),
    ] {
        let mut bad = original.clone();
        *bad.pointer_mut(pointer).unwrap() = value;
        assert!(
            parse_lookup(
                ProviderKind::CurseForge,
                &serde_json::to_vec(&bad).unwrap(),
                Some(murmur2::murmur2(b"payload", 1)),
                1
            )
            .is_err()
        );
    }
}

#[tokio::test]
async fn later_provider_failure_and_cumulative_limits_return_no_successful_subset() {
    let mut server = Server::new_async().await;
    let v = version();
    let project = json!({"id":"project1","slug":"project","title":"Project","project_type":"mod"});
    server
        .mock("GET", Matcher::Regex("/version_file/.*".into()))
        .with_body(v.to_string())
        .create_async()
        .await;
    server
        .mock("GET", "/project/project1")
        .with_body(project.to_string())
        .create_async()
        .await;
    server
        .mock("POST", "/fingerprints/432")
        .with_status(401)
        .create_async()
        .await;
    let (result, governor) = identify(
        &server,
        vec![ProviderKind::Modrinth, ProviderKind::CurseForge],
        Some("fixture-key".into()),
        limits(),
    )
    .await;
    assert!(matches!(&*result, OperationOutcome::Completed(Err(_))));
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    let mut limited = limits();
    limited.catalog.transfer_bytes = (v.to_string().len() + project.to_string().len() - 1) as u64;
    assert!(matches!(
        &*identify(&server, vec![ProviderKind::Modrinth], None, limited)
            .await
            .0,
        OperationOutcome::Completed(Err(_))
    ));
}
