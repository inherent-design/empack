use super::*;
use crate::engine::api::{
    build_input_tests::{fixture, supplied},
    tests::{engine, request},
};
use std::{
    fs,
    path::{Path, PathBuf},
};
fn documents(root: &Path) -> Vec<Vec<u8>> {
    ["empack.yml", "empack.lock"]
        .iter()
        .map(|name| fs::read(root.join(name)).unwrap())
        .collect()
}
fn records(state: &Path) -> Vec<PathBuf> {
    fs::read_dir(state.join("pending-builds"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect()
}
async fn suspend_first(owner: &Engine, root: &Path) -> SuspendedBuildReceipt {
    let pending = match owner
        .prepare(
            root.to_path_buf(),
            request().with_content(supplied("first", b"payload")),
        )
        .await
        .unwrap()
    {
        Preparation::NeedsInput(pending) => pending,
        _ => panic!("second input is pending"),
    };
    owner.suspend_build(*pending).await.unwrap()
}
#[tokio::test]
async fn durable_build_resume_reverifies_content_and_requires_fresh_approval() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    fixture(root.path(), true);
    let before = documents(root.path());
    let state = host.path().join("state");
    let (owner, governor) = engine(state.clone());
    let receipt = suspend_first(&owner, root.path()).await;
    assert_eq!(receipt.retained_files, 1);
    assert_eq!(receipt.retained_bytes, 7);
    assert_eq!(receipt.pending_files, 1);
    assert!(!receipt.replaced);
    assert_eq!(documents(root.path()), before);
    assert!(!root.path().join("dist").exists());
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    owner.shutdown().await;
    drop(owner);
    let record_path = records(&state).pop().unwrap();
    let record_before = fs::read(&record_path).unwrap();
    let (owner, governor) = engine(state.clone());
    let resumed = owner
        .resume_saved_build(root.path().to_path_buf())
        .await
        .unwrap();
    let SavedBuildResume::Prepared(prepared) = resumed else {
        panic!("saved build not prepared")
    };
    let Preparation::NeedsInput(pending) = prepared.preparation else {
        panic!("second input should remain pending")
    };
    assert_eq!(pending.build().unwrap().unresolved.len(), 1);
    assert_eq!(fs::read(&record_path).unwrap(), record_before);
    assert_eq!(documents(root.path()), before);
    assert!(!root.path().join("dist").exists());
    let prepared = match owner
        .resume(*pending, supplied("second", b"payload"))
        .await
        .unwrap()
    {
        Preparation::Ready(prepared) => prepared,
        _ => panic!("both inputs supplied"),
    };
    let grant = ExecutionGrant {
        plan: prepared.view().plan(),
        replacement: prepared.view().replacement(),
        network: NetworkPermission::Offline,
        run_installer: false,
        run_runtime: false,
    };
    let mut handle = owner.start(prepared.authorize(grant).unwrap()).unwrap();
    let outcome = handle.wait().await;
    assert!(matches!(
        &*outcome,
        OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Build(_)))
    ));
    assert!(root.path().join("dist/result.mrpack").is_file());
    assert!(root.path().join("dist/client.zip").is_file());
    assert_eq!(documents(root.path()), before);
    assert_eq!(fs::read(&record_path).unwrap(), record_before);
    owner.release_completed(handle.id());
    drop((outcome, handle));
    owner.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}
#[tokio::test]
async fn stale_and_missing_saved_build_inspection_is_read_only() {
    for changed in ["intent", "lock", "source"] {
        let root = tempfile::tempdir().unwrap();
        let host = tempfile::tempdir().unwrap();
        fixture(root.path(), false);
        let state = host.path().join("state");
        let (owner, governor) = engine(state.clone());
        assert!(matches!(
            owner
                .resume_saved_build(root.path().to_path_buf())
                .await
                .unwrap(),
            SavedBuildResume::Missing
        ));
        assert!(!state.exists());
        suspend_first(&owner, root.path()).await;
        let path = records(&state).pop().unwrap();
        let saved = fs::read(&path).unwrap();
        match changed {
            "intent" => {
                fs::write(root.path().join("empack.yml"), b"deliberately invalid: [").unwrap()
            }
            "lock" => fs::write(root.path().join("empack.lock"), b"changed").unwrap(),
            _ => {
                fs::create_dir_all(root.path().join("pack/config")).unwrap();
                fs::write(root.path().join("pack/config/changed"), b"changed").unwrap();
            }
        }
        assert!(
            matches!(
                owner
                    .resume_saved_build(root.path().to_path_buf())
                    .await
                    .unwrap(),
                SavedBuildResume::Stale
            ),
            "{changed}"
        );
        assert_eq!(fs::read(&path).unwrap(), saved);
        assert!(!root.path().join("dist").exists());
        assert_eq!(governor.status().reserved, ResourceRequest::default());
        owner.shutdown().await;
    }
}
#[tokio::test]
async fn missing_or_invalid_cached_content_never_becomes_verified_saved_input() {
    for corrupt in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let host = tempfile::tempdir().unwrap();
        fixture(root.path(), false);
        let state = host.path().join("state");
        let (owner, governor) = engine(state.clone());
        suspend_first(&owner, root.path()).await;
        let content = fs::read_dir(state.join("pending-content"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.file_name().unwrap() != "store.lock")
            .unwrap();
        if corrupt {
            fs::write(content, b"changed").unwrap();
        } else {
            fs::remove_file(content).unwrap();
        }
        let before = documents(root.path());
        let result = owner.resume_saved_build(root.path().to_path_buf()).await;
        if corrupt {
            assert!(result.is_err());
        } else {
            let SavedBuildResume::Prepared(prepared) = result.unwrap() else {
                panic!("cache miss should remain resumable")
            };
            let Preparation::NeedsInput(pending) = prepared.preparation else {
                panic!("cache miss cannot complete input")
            };
            assert_eq!(pending.build().unwrap().unresolved.len(), 2);
            drop(pending);
        }
        assert_eq!(documents(root.path()), before);
        assert!(!root.path().join("dist").exists());
        assert_eq!(governor.status().reserved, ResourceRequest::default());
        owner.shutdown().await;
    }
}
#[tokio::test]
async fn saved_records_reject_extra_slots_bad_paths_and_unknown_schema() {
    for mode in ["schema", "path", "duplicate", "extra"] {
        let root = tempfile::tempdir().unwrap();
        let host = tempfile::tempdir().unwrap();
        fixture(root.path(), false);
        let state = host.path().join("state");
        let (owner, governor) = engine(state.clone());
        suspend_first(&owner, root.path()).await;
        let path = records(&state).pop().unwrap();
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        match mode {
            "schema" => value["schema"] = 99.into(),
            "path" => value["recipe"]["outputs"][0][1] = "../escape.zip".into(),
            "duplicate" => {
                let duplicate = value["files"][0].clone();
                value["files"].as_array_mut().unwrap().push(duplicate);
            }
            _ => value["files"][0]["key"]["slot"] = "unrequested".into(),
        }
        let bytes = serde_json::to_vec(&value).unwrap();
        fs::write(&path, &bytes).unwrap();
        assert!(
            owner
                .resume_saved_build(root.path().to_path_buf())
                .await
                .is_err(),
            "{mode}"
        );
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert!(!root.path().join("dist").exists());
        assert_eq!(governor.status().reserved, ResourceRequest::default());
        owner.shutdown().await;
    }
}

#[test]
fn saved_recipes_preserve_every_build_choice_and_reject_unknown_fields() {
    use crate::engine::templates::TemplateMode;
    for archive in [
        DistributionArchive::Zip,
        DistributionArchive::TarGz,
        DistributionArchive::SevenZip,
    ] {
        let mut selected = request();
        selected.clean = true;
        selected.archive = archive;
        selected.outputs = NonEmpty::new(
            [
                Recipe::MODRINTH,
                Recipe::PRISM_REFERENCES,
                Recipe::SERVER_REFERENCES,
                Recipe::PRISM_BUNDLED,
                Recipe::SERVER_BUNDLED,
            ]
            .into_iter()
            .enumerate()
            .map(|(index, target)| BuildOutput {
                target,
                artifact: PortableRelPath::parse(
                    &format!("result-{index}.zip"),
                    PathSyntax::ArtifactName,
                )
                .unwrap(),
            })
            .collect(),
        )
        .unwrap();
        selected.optional = OptionalPolicy::Resolve {
            choices: BTreeMap::from([("chosen".into(), true), ("rejected".into(), false)]),
            use_defaults: false,
        };
        selected.mrpack_optional = OptionalConversion::AcknowledgedMetadataLoss;
        selected.evidence = SourceEvidencePolicy::StrongSourceRequired;
        selected
            .templates
            .values
            .insert("custom".into(), "literal value".into());
        selected.templates.limits.input_bytes = 123;
        selected.templates.limits.output_bytes = 456;
        selected.templates.limits.total_bytes = 789;
        selected.templates.limits.entries = 3;
        for (name, mode) in [
            ("copy", TemplateMode::Copy),
            ("render", TemplateMode::Handlebars),
            ("auto", TemplateMode::TextOrBinary),
        ] {
            selected.templates.modes.insert(
                PortableRelPath::parse(name, PathSyntax::ProjectContent).unwrap(),
                mode,
            );
        }
        let recipe = record::SavedRecipe::from(&selected);
        let wire = serde_json::to_value(recipe).unwrap();
        let decoded: record::SavedRecipe = serde_json::from_value(wire.clone()).unwrap();
        let restored = decoded.parse().unwrap();
        assert_eq!(
            serde_json::to_value(record::SavedRecipe::from(&restored)).unwrap(),
            wire
        );
        assert_eq!(restored.outputs.as_slice(), selected.outputs.as_slice());
        assert_eq!(restored.optional, selected.optional);
        assert_eq!(restored.templates.values, selected.templates.values);
        let mut unexpected = wire;
        unexpected["run_command"] = "never execute me".into();
        assert!(serde_json::from_value::<record::SavedRecipe>(unexpected).is_err());
    }
}
#[tokio::test]
async fn foreign_owner_and_oversized_saved_records_have_no_project_effects() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    fixture(root.path(), false);
    let state = host.path().join("state");
    let (owner, governor) = engine(state.clone());
    let (other, other_governor) = engine(host.path().join("other"));
    let pending = match owner
        .prepare(root.path().to_path_buf(), request())
        .await
        .unwrap()
    {
        Preparation::NeedsInput(pending) => pending,
        _ => panic!("manual content is missing"),
    };
    assert!(other.suspend_build(*pending).await.is_err());
    assert!(!state.exists());
    assert!(!host.path().join("other").exists());
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    assert_eq!(other_governor.status().reserved, ResourceRequest::default());
    other.shutdown().await;
    suspend_first(&owner, root.path()).await;
    let path = records(&state).pop().unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(record::MAX_RECORD + 1)
        .unwrap();
    let before = documents(root.path());
    assert!(
        owner
            .resume_saved_build(root.path().to_path_buf())
            .await
            .is_err()
    );
    assert_eq!(fs::metadata(&path).unwrap().len(), record::MAX_RECORD + 1);
    assert_eq!(documents(root.path()), before);
    assert!(!root.path().join("dist").exists());
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    owner.shutdown().await;
}
#[cfg(unix)]
#[tokio::test]
async fn saved_record_links_cannot_authorize_outside_reads_or_replacement() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    fixture(root.path(), false);
    let state = host.path().join("state");
    let (owner, governor) = engine(state.clone());
    suspend_first(&owner, root.path()).await;
    let path = records(&state).pop().unwrap();
    let outside = host.path().join("sentinel");
    fs::write(&outside, b"retain outside bytes").unwrap();
    fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(&outside, &path).unwrap();
    assert!(
        owner
            .resume_saved_build(root.path().to_path_buf())
            .await
            .is_err()
    );
    let pending = match owner
        .prepare(root.path().to_path_buf(), request())
        .await
        .unwrap()
    {
        Preparation::NeedsInput(pending) => pending,
        _ => panic!("manual content is missing"),
    };
    assert!(owner.suspend_build(*pending).await.is_err());
    assert_eq!(fs::read(&outside).unwrap(), b"retain outside bytes");
    assert!(
        fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(!root.path().join("dist").exists());
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    owner.shutdown().await;
}

async fn suspend_with_budget(memory: u64, scratch: u64) {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    fixture(root.path(), false);
    let state = host.path().join("state");
    let (template, _) = engine(state.clone());
    let mut config = template.config.clone();
    template.shutdown().await;
    config.resources.capture.scratch_bytes = 0;
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 2,
        memory_bytes: memory,
        scratch_bytes: scratch,
        open_files: 128,
    });
    let owner = Engine::new(config.clone(), governor.clone()).unwrap();
    let mut inputs = supplied("first", b"payload");
    let mut permit = governor
        .try_admit(ResourceRequest {
            scratch_bytes: 7,
            open_files: 1,
            ..Default::default()
        })
        .unwrap();
    inputs
        .locked
        .values_mut()
        .next()
        .unwrap()
        .content
        .retain_reservation(&mut permit)
        .unwrap();
    drop(permit);
    let pending = match owner
        .prepare(root.path().to_path_buf(), request().with_content(inputs))
        .await
        .unwrap()
    {
        Preparation::NeedsInput(pending) => pending,
        _ => panic!("second input remains pending"),
    };
    let receipt = owner.suspend_build(*pending).await.unwrap();
    assert_eq!(receipt.retained_bytes, 7);
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    owner.shutdown().await;
    let owner = Engine::new(config, governor.clone()).unwrap();
    let SavedBuildResume::Prepared(prepared) = owner
        .resume_saved_build(root.path().to_path_buf())
        .await
        .unwrap()
    else {
        panic!("small record was not resumed");
    };
    let Preparation::NeedsInput(pending) = prepared.preparation else {
        panic!("second input remains pending");
    };
    assert_eq!(pending.build().unwrap().unresolved.len(), 1);
    assert_eq!(governor.status().reserved.scratch_bytes, 7);
    drop(pending);
    owner.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}
#[tokio::test]
async fn small_suspensions_use_small_record_memory_admission() {
    suspend_with_budget(24 << 20, 64 << 20).await;
}
#[tokio::test]
async fn suspension_cache_capacity_does_not_double_private_scratch_admission() {
    suspend_with_budget(128 << 20, 10).await;
}

#[tokio::test]
async fn conditional_saved_record_cleanup_preserves_newer_requests() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    fixture(root.path(), false);
    let state = host.path().join("state");
    let (owner, governor) = engine(state.clone());
    suspend_first(&owner, root.path()).await;
    let SavedBuildResume::Prepared(prior) = owner
        .resume_saved_build(root.path().to_path_buf())
        .await
        .unwrap()
    else {
        panic!("saved build is missing");
    };
    drop(prior.preparation);
    let mut next = request();
    let mut outputs = next.outputs.into_vec();
    outputs[0].artifact =
        PortableRelPath::parse("alternate.mrpack", PathSyntax::ArtifactName).unwrap();
    next.outputs = NonEmpty::new(outputs).unwrap();
    let Preparation::NeedsInput(pending) = owner
        .prepare(root.path().to_path_buf(), next)
        .await
        .unwrap()
    else {
        panic!("manual content missing");
    };
    assert!(owner.suspend_build(*pending).await.unwrap().replaced);
    let path = records(&state).pop().unwrap();
    let changed = fs::read(&path).unwrap();
    assert!(!owner.discard_saved_build(prior.saved).await.unwrap());
    assert_eq!(fs::read(&path).unwrap(), changed);
    let SavedBuildResume::Prepared(current) = owner
        .resume_saved_build(root.path().to_path_buf())
        .await
        .unwrap()
    else {
        panic!("newer saved build is missing");
    };
    drop(current.preparation);
    assert!(owner.discard_saved_build(current.saved).await.unwrap());
    assert!(matches!(
        owner
            .resume_saved_build(root.path().to_path_buf())
            .await
            .unwrap(),
        SavedBuildResume::Missing
    ));
    assert!(!root.path().join("dist").exists());
    owner.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}

#[tokio::test]
async fn saved_record_inspection_fits_eight_handle_allowance() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    fixture(root.path(), true);
    let (owner, _) = engine(host.path().join("state"));
    suspend_first(&owner, root.path()).await;
    let config = owner.config.clone();
    owner.shutdown().await;
    fs::write(root.path().join("empack.yml"), b"changed input").unwrap();
    let before = records(&config.state_root).pop().unwrap();
    let bytes = fs::read(&before).unwrap();
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 1,
        memory_bytes: 64 << 20,
        scratch_bytes: 64 << 20,
        open_files: 8,
    });
    let reader = Engine::new(config, governor.clone()).unwrap();
    assert!(matches!(
        reader
            .resume_saved_build(root.path().to_path_buf())
            .await
            .unwrap(),
        SavedBuildResume::Stale
    ));
    assert_eq!(fs::read(before).unwrap(), bytes);
    reader.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}

#[tokio::test]
async fn explicit_saved_recipe_cleanup_is_read_only_until_approved_and_refuses_changed_records() {
    use crate::application::{
        config::AppConfig,
        engine_host::clean,
        session_mocks::{MockCommandSession, MockConfigProvider, MockInvocationProvider},
    };
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    fixture(root.path(), true);
    let state = host.path().join("state");
    let (owner, governor) = engine(state.clone());
    assert!(
        owner
            .observe_saved_build(root.path().to_path_buf())
            .await
            .unwrap()
            .is_none()
    );
    assert!(!state.exists());
    suspend_first(&owner, root.path()).await;
    let record = records(&state).pop().unwrap();
    let valid = fs::read(&record).unwrap();
    let authored = documents(root.path());
    let session = |dry| {
        MockCommandSession::new()
            .with_invocation(
                MockInvocationProvider::new().with_current_dir(host.path().to_path_buf()),
            )
            .with_config(MockConfigProvider::new(AppConfig {
                workdir: Some(root.path().to_path_buf()),
                state_dir: Some(state.clone()),
                yes: true,
                dry_run: dry,
                ..Default::default()
            }))
    };
    for bytes in [&valid[..], b"invalid recipe".as_slice()] {
        fs::write(&record, bytes).unwrap();
        let before = saved_tree(host.path());
        clean(&session(true), &["continuation".into()])
            .await
            .unwrap();
        assert_eq!(saved_tree(host.path()), before);
        let observed = owner
            .observe_saved_build(root.path().to_path_buf())
            .await
            .unwrap()
            .unwrap();
        fs::write(&record, b"a different pending operation").unwrap();
        assert!(!owner.discard_saved_build(observed).await.unwrap());
        assert_eq!(fs::read(&record).unwrap(), b"a different pending operation");
        fs::write(&record, bytes).unwrap();
        clean(&session(false), &["continuation".into()])
            .await
            .unwrap();
        assert!(!record.exists());
        assert_eq!(documents(root.path()), authored);
    }
    // Stale recipes do not need the changed documents to decode successfully.
    fs::write(&record, &valid).unwrap();
    fs::write(root.path().join("empack.yml"), b"invalid current intent").unwrap();
    clean(&session(false), &["continuation".into()])
        .await
        .unwrap();
    assert!(!record.exists());
    assert_eq!(
        fs::read(root.path().join("empack.yml")).unwrap(),
        b"invalid current intent"
    );
    assert!(state.join("pending-builds/state.lock").exists());
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    owner.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn saved_recipe_cleanup_refuses_symlinks_and_cross_engine_observations() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    fixture(root.path(), true);
    let state = host.path().join("state");
    let (owner, _) = engine(state.clone());
    suspend_first(&owner, root.path()).await;
    let record = records(&state).pop().unwrap();
    let observed = owner
        .observe_saved_build(root.path().to_path_buf())
        .await
        .unwrap()
        .unwrap();
    let (other, _) = engine(state);
    assert!(other.discard_saved_build(observed).await.is_err());
    let outside = host.path().join("sentinel");
    fs::write(&outside, b"outside").unwrap();
    fs::remove_file(&record).unwrap();
    std::os::unix::fs::symlink(&outside, &record).unwrap();
    assert!(
        owner
            .observe_saved_build(root.path().to_path_buf())
            .await
            .is_err()
    );
    assert_eq!(fs::read(&outside).unwrap(), b"outside");
    owner.shutdown().await;
    other.shutdown().await;
}

fn saved_tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(path: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                visit(&entry.path(), files);
            } else {
                files.insert(entry.path(), fs::read(entry.path()).unwrap());
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, &mut files);
    files
}

#[tokio::test]
async fn ordinary_cache_hits_do_not_invalidate_saved_content_slots() {
    use crate::engine::content::cache::ContentCache;
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    fixture(root.path(), false);
    let (owner, governor) = engine(host.path().join("state"));
    // Suspension stores the already verified first slot while the second remains pending.
    suspend_first(&owner, root.path()).await;
    let cache =
        ContentCache::new(host.path().join("cache"), ContentStoreLimits::default()).unwrap();
    let runtime = OperationRuntime::<()>::new(governor.clone(), 1);
    let writer = cache.clone();
    let mut handle = runtime
        .start(move |mut scope| async move {
            let supplied = supplied("first", b"payload");
            writer
                .publish(
                    &mut scope,
                    supplied
                        .locked
                        .values()
                        .map(|file| file.content.clone())
                        .collect(),
                )
                .await
                .unwrap();
            Ok(())
        })
        .unwrap();
    assert!(matches!(
        &*handle.wait().await,
        OperationOutcome::Completed(())
    ));
    drop(handle);
    runtime.shutdown().await;
    let owner = owner.with_content_cache(cache);
    let resumed = match owner
        .resume_saved_build(root.path().to_owned())
        .await
        .unwrap()
    {
        SavedBuildResume::Prepared(resumed) => *resumed,
        _ => panic!("saved request must remain usable after an ordinary cache hit"),
    };
    let view = match &resumed.preparation {
        Preparation::Ready(value) => value.view(),
        Preparation::NeedsInput(value) => value.view(),
    };
    // Both fixture slots have identical asserted bytes; cache reuse can satisfy both.
    assert!(view.build().unwrap().content.is_empty());
    drop(resumed);
    owner.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}

#[tokio::test]
async fn retaining_new_input_requires_the_exact_saved_recipe_observation() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    fixture(root.path(), true);
    let state = host.path().join("state");
    let (owner, governor) = engine(state.clone());
    let receipt = suspend_first(&owner, root.path()).await;
    let observed = owner
        .observe_saved_build(root.path().to_owned())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(receipt.saved, observed);
    let SavedBuildResume::Prepared(resumed) = owner
        .resume_saved_build(root.path().to_owned())
        .await
        .unwrap()
    else {
        panic!("saved recipe")
    };
    assert_eq!(receipt.saved, resumed.saved);
    let Preparation::NeedsInput(pending) = resumed.preparation else {
        panic!("second input remains")
    };
    let record = records(&state).pop().unwrap();
    let mut changed = fs::read(&record).unwrap();
    changed.push(b' ');
    fs::write(&record, &changed).unwrap();
    assert!(
        owner
            .extend_saved_build(*pending, receipt.saved)
            .await
            .is_err()
    );
    assert_eq!(fs::read(&record).unwrap(), changed);
    let SavedBuildResume::Prepared(resumed) = owner
        .resume_saved_build(root.path().to_owned())
        .await
        .unwrap()
    else {
        panic!("changed recipe is still valid")
    };
    let Preparation::NeedsInput(pending) = resumed.preparation else {
        panic!("second input remains")
    };
    let retained = owner
        .extend_saved_build(*pending, resumed.saved)
        .await
        .unwrap();
    assert_eq!(
        retained.saved,
        owner
            .observe_saved_build(root.path().to_owned())
            .await
            .unwrap()
            .unwrap()
    );
    assert!(retained.replaced);
    owner.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}

#[tokio::test]
async fn native_release_batches_resume_exact_downloads_and_publish_with_other_consumers() {
    use crate::engine::{documents::DocumentCodec, mrpack::tests::explicitly_placed};
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    fixture(root.path(), true);
    let source = DocumentCodec
        .decode_intent(&fs::read(root.path().join("empack.yml")).unwrap(), "test")
        .unwrap();
    let project = DocumentCodec
        .decode_lock(
            &fs::read(root.path().join("empack.lock")).unwrap(),
            &source,
            "test",
        )
        .unwrap();
    let mut intent = project.intent().clone();
    intent.distribution.native = Some(empack_core::model::NativeDistributionIntent {
        pack_id: "resume.native".into(),
        java_major: 17,
        policies: BTreeMap::new(),
    });
    let project = explicitly_placed(intent, project.lock().clone());
    fs::write(
        root.path().join("empack.yml"),
        DocumentCodec.encode_intent(project.intent()).unwrap(),
    )
    .unwrap();
    fs::write(
        root.path().join("empack.lock"),
        DocumentCodec.encode_lock(&project).unwrap(),
    )
    .unwrap();
    let mut selected = request();
    selected.outputs = NonEmpty::new(vec![
        BuildOutput {
            target: Recipe::EMPACK_REFERENCES,
            artifact: crate::engine::api::tests::path("references.empack"),
        },
        BuildOutput {
            target: Recipe::EMPACK_BUNDLED,
            artifact: crate::engine::api::tests::path("bundled.empack"),
        },
        BuildOutput {
            target: Recipe::PRISM_BUNDLED,
            artifact: crate::engine::api::tests::path("prism.zip"),
        },
    ])
    .unwrap();
    let (owner, _) = engine(host.path().join("state"));
    let Preparation::NeedsInput(pending) = owner
        .prepare(
            root.path().to_path_buf(),
            selected.clone().with_content(supplied("first", b"payload")),
        )
        .await
        .unwrap()
    else {
        panic!("second exact file is missing")
    };
    owner.suspend_build(*pending).await.unwrap();
    owner.shutdown().await;
    assert!(!root.path().join("dist").exists());
    let (owner, governor) = engine(host.path().join("state"));
    let SavedBuildResume::Prepared(resumed) = owner
        .resume_saved_build(root.path().to_path_buf())
        .await
        .unwrap()
    else {
        panic!("missing continuation")
    };
    let Preparation::NeedsInput(pending) = resumed.preparation else {
        panic!("second file remains missing")
    };
    assert_eq!(
        pending.build().unwrap().outputs,
        selected.outputs.as_slice()
    );
    let Preparation::Ready(prepared) = owner
        .resume(*pending, supplied("second", b"payload"))
        .await
        .unwrap()
    else {
        panic!("exact bytes supplied")
    };
    let grant = ExecutionGrant {
        plan: prepared.view().plan(),
        network: NetworkPermission::Offline,
        run_installer: false,
        run_runtime: false,
        replacement: prepared.view().replacement(),
    };
    let mut handle = owner.start(prepared.authorize(grant).unwrap()).unwrap();
    let outcome = handle.wait().await;
    let OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Build(receipt))) =
        &*outcome
    else {
        match &*outcome {
            OperationOutcome::Completed(ExecutionOutcome::FailedBeforePublication(error)) => {
                panic!("{error:#}")
            }
            OperationOutcome::Failed(error) => panic!("{error}"),
            _ => panic!("native batch did not complete"),
        }
    };
    assert_eq!(receipt.artifacts.len(), 3);
    for (name, bundled) in [("references.empack", false), ("bundled.empack", true)] {
        let mut archive =
            zip::ZipArchive::new(fs::File::open(root.path().join("dist").join(name)).unwrap())
                .unwrap();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut archive.by_name("release.json").unwrap(), &mut bytes)
            .unwrap();
        let release = crate::engine::release::DecodedRelease::decode(&bytes).unwrap();
        assert_eq!(release.document().pack, "resume.native");
        assert!(
            release
                .document()
                .files
                .iter()
                .all(|file| file.asset.is_some() == bundled)
        );
        assert!(release.document().files.iter().all(|file| {
            file.assertions
                .iter()
                .any(|digest| digest.algorithm == "md5")
        }));
    }
    owner.release_completed(handle.id());
    drop((outcome, handle));
    owner.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}

#[tokio::test]
async fn native_server_projection_does_not_require_client_downloads() {
    server_projection_without_client_downloads(empack_core::distribution::Consumer::Empack).await;
}

#[tokio::test]
async fn modrinth_server_projection_does_not_require_client_downloads() {
    server_projection_without_client_downloads(empack_core::distribution::Consumer::Modrinth).await;
}

async fn server_projection_without_client_downloads(consumer: empack_core::distribution::Consumer) {
    use crate::engine::{documents::DocumentCodec, mrpack::tests::explicitly_placed};
    use empack_core::{
        distribution::{Consumer, Delivery},
        requirements::Environments,
    };
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    fixture(root.path(), true);
    let source = DocumentCodec
        .decode_intent(&fs::read(root.path().join("empack.yml")).unwrap(), "test")
        .unwrap();
    let project = DocumentCodec
        .decode_lock(
            &fs::read(root.path().join("empack.lock")).unwrap(),
            &source,
            "test",
        )
        .unwrap();
    let mut intent = project.intent().clone();
    if consumer == Consumer::Empack {
        intent.distribution.native = Some(empack_core::model::NativeDistributionIntent {
            pack_id: "server.projection".into(),
            java_major: 17,
            policies: BTreeMap::new(),
        });
    }
    let project = explicitly_placed(intent, project.lock().clone());
    fs::write(
        root.path().join("empack.yml"),
        DocumentCodec.encode_intent(project.intent()).unwrap(),
    )
    .unwrap();
    fs::write(
        root.path().join("empack.lock"),
        DocumentCodec.encode_lock(&project).unwrap(),
    )
    .unwrap();
    crate::engine::api::tests::put(
        root.path(),
        "overrides/server/config/server.txt",
        b"server config",
    );
    crate::engine::api::tests::put(
        root.path(),
        "overrides/client/config/client.txt",
        b"client config",
    );
    let (owner, governor) = engine(host.path().join("state"));
    let mut selected = request();
    let recipe = Recipe::new(consumer, Delivery::References, Environments::Server).unwrap();
    let artifact = if consumer == Consumer::Empack {
        "server.empack"
    } else {
        "server.mrpack"
    };
    selected.outputs = NonEmpty::new(vec![BuildOutput {
        target: recipe,
        artifact: crate::engine::api::tests::path(artifact),
    }])
    .unwrap();
    let Preparation::Ready(prepared) = owner
        .prepare(root.path().to_path_buf(), selected)
        .await
        .unwrap()
    else {
        panic!("client-only restricted downloads are not needed")
    };
    assert!(prepared.view().build().unwrap().content.is_empty());
    assert!(!prepared.view().needs_network());
    let grant = ExecutionGrant {
        plan: prepared.view().plan(),
        replacement: prepared.view().replacement(),
        network: NetworkPermission::Offline,
        run_installer: false,
        run_runtime: false,
    };
    let mut handle = owner.start(prepared.authorize(grant).unwrap()).unwrap();
    let outcome = handle.wait().await;
    let OperationOutcome::Completed(ExecutionOutcome::Completed(ExecutionReceipt::Build(receipt))) =
        &*outcome
    else {
        panic!("server release did not complete")
    };
    assert_eq!(receipt.artifacts[0].content.target(), recipe);
    let mut archive =
        zip::ZipArchive::new(fs::File::open(root.path().join("dist").join(artifact)).unwrap())
            .unwrap();
    if consumer == Consumer::Empack {
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut archive.by_name("release.json").unwrap(), &mut bytes)
            .unwrap();
        let release = crate::engine::release::DecodedRelease::decode(&bytes).unwrap();
        assert_eq!(release.document().files.len(), 1);
        assert_eq!(release.document().files[0].destination, "config/server.txt");
    } else {
        let index: serde_json::Value =
            serde_json::from_reader(archive.by_name("modrinth.index.json").unwrap()).unwrap();
        assert!(index["files"].as_array().unwrap().is_empty());
    }
    let files: Vec<_> = (0..archive.len())
        .filter_map(|index| {
            let member = archive.by_index(index).unwrap();
            (!member.is_dir()).then(|| member.name().to_owned())
        })
        .collect();
    assert_eq!(files.len(), 2, "only manifest and selected authored asset");
    let asset = files
        .iter()
        .find(|name| {
            if consumer == Consumer::Empack {
                name.starts_with("assets/")
            } else {
                name.as_str() == "server-overrides/config/server.txt"
            }
        })
        .unwrap();
    let mut content = Vec::new();
    std::io::Read::read_to_end(&mut archive.by_name(asset).unwrap(), &mut content).unwrap();
    assert_eq!(content, b"server config");
    owner.release_completed(handle.id());
    drop((outcome, handle));
    owner.shutdown().await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}
