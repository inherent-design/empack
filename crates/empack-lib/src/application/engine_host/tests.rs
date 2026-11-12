use super::*;
use crate::test_support::file_bytes as snapshot;
use crate::{
    application::{
        Commands,
        cli::Cli,
        commands::execute_command_with_session,
        session_mocks::{MockCommandSession, MockConfigProvider, MockInteractiveProvider},
    },
    engine::{
        publication::{
            PublicationPoint, Publisher,
            tests::{interrupt_publication, prepare},
        },
        snapshot::ProjectReadRoot,
    },
};
use clap::Parser;
use std::{collections::BTreeMap, fs};

#[test]
fn recovery_parsing_and_state_selection_have_one_invocation_root() {
    let cli = Cli::try_parse_from([
        "empack",
        "--state-dir",
        "host/state",
        "recover",
        "restore",
        "--operation",
        "exact",
    ])
    .unwrap();
    assert!(
        matches!(cli.command, Some(Commands::Recover { action: CliRecoveryAction::Restore, operation: Some(ref id) }) if id == "exact")
    );
    let root = tempfile::tempdir().unwrap();
    assert_eq!(
        state_root(&cli.config, root.path()).unwrap(),
        root.path().join("host/state")
    );
    let cli = Cli::try_parse_from(["empack", "recover"]).unwrap();
    assert!(matches!(
        cli.command,
        Some(Commands::Recover {
            action: CliRecoveryAction::Inspect,
            ..
        })
    ));
    assert!(
        !Commands::Recover {
            action: CliRecoveryAction::Inspect,
            operation: None
        }
        .requires_modpack()
    );
    assert!(Cli::try_parse_from(["empack", "recover", "delete"]).is_err());
}
#[tokio::test]
async fn recovery_dispatch_inspects_previews_declines_and_recovers_native_publication() {
    for action in [CliRecoveryAction::Finish, CliRecoveryAction::Restore] {
        let project = tempfile::tempdir().unwrap();
        let host = tempfile::tempdir().unwrap();
        let state = host.path().join("state");
        fs::write(project.path().join("empack.yml"), b"old intent").unwrap();
        let root = ProjectReadRoot::open(project.path()).unwrap();
        let publisher = Publisher::open(&state).unwrap();
        assert!(
            interrupt_publication(
                &publisher,
                &root,
                prepare(&root),
                PublicationPoint::TargetChanged
            )
            .is_err()
        );
        let status = publisher.inspect_recovery(&root).unwrap().unwrap();
        let config = AppConfig {
            workdir: Some(project.path().to_path_buf()),
            cache_dir: Some(state.with_file_name("cache")),
            state_dir: Some(state),
            ..Default::default()
        };
        let before_project = snapshot(project.path());
        let before_host = snapshot(host.path());
        for mode in [
            "inspect",
            "preview",
            "decline",
            "headless",
            "wrong-operation",
        ] {
            let mut config = config.clone();
            config.dry_run = mode == "preview";
            let session = MockCommandSession::new()
                .with_config(MockConfigProvider::new(config))
                .with_interactive(MockInteractiveProvider::new().with_yes_mode(mode == "headless"));
            let result = execute_command_with_session(
                Commands::Recover {
                    action: if mode == "inspect" {
                        CliRecoveryAction::Inspect
                    } else {
                        action
                    },
                    operation: Some(if mode == "wrong-operation" {
                        "wrong".into()
                    } else {
                        status.operation.clone()
                    }),
                },
                &session,
            )
            .await;
            assert_eq!(
                result.is_err(),
                matches!(mode, "headless" | "wrong-operation"),
                "{mode}: {result:?}"
            );
            if mode == "headless" {
                let error = result.unwrap_err();
                assert_eq!(
                    crate::application::classify_error(&error),
                    crate::application::EmpackExitCode::Usage
                );
                let rendered = format!("{error:#}");
                assert!(rendered.contains("requires --yes"), "{rendered}");
                assert!(rendered.contains("Recovery inspection ("), "{rendered}");
                assert!(
                    rendered.contains(project.path().to_str().unwrap()),
                    "{rendered}"
                );
                assert!(
                    rendered.contains(host.path().to_str().unwrap()),
                    "{rendered}"
                );
                assert!(session.interactive_provider.get_confirm_calls().is_empty());
            }
            assert_eq!(snapshot(project.path()), before_project);
            assert_eq!(snapshot(host.path()), before_host);
        }
        let mut config = config;
        config.yes = true;
        let session = MockCommandSession::new().with_config(MockConfigProvider::new(config));
        execute_command_with_session(
            Commands::Recover {
                action,
                operation: Some(status.operation),
            },
            &session,
        )
        .await
        .unwrap();
        assert!(publisher.inspect_recovery(&root).unwrap().is_none());
        assert_eq!(
            fs::read(project.path().join("empack.yml")).unwrap(),
            if action == CliRecoveryAction::Finish {
                b"new intent"
            } else {
                b"old intent"
            }
        );
        assert_eq!(
            project.path().join("pack/config/new.txt").exists(),
            action == CliRecoveryAction::Finish
        );
    }
}

#[test]
fn recovery_preview_output_child() {
    let Some(project) = std::env::var_os("EMPACK_TEST_RECOVERY_PROJECT") else {
        return;
    };
    let state = std::env::var_os("EMPACK_TEST_RECOVERY_STATE").unwrap();
    let action = std::env::var("EMPACK_TEST_RECOVERY_ACTION").unwrap();
    let cli = Cli::try_parse_from([
        std::ffi::OsString::from("empack"),
        "--workdir".into(),
        project,
        "--state-dir".into(),
        state,
        "--dry-run".into(),
        "recover".into(),
        action.into(),
    ])
    .unwrap();
    let session = MockCommandSession::new().with_config(MockConfigProvider::new(cli.config));
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(execute_command_with_session(cli.command.unwrap(), &session))
        .unwrap();
}
#[test]
fn recovery_preview_displays_each_managed_path_and_change() {
    for (action, point, expected) in [
        (
            "finish",
            PublicationPoint::IntentDurable,
            vec!["replace empack.yml", "create pack/config/new.txt"],
        ),
        (
            "restore",
            PublicationPoint::Verified,
            vec!["replace empack.yml", "remove pack/config/new.txt"],
        ),
    ] {
        let project = tempfile::tempdir().unwrap();
        let host = tempfile::tempdir().unwrap();
        let state = host.path().join("state");
        fs::write(project.path().join("empack.yml"), b"old intent").unwrap();
        let root = ProjectReadRoot::open(project.path()).unwrap();
        let publisher = Publisher::open(&state).unwrap();
        assert!(interrupt_publication(&publisher, &root, prepare(&root), point).is_err());
        let project_before = snapshot(project.path());
        let host_before = snapshot(host.path());
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "application::engine_host::tests::recovery_preview_output_child",
                "--nocapture",
            ])
            .env("EMPACK_TEST_RECOVERY_PROJECT", project.path())
            .env("EMPACK_TEST_RECOVERY_STATE", &state)
            .env("EMPACK_TEST_RECOVERY_ACTION", action)
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.status.success(), "{text}");
        for change in expected {
            assert!(text.contains(change), "Missing {change}: {text}");
        }
        assert_eq!(snapshot(project.path()), project_before);
        assert_eq!(snapshot(host.path()), host_before);
    }
}

#[test]
fn reported_execution_failure_retains_typed_source_after_registry_release() {
    let outcome = std::sync::Arc::new(OperationOutcome::Completed(
        ExecutionOutcome::FailedBeforePublication(
            std::io::Error::from(std::io::ErrorKind::PermissionDenied).into(),
        ),
    ));
    let error = retained_failure(outcome.clone(), "Addition was not applied".into());
    drop(outcome);
    assert!(error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|cause| cause.kind() == std::io::ErrorKind::PermissionDenied)
    }));
    let diagnostic = error
        .downcast_ref::<crate::engine::diagnostics::Diagnostic>()
        .unwrap();
    assert_eq!(
        diagnostic.recovery,
        crate::engine::diagnostics::RecoveryClassification::NotPublished
    );
    assert!(format!("{error:#}").contains("Addition was not applied"));
}

#[test]
fn cli_budget_admits_installer_with_retained_build_inputs() {
    let governor = governor(&AppConfig::default());
    let resources = operation_resources();
    let prepared = governor.try_admit(resources.prepared).unwrap();
    let acquired = governor.try_admit(resources.acquired).unwrap();
    let (work, retained) = installer_execution().reservations().unwrap();
    let mut installer = governor.try_admit(work).unwrap();
    // The successful installer retains its captured runtime while assembly runs.
    let output = installer.split(retained).unwrap();
    drop(installer);
    let assembly = governor.try_admit(resources.assembly).unwrap();
    drop((assembly, output, acquired, prepared));
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}

#[tokio::test]
async fn capacity_failure_retains_host_diagnostics_and_other_owners_until_retry() {
    use crate::application::session_mocks::MockInvocationProvider;
    use crate::engine::{
        api::{BuildOutput, BuildRequest},
        content::SourceEvidencePolicy,
        diagnostics::{Diagnostic, DiagnosticCode, RecoveryClassification},
        documents::DocumentCodec,
        mrpack::OptionalConversion,
        resources::{AdmissionError, ResourceKind},
        templates::TemplateOptions,
    };
    use empack_core::{
        distribution::Recipe,
        inventory::OptionalPolicy,
        model::{
            DistributionArchive, NonEmpty, ResolutionLock, ResolvedProject, RuntimeResolution,
        },
        path::{PathSyntax, PortableRelPath},
    };

    let project = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let author = b"schema: 6\npack: {id: budget.fixture, name: Budget fixture, version: '1'}\nruntime: {minecraft: '1.21.1', loader: vanilla, java: {preferred: 21}}\ndependencies: {}\n";
    let codec = DocumentCodec;
    let intent = codec.decode_intent(author, "empack.yml").unwrap();
    let lock = ResolutionLock {
        acceptable_versions: intent.intent().runtime.acceptable_versions.clone(),
        intent_revision: intent.semantic_revision(),
        resolver: "budget-fixture".into(),
        dependencies: BTreeMap::new(),
        required_edges: BTreeMap::new(),
        coverage: BTreeMap::new(),
        runtime: RuntimeResolution {
            minecraft: intent.intent().runtime.minecraft.clone(),
            loader: intent.intent().runtime.loader,
            loader_version: None,
        },
    };
    let revision = intent.semantic_revision();
    let resolved = ResolvedProject::validate(intent.intent().clone(), lock, revision).unwrap();
    let lock = codec.encode_lock(&resolved).unwrap();
    fs::write(project.path().join("empack.yml"), author).unwrap();
    fs::write(project.path().join("empack.lock"), &lock).unwrap();
    fs::create_dir_all(project.path().join("pack/config")).unwrap();
    fs::write(
        project.path().join("pack/config/value.txt"),
        b"author config",
    )
    .unwrap();
    fs::create_dir_all(project.path().join("dist")).unwrap();
    fs::write(project.path().join("dist/result.mrpack"), b"prior artifact").unwrap();
    fs::write(project.path().join("dist/keep.zip"), b"unrelated artifact").unwrap();
    fs::create_dir(project.path().join("empty author directory")).unwrap();
    fs::write(host.path().join("neighbor.txt"), b"unrelated host data").unwrap();
    let before = snapshot(project.path());
    let host_before = snapshot(host.path());
    let config = AppConfig {
        state_dir: Some(host.path().join("state")),
        cache_dir: Some(host.path().join("cache")),
        ..Default::default()
    };
    let session = MockCommandSession::new()
        .with_config(MockConfigProvider::new(config.clone()))
        .with_invocation(MockInvocationProvider::new().with_current_dir(project.path().to_owned()));
    let governor = ResourceGovernor::new(ResourceRequest {
        jobs: 4,
        memory_bytes: 256 << 20,
        scratch_bytes: 1 << 30,
        open_files: 1024,
    });
    let engine = engine_with_governor(&config, project.path(), governor.clone()).unwrap();
    let request = BuildRequest {
        clean: false,
        outputs: NonEmpty::new(vec![BuildOutput {
            archive: DistributionArchive::Zip,
            target: Recipe::MODRINTH,
            artifact: PortableRelPath::parse("result.mrpack", PathSyntax::ArtifactName).unwrap(),
        }])
        .unwrap(),
        optional: OptionalPolicy::Preserve,
        mrpack_optional: OptionalConversion::RejectMetadataLoss,
        templates: TemplateOptions::default(),
        evidence: SourceEvidencePolicy::Compatibility,
    };
    let Preparation::Ready(prepared) = engine
        .prepare(project.path().to_path_buf(), request.clone())
        .await
        .unwrap()
    else {
        panic!("local build unexpectedly needs input");
    };
    let other = governor
        .try_admit(ResourceRequest {
            memory_bytes: 192 << 20,
            ..Default::default()
        })
        .unwrap();
    let error = execute_approved(&session, &engine, prepared, "Build", |_| {
        panic!("capacity refusal cannot produce a completed receipt")
    })
    .await
    .unwrap_err();
    let diagnostic = error.downcast_ref::<Diagnostic>().unwrap();
    assert_eq!(diagnostic.code, DiagnosticCode::ResourceAdmission);
    assert_eq!(diagnostic.recovery, RecoveryClassification::NotPublished);
    assert_eq!(diagnostic.object.as_deref(), Some("estimated memory"));
    assert_eq!(diagnostic.expected.as_deref(), Some("0 bytes"));
    assert_eq!(
        diagnostic.observed.as_deref(),
        Some("64.0 MiB (67108864 bytes)")
    );
    assert!(error.chain().any(|cause| matches!(
        cause.downcast_ref::<RuntimeError>(),
        Some(RuntimeError::Admission(AdmissionError::Busy {
            resource: ResourceKind::MemoryBytes,
            requested: 67_108_864,
            available: 0,
        }))
    )));
    let rendered = format!("{error:#}");
    assert!(rendered.contains("Build was not applied"), "{rendered}");
    assert!(
        rendered.contains("requested 64.0 MiB (67108864 bytes); currently available 0 bytes"),
        "{rendered}"
    );
    assert_eq!(
        crate::application::classify_error(&error),
        crate::application::EmpackExitCode::General
    );
    assert_eq!(snapshot(project.path()), before);
    assert_eq!(snapshot(host.path()), host_before);
    assert!(!host.path().join("state").exists());
    assert!(!host.path().join("cache").exists());
    assert!(project.path().join("empty author directory").is_dir());
    assert_eq!(
        governor.status().reserved,
        ResourceRequest {
            memory_bytes: 192 << 20,
            ..Default::default()
        },
        "a failed operation must not release another owner's reservation"
    );
    drop(other);
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    // Keep the previous error alive: its diagnostic ownership must not keep the
    // registry result or failed preparation charged against the next operation.
    let Preparation::Ready(prepared) = engine
        .prepare(project.path().to_path_buf(), request)
        .await
        .unwrap()
    else {
        panic!("released capacity did not permit a fresh build");
    };
    execute_approved(&session, &engine, prepared, "Build", |receipt| {
        let ExecutionReceipt::Build(receipt) = receipt else {
            panic!("unexpected execution receipt");
        };
        assert_eq!(receipt.artifacts.len(), 1);
        Ok("Budgeted build completed".into())
    })
    .await
    .unwrap();
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    let mut archive =
        zip::ZipArchive::new(fs::File::open(project.path().join("dist/result.mrpack")).unwrap())
            .unwrap();
    assert_eq!(archive.len(), 4);
    let mut names: Vec<_> = archive
        .file_names()
        .map(|name| name.unwrap().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "modrinth.index.json",
            "overrides/",
            "overrides/config/",
            "overrides/config/value.txt"
        ]
    );
    for (name, directory) in [
        ("modrinth.index.json", false),
        ("overrides/", true),
        ("overrides/config/", true),
        ("overrides/config/value.txt", false),
    ] {
        let entry = archive.by_name(name).unwrap();
        assert_eq!(entry.is_dir(), directory, "{name}");
        if directory {
            assert_eq!(entry.size(), 0, "directory has payload: {name}");
        }
    }
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(
        &mut archive.by_name("overrides/config/value.txt").unwrap(),
        &mut bytes,
    )
    .unwrap();
    assert_eq!(bytes, b"author config");
    assert_eq!(fs::read(project.path().join("empack.yml")).unwrap(), author);
    assert_eq!(fs::read(project.path().join("empack.lock")).unwrap(), lock);
    assert_eq!(
        fs::read(project.path().join("dist/keep.zip")).unwrap(),
        b"unrelated artifact"
    );
    assert_eq!(
        fs::read(host.path().join("neighbor.txt")).unwrap(),
        b"unrelated host data"
    );
    assert!(project.path().join("empty author directory").is_dir());
    assert!(format!("{error:#}").contains("currently available 0 bytes"));
    engine.shutdown().await;
}

#[test]
fn retained_recovery_coordinates_preserve_the_original_terminal_result() {
    use crate::engine::publication::RecoveryLocation;
    let root = tempfile::tempdir().unwrap();
    let location = RecoveryLocation {
        project: root.path().join("actual project"),
        state: root.path().join("actual state"),
    };
    let outcome = std::sync::Arc::new(OperationOutcome::Completed(
        ExecutionOutcome::RecoveryRequired {
            operation: "op-fixture".into(),
            cause: location.attach(super::super::process_runtime::Interrupted.into()),
        },
    ));
    let error = retained_failure(outcome.clone(), "Build requires recovery".into());
    drop(outcome);
    let error = super::super::recovery_guidance::annotate(error);
    let text = format!("{error:#}");
    assert!(text.contains("Recovery inspection ("), "{text}");
    assert!(text.contains(location.project.to_str().unwrap()), "{text}");
    assert!(text.contains(location.state.to_str().unwrap()), "{text}");
    assert!(text.contains("Operation interrupted"), "{text}");
    assert_eq!(
        crate::application::classify_error(&error),
        crate::application::EmpackExitCode::General
    );
}

#[test]
fn retained_uncertain_runtime_preserves_recovery_and_typed_cause_after_registry_release() {
    use crate::engine::{
        diagnostics::{Diagnostic, DiagnosticCode, RecoveryClassification},
        publication::RecoveryLocation,
    };
    for interrupted in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let location = RecoveryLocation {
            project: root.path().join("runtime instance"),
            state: root.path().join("runtime state"),
        };
        let cause: anyhow::Error = if interrupted {
            super::super::process_runtime::Interrupted.into()
        } else {
            std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "fixture termination denied",
            )
            .into()
        };
        let outcome =
            std::sync::Arc::new(OperationOutcome::Completed(
                ExecutionOutcome::ExecutionUncertain(location.attach(cause.context(
                    "Runtime retirement is unconfirmed; inspect recovery before retrying",
                ))),
            ));
        let error = retained_failure(
            outcome.clone(),
            "Instance launch outcome is uncertain; inspect recovery before retrying".into(),
        );
        drop(outcome);
        let diagnostic = error.downcast_ref::<Diagnostic>().unwrap();
        assert_eq!(diagnostic.code, DiagnosticCode::ExecutionUncertain);
        assert_eq!(diagnostic.recovery, RecoveryClassification::InspectRecovery);
        if interrupted {
            assert!(
                error
                    .chain()
                    .any(|cause| cause.is::<super::super::process_runtime::Interrupted>())
            );
        } else {
            assert_eq!(
                error
                    .chain()
                    .find_map(|cause| cause.downcast_ref::<std::io::Error>())
                    .unwrap()
                    .kind(),
                std::io::ErrorKind::PermissionDenied
            );
        }
        assert_eq!(
            crate::application::classify_error(&error),
            crate::application::EmpackExitCode::General
        );
        let text = format!("{:#}", super::super::recovery_guidance::annotate(error));
        assert!(
            text.contains("Instance launch outcome is uncertain"),
            "{text}"
        );
        assert!(text.contains("Runtime retirement is unconfirmed"), "{text}");
        assert!(text.contains("Recovery inspection ("), "{text}");
        assert!(text.contains(location.project.to_str().unwrap()), "{text}");
        assert!(text.contains(location.state.to_str().unwrap()), "{text}");
        assert!(
            text.contains(if interrupted {
                "Operation interrupted"
            } else {
                "fixture termination denied"
            }),
            "{text}"
        );
    }
}

#[test]
fn launch_failure_summary_preserves_runtime_phase_and_cause() {
    for phase in [
        "Runtime did not start",
        "Runtime exited unsuccessfully",
        "Runtime retired, but terminal restoration failed",
    ] {
        let cause = anyhow::Error::new(std::io::Error::other("fixture failure")).context(phase);
        let outcome = std::sync::Arc::new(OperationOutcome::Completed(
            ExecutionOutcome::FailedBeforePublication(cause),
        ));
        let error = RetainedExecutionError {
            message: failure_summary("Instance launch", true),
            outcome,
        };
        assert_eq!(
            format!("{:#}", anyhow::Error::new(error)),
            format!("Instance launch failed: {phase}: fixture failure")
        );
    }
    assert_eq!(
        failure_summary("Addition", false),
        "Addition was not applied"
    );
}

#[tokio::test]
async fn scoped_service_cancellation_preserves_completed_effects_and_final_result() {
    use crate::application::{
        process_runtime::{Cancellation, Interrupted},
        session::LiveProcessProvider,
    };
    for outcome in ["partial", "complete", "unrelated", "invalidated"] {
        let root = tempfile::tempdir().unwrap();
        let selected = root.path().join("selected");
        let neighbor = root.path().join("neighbor");
        fs::write(&selected, b"discard this").unwrap();
        fs::write(&neighbor, b"keep this").unwrap();
        let cancellation = Cancellation::default();
        let mut session = MockCommandSession::new();
        session.process_provider =
            LiveProcessProvider::new().with_cancellation(cancellation.clone());
        let governor = ResourceGovernor::new(ResourceRequest {
            jobs: 1,
            open_files: 2,
            ..Default::default()
        });
        let removed = selected.clone();
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            scoped(&session, governor.clone(), move |mut scope| async move {
                let work = scope.spawn_blocking(
                    ResourceRequest {
                        jobs: 1,
                        open_files: 1,
                        ..Default::default()
                    },
                    ResourceRequest::default(),
                    move |_| fs::remove_file(removed),
                )?;
                scope.accept(work.wait().await?)?.transpose()?;
                // The host is interrupted only after a native effect was established.
                // Wait for propagation instead of guessing when its polling tick runs.
                cancellation.cancel();
                scope.cancellation().cancelled().await;
                match outcome {
                    "partial" => Err(anyhow::Error::new(Interrupted)
                        .context("Cleanup interrupted; completed scope: selected (12 bytes)")),
                    "complete" => Ok("completed selected cleanup"),
                    "invalidated" => Err(anyhow::Error::new(RuntimeError::StaleResult)
                        .context("Service input was invalidated during cancellation")),
                    "unrelated" => Err(anyhow::Error::new(std::io::Error::from(
                        std::io::ErrorKind::PermissionDenied,
                    ))
                    .context("Later cleanup scope failed; selected already removed")),
                    _ => unreachable!(),
                }
            }),
        )
        .await
        .expect("host must retire its service promptly");
        assert!(!selected.exists());
        assert_eq!(fs::read(neighbor).unwrap(), b"keep this");
        match outcome {
            "partial" => {
                let error = result.unwrap_err();
                assert!(error.is::<Interrupted>());
                assert_eq!(
                    format!("{error:#}"),
                    "Cleanup interrupted; completed scope: selected (12 bytes): Operation interrupted"
                );
            }
            "complete" => assert_eq!(result.unwrap(), "completed selected cleanup"),
            "invalidated" => {
                let error = result.unwrap_err();
                assert!(error.is::<Interrupted>());
                assert!(matches!(
                    error.downcast_ref::<RuntimeError>(),
                    Some(RuntimeError::StaleResult)
                ));
                assert!(
                    format!("{error:#}")
                        .contains("Service input was invalidated during cancellation")
                );
            }
            "unrelated" => {
                let error = result.unwrap_err();
                assert!(!error.is::<Interrupted>());
                assert_eq!(
                    error.downcast_ref::<std::io::Error>().unwrap().kind(),
                    std::io::ErrorKind::PermissionDenied
                );
                assert!(
                    format!("{error:#}")
                        .contains("Later cleanup scope failed; selected already removed")
                );
                assert_eq!(
                    crate::application::exit::classify_error(&error),
                    crate::application::exit::EmpackExitCode::General
                );
            }
            _ => unreachable!(),
        }
        assert_eq!(governor.status().reserved, ResourceRequest::default());
    }
}

#[tokio::test]
async fn scoped_service_precancellation_never_starts_work() {
    use crate::application::{process_runtime::Cancellation, session::LiveProcessProvider};
    let cancellation = Cancellation::default();
    cancellation.cancel();
    let mut session = MockCommandSession::new();
    session.process_provider = LiveProcessProvider::new().with_cancellation(cancellation);
    let root = tempfile::tempdir().unwrap();
    let selected = root.path().join("selected");
    fs::write(&selected, b"keep this").unwrap();
    let target = selected.clone();
    let error = scoped(
        &session,
        ResourceGovernor::new(ResourceRequest {
            jobs: 1,
            open_files: 2,
            ..Default::default()
        }),
        move |_| async move {
            fs::remove_file(target)?;
            Ok(())
        },
    )
    .await
    .unwrap_err();
    assert!(error.is::<crate::application::process_runtime::Interrupted>());
    assert_eq!(fs::read(selected).unwrap(), b"keep this");
}
