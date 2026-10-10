use super::*;
use crate::engine::{
    documents::DocumentCodec,
    mrpack::tests::project,
    project::ProjectReader,
    publication::{Publisher, RecoveryReader},
    resources::{ResourceGovernor, ResourceRequest},
    runtime::{OperationOutcome, OperationRuntime},
    snapshot::SnapshotLimits,
};
use empack_core::{digest::DigestSet, path::PathSyntax};
use std::fs;

fn locked_key(name: &str) -> LockedFileKey {
    LockedFileKey {
        dependency: empack_core::model::DependencyKey::parse(name).unwrap(),
        slot: empack_core::model::FileSlot::parse("main").unwrap(),
    }
}
fn path(value: &str) -> PortableRelPath {
    PortableRelPath::parse(value, PathSyntax::ProjectContent).unwrap()
}
fn write_project(root: &std::path::Path, project: &ResolvedProject) {
    fs::write(
        root.join("empack.yml"),
        DocumentCodec.encode_intent(project.intent()).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("empack.lock"),
        DocumentCodec.encode_lock(project).unwrap(),
    )
    .unwrap();
}
fn capture(root: &std::path::Path, host: &std::path::Path) -> WorkspaceSnapshot {
    ProjectReader::new(RecoveryReader::new(host.join("private")))
        .capture_build(
            root,
            &[path("result.mrpack")],
            SnapshotLimits::default(),
            &Cancellation::default(),
        )
        .unwrap()
}

#[test]
fn acquisition_selection_matches_materialized_missing_content_and_optional_choices() {
    use empack_core::{distribution::Recipe, inventory::OptionalPolicy};
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let cancel = Cancellation::default();
    write_project(root.path(), &project(true, true));
    fs::create_dir_all(root.path().join("pack/mods")).unwrap();
    fs::write(root.path().join("pack/mods/extra.pw.toml"), b"filename='extra.jar'\nside='client'\n[download]\nurl='https://example.com/extra.jar'\nhash-format='md5'\nhash='321c3cf486ed509164edec1e1981fec8'\n[option]\noptional=true\ndefault=false\n").unwrap();
    let workspace = capture(root.path(), host.path());
    for enabled in [false, true] {
        let optional = OptionalPolicy::Resolve {
            choices: BTreeMap::from([("extra".into(), enabled)]),
            use_defaults: false,
        };
        for target in [Recipe::PRISM_BUNDLED, Recipe::SERVER_BUNDLED] {
            let plan = plan_target_build_acquisitions(
                &workspace,
                &BuildAcquisitions::default(),
                target,
                &optional,
                SourceEvidencePolicy::Compatibility,
                &cancel,
            )
            .unwrap();
            let actual = super::super::materialized::prepare_game_content(
                &workspace,
                &BuildAcquisitions::default(),
                target,
                &optional,
                SourceEvidencePolicy::Compatibility,
                &cancel,
            );
            if enabled && target == Recipe::PRISM_BUNDLED {
                let error = actual.err().unwrap();
                let missing = error
                    .downcast_ref::<super::super::materialized::MissingGameContent>()
                    .unwrap();
                let expected: BTreeSet<_> = missing
                    .files
                    .iter()
                    .cloned()
                    .map(AcquisitionKey::Locked)
                    .collect();
                assert_eq!(
                    plan.needs()
                        .iter()
                        .map(|need| need.key.clone())
                        .collect::<BTreeSet<_>>(),
                    expected
                );
                assert_eq!(plan.needs().len(), 2);
            } else {
                assert!(plan.needs().is_empty());
                assert!(actual.is_ok());
            }
        }
    }
    assert!(!host.path().join("private").exists());
    assert!(!root.path().join("dist").exists());
}

#[test]
fn native_references_acquire_addresses_and_retain_source_evidence() {
    use empack_core::{distribution::Recipe, inventory::OptionalPolicy};
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let cancel = Cancellation::default();
    write_project(root.path(), &project(true, false));
    fs::create_dir_all(root.path().join("pack/mods")).unwrap();
    fs::write(root.path().join("pack/mods/extra.pw.toml"), b"filename='extra.jar'\nside='client'\n[download]\nurl='https://example.com/extra.jar'\nhash-format='md5'\nhash='321c3cf486ed509164edec1e1981fec8'\n").unwrap();
    let workspace = capture(root.path(), host.path());
    let plan = |target, evidence| {
        plan_target_build_acquisitions(
            &workspace,
            &BuildAcquisitions::default(),
            target,
            &OptionalPolicy::Preserve,
            evidence,
            &cancel,
        )
    };
    assert_eq!(
        plan(
            Recipe::PRISM_REFERENCES,
            SourceEvidencePolicy::Compatibility
        )
        .unwrap()
        .needs()
        .len(),
        2
    );
    assert!(
        plan(
            Recipe::PRISM_REFERENCES,
            SourceEvidencePolicy::StrongSourceRequired
        )
        .is_err()
    );
    assert!(
        plan(
            Recipe::SERVER_BUNDLED,
            SourceEvidencePolicy::StrongSourceRequired
        )
        .unwrap()
        .needs()
        .is_empty()
    );
    assert!(
        super::super::materialized::prepare_game_content(
            &workspace,
            &BuildAcquisitions::default(),
            Recipe::SERVER_BUNDLED,
            &OptionalPolicy::Preserve,
            SourceEvidencePolicy::StrongSourceRequired,
            &cancel,
        )
        .is_ok()
    );
    // Both native releases and mrpack establish their required content hashes.
    assert_eq!(
        plan(Recipe::MODRINTH, SourceEvidencePolicy::Compatibility)
            .unwrap()
            .needs()
            .len(),
        2
    );
}

#[test]
fn full_target_acquires_only_surviving_overlay_owners() {
    use empack_core::{distribution::Recipe, inventory::OptionalPolicy};
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    write_project(root.path(), &project(false, false));
    fs::create_dir_all(root.path().join("overrides/client/resourcepacks")).unwrap();
    for file in ["a.zip", "copy.zip"] {
        fs::write(
            root.path()
                .join("overrides/client/resourcepacks")
                .join(file),
            b"replacement",
        )
        .unwrap();
    }
    let workspace = capture(root.path(), host.path());
    let plan = plan_target_build_acquisitions(
        &workspace,
        &BuildAcquisitions::default(),
        Recipe::PRISM_BUNDLED,
        &OptionalPolicy::Preserve,
        SourceEvidencePolicy::Compatibility,
        &Cancellation::default(),
    )
    .unwrap();
    assert_eq!(plan.needs().len(), 1);
    assert!(
        matches!(&plan.needs()[0].key, AcquisitionKey::Locked(key) if key.slot.as_str() == "second")
    );
}

#[test]
fn excluded_local_records_do_not_require_missing_or_modified_bytes() {
    use empack_core::{
        distribution::Recipe,
        inventory::OptionalPolicy,
        model::{ResolvedIdentity, SourceIntent},
    };
    let base = project(true, true);
    let mut intent = base.intent().clone();
    intent.roots.values_mut().next().unwrap().source = SourceIntent::Local(path("pack/local.zip"));
    let revision = DocumentCodec
        .decode_intent(&DocumentCodec.encode_intent(&intent).unwrap(), "local")
        .unwrap()
        .semantic_revision();
    let mut lock = base.lock().clone();
    lock.intent_revision = revision;
    let (key, dependency) = lock.dependencies.iter_mut().next().unwrap();
    dependency.identity = ResolvedIdentity::Local(key.clone());
    let mut files = dependency.files.as_slice().to_vec();
    for file in &mut files {
        file.acquisition = AcquisitionSpec::Local(path("pack/local.zip"));
    }
    dependency.files = NonEmpty::new(files).unwrap();
    let project = ResolvedProject::validate(intent, lock, revision).unwrap();
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    write_project(root.path(), &project);
    fs::create_dir(root.path().join("pack")).unwrap();
    for present in [false, true] {
        if present {
            fs::write(root.path().join("pack/local.zip"), b"wrong").unwrap();
        }
        let workspace = capture(root.path(), host.path());
        for enabled in [false, true] {
            let optional = OptionalPolicy::Resolve {
                choices: BTreeMap::from([("extra".into(), enabled)]),
                use_defaults: false,
            };
            let actual = super::super::materialized::prepare_game_content(
                &workspace,
                &BuildAcquisitions::default(),
                Recipe::PRISM_BUNDLED,
                &optional,
                SourceEvidencePolicy::Compatibility,
                &Cancellation::default(),
            );
            assert_eq!(
                actual.is_err(),
                enabled,
                "selected local content must still verify"
            );
            if !enabled {
                assert!(
                    plan_target_build_acquisitions(
                        &workspace,
                        &BuildAcquisitions::default(),
                        Recipe::PRISM_BUNDLED,
                        &optional,
                        SourceEvidencePolicy::Compatibility,
                        &Cancellation::default()
                    )
                    .unwrap()
                    .needs()
                    .is_empty()
                );
            }
        }
    }
}
#[test]
fn reference_evidence_and_layer_precedence_determine_missing_bytes_without_effects() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let cancel = Cancellation::default();
    write_project(root.path(), &project(false, false));
    let plan = |mode| {
        plan_build_acquisitions(
            &capture(root.path(), host.path()),
            &BuildAcquisitions::default(),
            mode,
            &cancel,
        )
        .unwrap()
    };
    assert!(
        plan(BuildMaterialization::ReferenceArchive)
            .needs()
            .is_empty()
    );
    assert_eq!(plan(BuildMaterialization::AllContent).needs().len(), 2);
    fs::create_dir_all(root.path().join("overrides/client/resourcepacks")).unwrap();
    fs::write(
        root.path().join("overrides/client/resourcepacks/a.zip"),
        b"replacement",
    )
    .unwrap();
    let layered = plan(BuildMaterialization::ReferenceArchive);
    assert_eq!(layered.needs().len(), 1);
    assert_eq!(
        layered.needs()[0].reason,
        AcquisitionReason::LayeredReplacement
    );
    write_project(root.path(), &project(true, false));
    assert_eq!(
        plan(BuildMaterialization::ReferenceArchive).needs().len(),
        2
    );
    // Captured materialization supplies evidence for the first slot's two placements.
    fs::create_dir_all(root.path().join("pack/resourcepacks")).unwrap();
    fs::write(root.path().join("pack/resourcepacks/copy.zip"), b"payload").unwrap();
    let remaining = plan(BuildMaterialization::ReferenceArchive);
    assert_eq!(remaining.needs().len(), 1);
    assert!(
        matches!(&remaining.needs()[0].key, AcquisitionKey::Locked(key) if key.slot.as_str()=="second")
    );
    assert!(!host.path().join("private").exists());
    assert!(!root.path().join("dist").exists());
}
#[tokio::test]
async fn download_batch_verifies_every_requested_file_before_artifact_publication() {
    for fail_second in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let host = tempfile::tempdir().unwrap();
        write_project(root.path(), &project(true, false));
        fs::create_dir(root.path().join("dist")).unwrap();
        fs::write(root.path().join("dist/result.mrpack"), b"previous artifact").unwrap();
        let original_lock = fs::read(root.path().join("empack.lock")).unwrap();
        let workspace = capture(root.path(), host.path());
        let mut plan = plan_build_acquisitions(
            &workspace,
            &BuildAcquisitions::default(),
            BuildMaterialization::ReferenceArchive,
            &Cancellation::default(),
        )
        .unwrap();
        assert_eq!(plan.needs().len(), 2);
        let mut server = mockito::Server::new_async().await;
        let first = server
            .mock("GET", "/first")
            .with_body("payload")
            .create_async()
            .await;
        let second = server
            .mock("GET", "/second")
            .with_status(if fail_second { 503 } else { 200 })
            .expect(if fail_second { 3 } else { 1 })
            .with_body("payload")
            .create_async()
            .await;
        // The test resolver maps stable HTTPS declarations to a local transport fixture.
        for (need, name) in plan.needs.iter_mut().zip(["first", "second"]) {
            let alternatives = vec![format!("{}/{name}", server.url())];
            need.source = if name == "first" {
                BuildContentSource::Download(NonEmpty::new(alternatives).unwrap())
            } else {
                // A host without a catalog can still use original provider download evidence.
                let project = empack_core::identity::ProviderProjectId::Modrinth(
                    empack_core::identity::ModrinthProjectId::parse("AANobbMI").unwrap(),
                );
                BuildContentSource::Provider {
                    pin: ResolvedPin {
                        selection: project.parse_pin("abcdefgh").unwrap(),
                        project,
                    },
                    slot: empack_core::model::FileSlot::parse("second").unwrap(),
                    alternatives,
                }
            };
        }
        let governor = ResourceGovernor::new(ResourceRequest {
            jobs: 1,
            memory_bytes: 1 << 20,
            scratch_bytes: 16,
            open_files: 10,
        });
        let runtime = OperationRuntime::new(governor.clone(), 2);
        let transport = HttpAcquisition::for_loopback_tests();
        let mut handle = runtime
            .start(move |mut scope| async move {
                Ok(plan
                    .acquire_http(
                        &transport,
                        &mut scope,
                        SourceEvidencePolicy::Compatibility,
                        TransferLimits {
                            file_bytes: 16,
                            transfer_bytes: 16,
                            ..TransferLimits::default()
                        },
                    )
                    .await)
            })
            .unwrap();
        let outcome = handle.wait().await;
        runtime.shutdown().await;
        assert_eq!(
            fs::read(root.path().join("dist/result.mrpack")).unwrap(),
            b"previous artifact"
        );
        if fail_second {
            assert!(matches!(&*outcome, OperationOutcome::Completed(Err(_))));
            assert_eq!(governor.status().reserved, ResourceRequest::default());
        } else {
            let result = match &*outcome {
                OperationOutcome::Completed(Ok(result)) => result,
                _ => panic!("acquisition failed"),
            };
            assert!(result.pending.is_empty());
            assert_eq!(result.acquired.locked.len(), 2);
            let prepared = prepare_mrpack_build(
                workspace,
                path("result.mrpack"),
                &result.acquired,
                SourceEvidencePolicy::Compatibility,
                OptionalConversion::RejectMetadataLoss,
                &Cancellation::default(),
            )
            .unwrap();
            let publisher = Publisher::open(&host.path().join("private")).unwrap();
            prepared
                .publish(&publisher, &Cancellation::default())
                .unwrap();
            let mut archive = zip::ZipArchive::new(
                fs::File::open(root.path().join("dist/result.mrpack")).unwrap(),
            )
            .unwrap();
            let index: serde_json::Value =
                serde_json::from_reader(archive.by_name("modrinth.index.json").unwrap()).unwrap();
            assert_eq!(index["files"].as_array().unwrap().len(), 3);
            assert_eq!(
                index["files"][0]["downloads"][0],
                "https://example.com/unrelated-name.jar"
            );
            assert_eq!(
                fs::read(root.path().join("empack.lock")).unwrap(),
                original_lock
            );
        }
        first.assert_async().await;
        second.assert_async().await;
    }
}
fn manual_embedded_project() -> ResolvedProject {
    let initial = project(true, false);
    let mut intent = initial.intent().clone();
    let id = empack_core::identity::ProviderProjectId::Modrinth(
        empack_core::identity::ModrinthProjectId::parse("AANobbMI").unwrap(),
    );
    let pin = ResolvedPin {
        project: id.clone(),
        selection: id.parse_pin("Version1").unwrap(),
    };
    intent.roots.values_mut().next().unwrap().source =
        empack_core::model::SourceIntent::Provider(id.clone());
    let decoded = DocumentCodec
        .decode_intent(&DocumentCodec.encode_intent(&intent).unwrap(), "fixture")
        .unwrap();
    let mut lock = initial.lock().clone();
    lock.intent_revision = decoded.semantic_revision();
    let dependency = lock.dependencies.values_mut().next().unwrap();
    dependency.identity = empack_core::model::ResolvedIdentity::Provider(id);
    dependency.selected = Some(pin.clone());
    let mut files = dependency.files.as_slice().to_vec();
    files[0].acquisition = AcquisitionSpec::Manual {
        pin: Some(pin),
        instructions: "select the exact file".into(),
    };
    files[1].acquisition = AcquisitionSpec::Embedded {
        archive: path("pack/sources/source.zip"),
        member: path("assets/item.zip"),
    };
    dependency.files = NonEmpty::new(files).unwrap();
    ResolvedProject::validate(intent, lock.clone(), lock.intent_revision).unwrap()
}

#[tokio::test]
async fn unresolved_manual_and_archive_sources_remain_explicit_pending_obligations() {
    let resolved = manual_embedded_project();
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    write_project(root.path(), &resolved);
    let plan = plan_build_acquisitions(
        &capture(root.path(), host.path()),
        &BuildAcquisitions::default(),
        BuildMaterialization::AllContent,
        &Cancellation::default(),
    )
    .unwrap();
    let governor = ResourceGovernor::new(ResourceRequest::default());
    let runtime = OperationRuntime::new(governor.clone(), 1);
    let mut handle = runtime
        .start(move |mut scope| async move {
            Ok(plan
                .acquire_http(
                    &HttpAcquisition::for_loopback_tests(),
                    &mut scope,
                    SourceEvidencePolicy::Compatibility,
                    TransferLimits::default(),
                )
                .await)
        })
        .unwrap();
    let outcome = handle.wait().await;
    let result = match &*outcome {
        OperationOutcome::Completed(Ok(result)) => result,
        _ => panic!("pending work must not need admission"),
    };
    assert_eq!(result.pending.len(), 2);
    assert!(result.acquired.locked.is_empty());
    assert!(matches!(
        &result.pending[0].source,
        BuildContentSource::Manual { pin: Some(_) }
    ));
    assert!(matches!(
        &result.pending[1].source,
        BuildContentSource::Embedded { .. }
    ));
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    assert!(!host.path().join("private").exists());
    assert!(!root.path().join("dist").exists());
}

#[test]
fn captured_embedded_members_feed_verified_publication_without_distributing_the_source_archive() {
    use std::io::{Cursor, Read, Write};
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    write_project(root.path(), &manual_embedded_project());
    fs::create_dir_all(root.path().join("pack/resourcepacks")).unwrap();
    fs::write(root.path().join("pack/resourcepacks/a.zip"), b"payload").unwrap();
    fs::create_dir_all(root.path().join("pack/sources")).unwrap();
    let archive_path = root.path().join("pack/sources/source.zip");
    let write_source = |bytes: &[u8]| {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file(
            "assets/item.zip",
            zip::write::SimpleFileOptions::default().unix_permissions(0o755),
        )
        .unwrap();
        zip.write_all(bytes).unwrap();
        fs::write(&archive_path, zip.finish().unwrap().into_inner()).unwrap();
    };
    write_source(b"payload");
    let cancel = Cancellation::default();
    let workspace = capture(root.path(), host.path());
    let plan = plan_build_acquisitions(
        &workspace,
        &BuildAcquisitions::default(),
        BuildMaterialization::ReferenceArchive,
        &cancel,
    )
    .unwrap();
    assert_eq!(plan.needs().len(), 1);
    let acquired = plan
        .begin()
        .acquire_embedded(
            &workspace,
            crate::engine::artifacts::ArchiveLimits::default(),
            SourceEvidencePolicy::Compatibility,
            &cancel,
        )
        .unwrap();
    assert!(acquired.pending.is_empty());
    assert_eq!(acquired.acquired.locked.len(), 1);
    let prepared = prepare_mrpack_build(
        workspace,
        path("result.mrpack"),
        &acquired.acquired,
        SourceEvidencePolicy::Compatibility,
        OptionalConversion::RejectMetadataLoss,
        &cancel,
    )
    .unwrap();
    assert!(!root.path().join("dist").exists());
    prepared
        .publish(
            &Publisher::open(&host.path().join("private")).unwrap(),
            &cancel,
        )
        .unwrap();
    let previous = fs::read(root.path().join("dist/result.mrpack")).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(&previous)).unwrap();
    assert!(zip.by_name("overrides/sources/source.zip").is_err());
    let mut entry = zip.by_name("client-overrides/resourcepacks/b.zip").unwrap();
    assert_ne!(entry.unix_mode().unwrap() & 0o100, 0);
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"payload");
    write_source(b"changed");
    let workspace = capture(root.path(), host.path());
    let plan = plan_build_acquisitions(
        &workspace,
        &BuildAcquisitions::default(),
        BuildMaterialization::ReferenceArchive,
        &cancel,
    )
    .unwrap();
    assert!(
        plan.begin()
            .acquire_embedded(
                &workspace,
                crate::engine::artifacts::ArchiveLimits::default(),
                SourceEvidencePolicy::Compatibility,
                &cancel
            )
            .is_err()
    );
    assert_eq!(
        fs::read(root.path().join("dist/result.mrpack")).unwrap(),
        previous
    );
}

#[test]
fn selected_artifact_cannot_overwrite_a_declared_local_source() {
    let initial = project(true, false);
    let mut intent = initial.intent().clone();
    let source = path("dist/result.mrpack");
    intent.roots.values_mut().next().unwrap().source =
        empack_core::model::SourceIntent::Local(source.clone());
    let decoded = DocumentCodec
        .decode_intent(&DocumentCodec.encode_intent(&intent).unwrap(), "fixture")
        .unwrap();
    let mut lock = initial.lock().clone();
    lock.intent_revision = decoded.semantic_revision();
    let (key, dependency) = lock.dependencies.iter_mut().next().unwrap();
    dependency.identity = empack_core::model::ResolvedIdentity::Local(key.clone());
    dependency.files = NonEmpty::new(
        dependency
            .files
            .as_slice()
            .iter()
            .cloned()
            .map(|mut file| {
                file.acquisition = AcquisitionSpec::Local(source.clone());
                file
            })
            .collect(),
    )
    .unwrap();
    let project = ResolvedProject::validate(intent, lock.clone(), lock.intent_revision).unwrap();
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    write_project(root.path(), &project);
    fs::create_dir(root.path().join("dist")).unwrap();
    fs::write(root.path().join(source.as_str()), b"payload").unwrap();
    let reader = ProjectReader::new(RecoveryReader::new(host.path().join("private")));
    for output in ["result.mrpack", "RESULT.mrpack"] {
        assert!(
            reader
                .capture_build(
                    root.path(),
                    &[path(output)],
                    SnapshotLimits::default(),
                    &Cancellation::default()
                )
                .is_err()
        );
    }
    assert_eq!(
        fs::read(root.path().join(source.as_str())).unwrap(),
        b"payload"
    );
    assert!(!host.path().join("private").exists());
}

#[tokio::test]
async fn provider_slots_share_one_exact_resolution_and_keep_their_assertions() {
    use crate::engine::providers::{CatalogLimits, ProviderCatalog};
    use empack_core::{
        identity::{ModrinthProjectId, ProviderProjectId},
        model::{DependencyKey, FileSlot},
    };
    use serde_json::json;
    let mut server = mockito::Server::new_async().await;
    let catalog = ProviderCatalog::for_loopback_tests(&server.url(), None);
    let id = ProviderProjectId::Modrinth(ModrinthProjectId::parse("AANobbMI").unwrap());
    let pin = ResolvedPin {
        selection: id.parse_pin("abcdefgh").unwrap(),
        project: id,
    };
    let project = server
        .mock("GET", "/project/AANobbMI")
        .with_body(
            json!({"id":"AANobbMI","slug":"assets","title":"Assets","project_type":"resourcepack"})
                .to_string(),
        )
        .expect(1)
        .create_async()
        .await;
    let expected = ExpectedContent {
        digests: Some(DigestSet::parse([("md5", "321c3cf486ed509164edec1e1981fec8")]).unwrap()),
        size: Some(7),
        accepted_observation: None,
    };
    let files: Vec<_> = ["first.zip", "second.zip"].iter().map(|name|json!({"filename":name,"primary":false,"size":7,"hashes":{"md5":"321c3cf486ed509164edec1e1981fec8"},"url":format!("https://example.com/{name}")})).collect();
    let selection = server.mock("GET", "/version/abcdefgh")
        .with_body(json!({"id":"abcdefgh","project_id":"AANobbMI","files":files,"game_versions":["1.20.1"],"loaders":["minecraft"],"dependencies":[]}).to_string())
        .expect(1).create_async().await;
    let mut plan = BuildAcquisitionResult {
        acquired: BuildAcquisitions::default(),
        pending: ["first.zip", "second.zip"]
            .iter()
            .map(|name| {
                let slot = FileSlot::parse(name).unwrap();
                AcquisitionNeed {
                    key: AcquisitionKey::Locked(LockedFileKey {
                        dependency: DependencyKey::parse("assets").unwrap(),
                        slot: slot.clone(),
                    }),
                    reason: AcquisitionReason::MaterializedTarget,
                    expected: expected.clone(),
                    source: BuildContentSource::Provider {
                        pin: pin.clone(),
                        slot,
                        alternatives: Vec::new(),
                    },
                }
            })
            .collect(),
    };
    let member_expected = ExpectedContent {
        digests: None,
        size: Some(3),
        accepted_observation: Some(empack_core::digest::ContentId::from_sha256([7; 32])),
    };
    plan.pending.push(AcquisitionNeed {
        key: AcquisitionKey::Locked(LockedFileKey {
            dependency: DependencyKey::parse("world").unwrap(),
            slot: FileSlot::parse("level.dat").unwrap(),
        }),
        reason: AcquisitionReason::MaterializedTarget,
        expected: member_expected.clone(),
        source: BuildContentSource::ProviderArchiveMember {
            archive: empack_core::model::ProviderArchiveSource {
                pin,
                slot: FileSlot::parse("first.zip").unwrap(),
                expected: expected.clone(),
                alternatives: vec![],
            },
            member: path("World/level.dat"),
        },
    });
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 1,
        memory_bytes: 1 << 20,
        scratch_bytes: 0,
        open_files: 8,
    });
    let runtime = OperationRuntime::new(governor.clone(), 1);
    let mut handle = runtime
        .start(move |mut scope| async move {
            Ok(plan
                .refresh_provider_locators(
                    &catalog,
                    &mut scope,
                    CatalogLimits {
                        response_bytes: 4096,
                        transfer_bytes: 8192,
                        deadline: std::time::Duration::from_secs(2),
                    },
                )
                .await)
        })
        .unwrap();
    let outcome = handle.wait().await;
    let OperationOutcome::Completed(Ok(result)) = &*outcome else {
        panic!("refresh failed")
    };
    for (need, name) in result.pending.iter().zip(["first.zip", "second.zip"]) {
        assert_eq!(need.expected, expected);
        assert!(
            matches!(&need.source, BuildContentSource::Download(urls) if urls.as_slice()==[format!("https://example.com/{name}")])
        );
    }
    assert_eq!(result.pending[2].expected, member_expected);
    assert!(
        matches!(&result.pending[2].source,BuildContentSource::ProviderArchiveMember {archive,..} if archive.expected==expected && archive.alternatives==["https://example.com/first.zip"])
    );
    project.assert_async().await;
    selection.assert_async().await;
    runtime.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}

#[tokio::test]
async fn build_downloads_share_the_batch_budget_and_validate_all_declarations_first() {
    for invalid_later_declaration in [false, true] {
        let mut server = mockito::Server::new_async().await;
        let first = server
            .mock("GET", "/first")
            .with_body("payload")
            .expect(usize::from(!invalid_later_declaration))
            .create_async()
            .await;
        let second = server
            .mock("GET", "/second")
            .with_body("payload")
            .expect(usize::from(!invalid_later_declaration))
            .create_async()
            .await;
        let expected = || ExpectedContent {
            digests: Some(
                DigestSet::new(vec![
                    empack_core::digest::ExpectedDigest::parse(
                        "md5",
                        "321c3cf486ed509164edec1e1981fec8",
                    )
                    .unwrap(),
                ])
                .unwrap(),
            ),
            size: Some(7),
            accepted_observation: None,
        };
        let needs = ["first", "second"]
            .into_iter()
            .map(|name| {
                let mut expected = expected();
                if invalid_later_declaration && name == "second" {
                    expected.size = Some(100);
                }
                AcquisitionNeed {
                    key: AcquisitionKey::Locked(locked_key(name)),
                    reason: AcquisitionReason::MaterializedTarget,
                    expected,
                    source: BuildContentSource::Download(
                        NonEmpty::new(vec![format!("{}/{name}", server.url())]).unwrap(),
                    ),
                }
            })
            .collect();
        let plan = BuildAcquisitionPlan { needs };
        let governor = ResourceGovernor::new(ResourceRequest {
            jobs: 1,
            memory_bytes: 1 << 20,
            scratch_bytes: 64,
            open_files: 10,
        });
        let runtime = OperationRuntime::new(governor.clone(), 1);
        let mut handle = runtime
            .start(move |mut scope| async move {
                Ok(plan
                    .acquire_http(
                        &HttpAcquisition::for_loopback_tests(),
                        &mut scope,
                        SourceEvidencePolicy::Compatibility,
                        TransferLimits {
                            file_bytes: 16,
                            transfer_bytes: 10,
                            ..TransferLimits::default()
                        },
                    )
                    .await)
            })
            .unwrap();
        let outcome = handle.wait().await;
        assert!(
            matches!(&*outcome, OperationOutcome::Completed(Err(_))),
            "each file passed, but the combined byte budget was exceeded"
        );
        first.assert_async().await;
        second.assert_async().await;
        runtime.release_completed(handle.id());
        runtime.shutdown().await;
        drop(handle);
        drop(outcome);
        assert_eq!(governor.status().reserved, ResourceRequest::default());
    }
}

#[tokio::test]
async fn refreshed_provider_origins_retain_verified_saved_download_fallbacks() {
    use crate::engine::providers::{CatalogLimits, ProviderCatalog};
    use empack_core::{
        identity::{ModrinthProjectId, ProviderProjectId},
        model::{DependencyKey, FileSlot},
    };
    use serde_json::json;
    use std::io::Read;
    let mut server = mockito::Server::new_async().await;
    let catalog = ProviderCatalog::for_loopback_tests(&server.url(), None);
    let project = ProviderProjectId::Modrinth(ModrinthProjectId::parse("AANobbMI").unwrap());
    let pin = ResolvedPin {
        selection: project.parse_pin("abcdefgh").unwrap(),
        project,
    };
    let metadata_project = server
        .mock("GET", "/project/AANobbMI")
        .with_body(
            json!({"id":"AANobbMI","slug":"assets","title":"Assets","project_type":"resourcepack"})
                .to_string(),
        )
        .create_async()
        .await;
    let metadata_version = server.mock("GET", "/version/abcdefgh").with_body(json!({"id":"abcdefgh","project_id":"AANobbMI","files":[{"filename":"assets.zip","primary":true,"size":7,"hashes":{"md5":"321c3cf486ed509164edec1e1981fec8"},"url":format!("{}/unavailable", server.url())}],"game_versions":["1.20.1"],"loaders":["minecraft"],"dependencies":[]}).to_string()).create_async().await;
    let refreshed = server
        .mock("GET", "/unavailable")
        .with_status(404)
        .expect(1)
        .create_async()
        .await;
    let saved = server
        .mock("GET", "/saved")
        .with_body("payload")
        .expect(1)
        .create_async()
        .await;
    let plan = BuildAcquisitionResult {
        acquired: BuildAcquisitions::default(),
        pending: vec![AcquisitionNeed {
            key: AcquisitionKey::Locked(LockedFileKey {
                dependency: DependencyKey::parse("assets").unwrap(),
                slot: FileSlot::parse("primary").unwrap(),
            }),
            reason: AcquisitionReason::MaterializedTarget,
            expected: ExpectedContent {
                digests: Some(
                    DigestSet::parse([("md5", "321c3cf486ed509164edec1e1981fec8")]).unwrap(),
                ),
                size: Some(7),
                accepted_observation: None,
            },
            source: BuildContentSource::Provider {
                pin,
                slot: FileSlot::parse("primary").unwrap(),
                alternatives: vec![format!("{}/saved", server.url())],
            },
        }],
    };
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 1,
        memory_bytes: 4 << 20,
        scratch_bytes: 32,
        open_files: 16,
    });
    let runtime = OperationRuntime::new(governor.clone(), 1);
    let mut handle = runtime
        .start(move |mut scope| async move {
            let result = async {
                plan.refresh_provider_locators(
                    &catalog,
                    &mut scope,
                    CatalogLimits {
                        response_bytes: 4096,
                        transfer_bytes: 8192,
                        deadline: std::time::Duration::from_secs(2),
                    },
                )
                .await?
                .acquire_http(
                    &HttpAcquisition::for_loopback_tests(),
                    &mut scope,
                    SourceEvidencePolicy::Compatibility,
                    TransferLimits {
                        file_bytes: 16,
                        transfer_bytes: 16,
                        ..Default::default()
                    },
                )
                .await
            }
            .await;
            Ok(result)
        })
        .unwrap();
    let outcome = handle.wait().await;
    runtime.shutdown().await;
    let result = match &*outcome {
        OperationOutcome::Completed(Ok(result)) => result,
        OperationOutcome::Completed(Err(error)) => {
            panic!("Saved fallback was discarded: {error:#}")
        }
        _ => panic!("Expected verified acquisition"),
    };
    assert!(result.pending.is_empty());
    let mut bytes = Vec::new();
    result
        .acquired
        .locked
        .values()
        .next()
        .unwrap()
        .content
        .lease()
        .open()
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(bytes, b"payload");
    for mock in [metadata_project, metadata_version, refreshed, saved] {
        mock.assert_async().await;
    }
    drop(handle);
    drop(runtime);
    drop(outcome);
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}

#[tokio::test]
async fn retained_build_inputs_reduce_the_http_batch_allowance() {
    for allowance in [13, 14] {
        let mut server = mockito::Server::new_async().await;
        let download = server
            .mock("GET", "/second")
            .with_body("payload")
            .expect(1)
            .create_async()
            .await;
        let expected = ExpectedContent {
            digests: Some(
                DigestSet::new(vec![
                    empack_core::digest::ExpectedDigest::parse(
                        "md5",
                        "321c3cf486ed509164edec1e1981fec8",
                    )
                    .unwrap(),
                ])
                .unwrap(),
            ),
            size: Some(7),
            accepted_observation: None,
        };
        let content = crate::engine::content::verify_stream(
            &mut &b"payload"[..],
            &expected,
            7,
            SourceEvidencePolicy::Compatibility,
            InitialObservation::RequireEvidence,
            &Cancellation::default(),
        )
        .unwrap();
        let result = BuildAcquisitionResult {
            acquired: BuildAcquisitions {
                locked: BTreeMap::from([(
                    locked_key("first"),
                    AcquiredBuildFile {
                        content,
                        permissions: FilePermissions {
                            readonly: false,
                            executable: false,
                        },
                    },
                )]),
            },
            pending: vec![AcquisitionNeed {
                key: AcquisitionKey::Locked(locked_key("second")),
                reason: AcquisitionReason::MaterializedTarget,
                expected,
                source: BuildContentSource::Download(
                    NonEmpty::new(vec![format!("{}/second", server.url())]).unwrap(),
                ),
            }],
        };
        let governor = ResourceGovernor::new(ResourceRequest {
            jobs: 1,
            memory_bytes: 1 << 20,
            scratch_bytes: 64,
            open_files: 10,
        });
        let runtime = OperationRuntime::new(governor.clone(), 1);
        let mut handle = runtime
            .start(move |mut scope| async move {
                Ok(result
                    .acquire_http(
                        &HttpAcquisition::for_loopback_tests(),
                        &mut scope,
                        SourceEvidencePolicy::Compatibility,
                        TransferLimits {
                            file_bytes: 16,
                            transfer_bytes: allowance,
                            ..TransferLimits::default()
                        },
                    )
                    .await)
            })
            .unwrap();
        let outcome = handle.wait().await;
        match &*outcome {
            OperationOutcome::Completed(result) => {
                assert_eq!(result.is_ok(), allowance == 14);
                if let Ok(result) = result {
                    assert!(result.pending.is_empty());
                    assert_eq!(result.acquired.retained_bytes().unwrap(), 14);
                }
            }
            _ => panic!("HTTP batch did not complete"),
        }
        download.assert_async().await;
        runtime.release_completed(handle.id());
        runtime.shutdown().await;
        drop((handle, outcome));
        assert_eq!(governor.status().reserved, ResourceRequest::default());
    }
}

#[tokio::test]
async fn provider_archive_acquisition_shares_one_download_and_verifies_every_member() {
    use empack_core::{
        digest::{ContentId, ExpectedDigest},
        identity::{ModrinthProjectId, ProviderProjectId},
        model::{FileSlot, ProviderArchiveSource},
    };
    use sha2::Digest;
    use std::io::{Cursor, Write};
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for name in ["World/level.dat", "World/region/r.0.0.mca"] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"payload").unwrap();
    }
    let bytes = zip.finish().unwrap().into_inner();
    for failure in [
        None,
        Some("archive"),
        Some("member"),
        Some("limit"),
        Some("weak"),
    ] {
        let mut server = mockito::Server::new_async().await;
        let download = server
            .mock("GET", "/world.zip")
            .with_body(bytes.clone())
            .expect(1)
            .create_async()
            .await;
        let id = ProviderProjectId::Modrinth(ModrinthProjectId::parse("AANobbMI").unwrap());
        let pin = ResolvedPin {
            selection: id.parse_pin("abcdefgh").unwrap(),
            project: id,
        };
        let digest = if failure == Some("weak") {
            ExpectedDigest::Md5(md5::Md5::digest(&bytes).into())
        } else {
            ExpectedDigest::Sha256(if failure == Some("archive") {
                [0; 32]
            } else {
                sha2::Sha256::digest(&bytes).into()
            })
        };
        let archive = ProviderArchiveSource {
            pin,
            slot: FileSlot::parse("world.zip").unwrap(),
            expected: ExpectedContent {
                digests: Some(DigestSet::new(vec![digest]).unwrap()),
                size: Some(bytes.len() as u64),
                accepted_observation: None,
            },
            alternatives: vec![format!("{}/world.zip", server.url())],
        };
        let recorded_archive = archive.clone();
        let needs = ["level.dat", "region/r.0.0.mca"]
            .into_iter()
            .map(|name| AcquisitionNeed {
                key: AcquisitionKey::Locked(LockedFileKey {
                    dependency: empack_core::model::DependencyKey::parse("world").unwrap(),
                    slot: FileSlot::parse(name).unwrap(),
                }),
                reason: AcquisitionReason::MaterializedTarget,
                expected: ExpectedContent {
                    digests: None,
                    size: Some(7),
                    accepted_observation: Some(ContentId::from_sha256(
                        if failure == Some("member") && name != "level.dat" {
                            [0; 32]
                        } else {
                            sha2::Sha256::digest(b"payload").into()
                        },
                    )),
                },
                source: BuildContentSource::ProviderArchiveMember {
                    archive: archive.clone(),
                    member: path(&format!("World/{name}")),
                },
            })
            .collect();
        let governor = ResourceGovernor::new(ResourceRequest {
            jobs: 2,
            memory_bytes: 8 << 20,
            scratch_bytes: 1 << 20,
            open_files: 32,
        });
        let runtime = OperationRuntime::new(governor.clone(), 2);
        let mut handle = runtime
            .start(move |mut scope| async move {
                Ok(BuildAcquisitionPlan { needs }
                    .begin()
                    .acquire_http_with_archives(
                        &HttpAcquisition::for_loopback_tests(),
                        &mut scope,
                        SourceEvidencePolicy::StrongSourceRequired,
                        TransferLimits {
                            file_bytes: 1 << 20,
                            transfer_bytes: 1 << 20,
                            ..Default::default()
                        },
                        crate::engine::artifacts::ArchiveLimits {
                            entries: 16,
                            total_bytes: if failure == Some("limit") { 10 } else { 64 },
                            ..Default::default()
                        },
                    )
                    .await)
            })
            .unwrap();
        let outcome = handle.wait().await;
        runtime.shutdown().await;
        match &*outcome {
            OperationOutcome::Completed(Ok(result)) if failure.is_none() => {
                assert!(result.pending.is_empty());
                assert_eq!(result.acquired.locked.len(), 2);
                for file in result.acquired.locked.values() {
                    assert_eq!(file.content.lease().len(), 7);
                    assert!(matches!(
                        file.content.evidence(),
                        empack_core::digest::IntegrityEvidence::ObservedOnly { .. }
                    ));
                }
                verify_world_build(recorded_archive, &result.acquired);
            }
            OperationOutcome::Completed(Err(_)) if failure.is_some() => {}
            other => panic!(
                "Unexpected archive result for {failure:?}: {}",
                matches!(other, OperationOutcome::Completed(Ok(_)))
            ),
        }
        if failure == Some("weak") {
            assert!(!download.matched_async().await);
        } else {
            download.assert_async().await;
        }
        drop(outcome);
        drop(handle);
        drop(runtime);
        assert_eq!(governor.status().reserved, ResourceRequest::default());
    }
}

fn verify_world_build(
    mut archive: empack_core::model::ProviderArchiveSource,
    acquired: &BuildAcquisitions,
) {
    use empack_core::{
        distribution::Recipe, inventory::OptionalPolicy, model::*, path::InstallDestination,
    };
    let base = project(false, false);
    let mut intent = base.intent().clone();
    let mut root_intent = intent.roots.values().next().unwrap().clone();
    let key = DependencyKey::parse("world").unwrap();
    let requirements = root_intent.requirements.clone();
    root_intent.kind = ContentKind::World;
    root_intent.source = SourceIntent::Provider(archive.pin.project.clone());
    root_intent.version = VersionIntent::Exact(archive.pin.selection.clone());
    root_intent.placement = PlacementIntent::ArchiveRoot(
        NonEmpty::new(vec![Placement {
            layer: ContentLayer::Common,
            requirements: requirements.clone(),
            destination: InstallDestination::parse("saves/MyWorld").unwrap(),
        }])
        .unwrap(),
    );
    intent.roots = BTreeMap::from([(key.clone(), root_intent)]);
    archive.alternatives = vec!["https://example.com/world.zip".into()];
    let files = acquired
        .locked
        .iter()
        .map(|(key, file)| ResolvedFile {
            slot: key.slot.clone(),
            expected: ExpectedContent {
                digests: None,
                size: Some(file.content.lease().len()),
                accepted_observation: Some(file.content.lease().id()),
            },
            acquisition: AcquisitionSpec::ProviderArchiveMember {
                archive: archive.clone(),
                member: path(&format!("World/{}", key.slot.as_str())),
            },
            provenance: Provenance {
                source: "provider-archive".into(),
                location: None,
                declared_digests: None,
                conversions: vec![],
            },
            placements: NonEmpty::new(vec![Placement {
                layer: ContentLayer::Common,
                requirements: requirements.clone(),
                destination: InstallDestination::parse(&format!(
                    "saves/MyWorld/{}",
                    key.slot.as_str()
                ))
                .unwrap(),
            }])
            .unwrap(),
        })
        .collect();
    let revision = DocumentCodec
        .decode_intent(&DocumentCodec.encode_intent(&intent).unwrap(), "world")
        .unwrap()
        .semantic_revision();
    let mut lock = base.lock().clone();
    lock.intent_revision = revision;
    lock.dependencies = BTreeMap::from([(
        key.clone(),
        LockedDependency {
            title: "World".into(),
            kind: ContentKind::World,
            identity: ResolvedIdentity::Provider(archive.pin.project.clone()),
            selected: Some(archive.pin),
            files: NonEmpty::new(files).unwrap(),
        },
    )]);
    lock.coverage = BTreeMap::from([(key, empack_core::model::Coverage::CompleteForSelection)]);
    lock.required_edges.clear();
    let project = ResolvedProject::validate(intent, lock, revision).unwrap();
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    write_project(root.path(), &project);
    fs::create_dir_all(root.path().join("pack/saves/MyWorld/region")).unwrap();
    for name in ["level.dat", "region/r.0.0.mca"] {
        fs::write(
            root.path().join("pack/saves/MyWorld").join(name),
            b"payload",
        )
        .unwrap();
    }
    let cancel = Cancellation::default();
    let workspace = capture(root.path(), host.path());
    for target in [Recipe::MODRINTH, Recipe::PRISM_BUNDLED] {
        assert_eq!(
            plan_target_build_acquisitions(
                &workspace,
                &BuildAcquisitions::default(),
                target,
                &OptionalPolicy::Preserve,
                SourceEvidencePolicy::StrongSourceRequired,
                &cancel
            )
            .unwrap()
            .needs()
            .len(),
            2
        );
        assert!(
            plan_target_build_acquisitions(
                &workspace,
                acquired,
                target,
                &OptionalPolicy::Preserve,
                SourceEvidencePolicy::StrongSourceRequired,
                &cancel
            )
            .unwrap()
            .needs()
            .is_empty()
        );
    }
    assert!(
        prepare_mrpack_build(
            capture(root.path(), host.path()),
            path("result.mrpack"),
            &BuildAcquisitions::default(),
            SourceEvidencePolicy::StrongSourceRequired,
            OptionalConversion::RejectMetadataLoss,
            &cancel
        )
        .is_err()
    );
    let prepared = prepare_mrpack_build(
        workspace,
        path("result.mrpack"),
        acquired,
        SourceEvidencePolicy::StrongSourceRequired,
        OptionalConversion::RejectMetadataLoss,
        &cancel,
    )
    .unwrap();
    prepared
        .publish(
            &Publisher::open(&host.path().join("private")).unwrap(),
            &cancel,
        )
        .unwrap();
    let mut output =
        zip::ZipArchive::new(fs::File::open(root.path().join("dist/result.mrpack")).unwrap())
            .unwrap();
    use std::io::Read;
    let mut payload = Vec::new();
    output
        .by_name("client-overrides/saves/MyWorld/level.dat")
        .unwrap()
        .read_to_end(&mut payload)
        .unwrap();
    assert_eq!(payload, b"payload");
}
