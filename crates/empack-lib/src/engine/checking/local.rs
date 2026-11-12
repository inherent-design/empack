//! Local checks use the normal document, snapshot and content verifiers without staging writes.
use super::*;
use crate::engine::{
    archive_source::ZipContentSource,
    artifacts::ArchiveLimits,
    content::{InitialObservation, SourceEvidencePolicy, validate_expectation},
    documents::{DocumentCodec, MAX_DOCUMENT_BYTES},
    layout::ProjectLayout,
    snapshot::Observation,
};
use empack_core::files::ManagedPath;

struct FileCheck {
    key: DependencyKey,
    slot: FileSlot,
    expected: ExpectedContent,
    member: Option<PortableRelPath>,
    required: bool,
}

pub(super) fn inspect(
    request: &CheckRequest,
    cancel: &Cancellation,
    progress: &impl Fn(WorkProgress),
) -> Result<Inspection> {
    let root = ProjectReadRoot::open(&request.project)?;
    let mut result = Inspection {
        report: CheckReport {
            schema: 1,
            scope: request.scope,
            pack: None,
            author_schema: None,
            authoring: None,
            lock: "missing",
            verified_files: 0,
            references: 0,
            findings: vec![],
            complete: false,
        },
        failure: None,
        root,
        documents: vec![],
        project: None,
        workspace: None,
        observed: BTreeMap::new(),
    };
    let recovery = RecoveryReader::new(request.state.clone());
    let mut documents = BTreeMap::new();
    {
        let _guard = recovery.enter(&result.root)?;
        for name in ["empack.yml", "empack.bindings.yml", "empack.lock"] {
            cancel.check()?;
            progress(WorkProgress::CheckingProjectFile {
                path: name.into(),
                index: 0,
                total: 0,
            });
            let captured = (|| {
                let path = PortableRelPath::parse(name, PathSyntax::ProjectContent)?;
                let snapshot = result.root.capture(
                    &[path],
                    SnapshotLimits {
                        entries: 1,
                        depth: 1,
                        file_bytes: MAX_DOCUMENT_BYTES as u64,
                        total_bytes: MAX_DOCUMENT_BYTES as u64,
                    },
                    cancel,
                )?;
                let bytes =
                    crate::engine::project::read_document(&result.root, &snapshot, name, cancel)?;
                Ok::<_, anyhow::Error>((snapshot, bytes))
            })();
            match captured {
                Ok((snapshot, bytes)) => {
                    result.documents.push(snapshot);
                    documents.insert(name, bytes);
                }
                Err(error) => {
                    if name == "empack.lock" {
                        result.report.lock = "unreadable";
                    }
                    result.failed(name, error)
                }
            }
        }
    }
    let source = match (
        documents.get("empack.yml"),
        documents.get("empack.bindings.yml"),
    ) {
        (Some(Some(author)), Some(bindings)) => {
            match DocumentCodec.decode_authoring(author, bindings.as_deref(), "empack.yml") {
                Ok(source) => {
                    result.report.authoring = Some(DocumentCodec.inspect_authoring(&source, None));
                    result.report.pack = Some(source.intent().metadata.name.clone());
                    result.report.author_schema = Some(crate::engine::documents::INTENT_SCHEMA);
                    result.passed("Authoring", "Author files are valid");
                    Some(source)
                }
                Err(error) => {
                    result.failed("Authoring", error);
                    None
                }
            }
        }
        (Some(None), _) => {
            result.failed(
                "empack.yml",
                anyhow::anyhow!("Author file is missing: empack.yml"),
            );
            None
        }
        _ => None,
    };
    if let Some(bytes) = documents.get("empack.lock") {
        match bytes {
            None if request.scope == CheckScope::Documents => result.finding(
                "Lock",
                CheckState::Unverified,
                "No lock present; recorded dependencies were not checked",
            ),
            None => result.failed(
                "Lock",
                anyhow::anyhow!(
                    "Current lock required; resolve the author project with empack sync"
                ),
            ),
            Some(bytes) => match DocumentCodec.decode_prior_lock(bytes, "empack.lock") {
                Err(error) => {
                    result.report.lock = "invalid";
                    if let Some(authoring) = &mut result.report.authoring {
                        authoring.lock_status = "invalid";
                        authoring.lock_reason = Some(format!("{error:#}"));
                    }
                    result.failed("empack.lock", error);
                }
                Ok(lock) => {
                    result.report.lock = "unbound";
                    if let Some(source) = &source {
                        result.report.authoring =
                            Some(DocumentCodec.inspect_authoring(source, Some(&lock)));
                        match lock.bind(source) {
                            Ok(project) => {
                                result.report.lock = "current";
                                result.passed("Lock", "Author files and lock agree");
                                result.project = Some(project);
                            }
                            Err(error) => {
                                result.report.lock = "stale";
                                result.failed("Lock", error);
                            }
                        }
                    } else {
                        result.finding(
                            "Lock",
                            CheckState::Unverified,
                            "Lock parsed, but authoring is unavailable for comparison",
                        );
                    }
                }
            },
        }
    }
    if result.report.lock == "unreadable"
        && let Some(authoring) = &mut result.report.authoring
    {
        authoring.lock_status = "unreadable";
        authoring.lock_reason = Some("Lock could not be captured/read".into());
    }
    if request.scope == CheckScope::Documents {
        return Ok(result);
    }
    let Some(project) = result.project.clone() else {
        return Ok(result);
    };
    result.passed(
        "Recorded relationships",
        "Recorded runtime, required relationships, placements and incompatibilities are consistent",
    );
    for (key, dependency) in &project.lock().dependencies {
        if project.lock().coverage.get(key) != Some(&Coverage::CompleteForSelection) {
            if matches!(dependency.identity, ResolvedIdentity::Provider(_))
                && request.scope == CheckScope::Recorded
            {
                result.failed(key.as_str(), anyhow::anyhow!("Dependency metadata is incomplete for {}; use check --online for fresh exact metadata",key.as_str()));
            } else {
                result.finding(
                    key.as_str(),
                    CheckState::Unverified,
                    "Complete dependency metadata is not recorded",
                );
            }
        }
    }
    let workspace = match ProjectReader::new(recovery).capture_build(
        &request.project,
        &[],
        request.files,
        cancel,
    ) {
        Ok(workspace) => workspace,
        Err(error) => {
            result.failed("Local source files", error);
            return Ok(result);
        }
    };
    let mut paths = BTreeMap::<PortableRelPath, Vec<FileCheck>>::new();
    for (key, dependency) in &project.lock().dependencies {
        for file in dependency.files.as_slice() {
            let spec = |member, required| FileCheck {
                key: key.clone(),
                slot: file.slot.clone(),
                expected: file.expected.clone(),
                member,
                required,
            };
            match &file.acquisition {
                AcquisitionSpec::Local(path) => paths
                    .entry(path.clone())
                    .or_default()
                    .push(spec(None, true)),
                AcquisitionSpec::Embedded { archive, member } => paths
                    .entry(archive.clone())
                    .or_default()
                    .push(spec(Some(member.clone()), true)),
                _ => {}
            }
            for placement in file.placements.as_slice() {
                let path = ProjectLayout::path(&ManagedPath::Content {
                    layer: placement.layer,
                    path: placement.destination.relative().clone(),
                })?;
                paths.entry(path).or_default().push(spec(None, false));
            }
        }
    }
    let total = paths.len();
    let mut decoded = 0u64;
    let mut verified_paths = std::collections::BTreeSet::new();
    for (index, (path, checks)) in paths.into_iter().enumerate() {
        cancel.check()?;
        progress(WorkProgress::CheckingProjectFile {
            path: path.as_str().into(),
            index: index + 1,
            total,
        });
        let observations = workspace.observations();
        let absent = matches!(observations.entries().get(&path), Some(Observation::Absent))
            || observations.entries().iter().any(|(ancestor, value)| {
                matches!(value, Observation::Absent)
                    && path
                        .as_str()
                        .starts_with(&format!("{}/", ancestor.as_str()))
            })
            || (!observations.entries().contains_key(&path)
                && observations.membership_covers(&path)?);
        if absent {
            if checks.iter().any(|check| check.required) {
                result.failed(
                    path.as_str(),
                    anyhow::anyhow!("Required local source is missing: {}", path.as_str()),
                );
            } else {
                result.report.references += 1;
                result.finding(
                    path.as_str(),
                    CheckState::Reference,
                    "Referenced bytes are not materialized locally",
                );
            }
            continue;
        }
        let mut archive: Option<ZipContentSource> = None;
        for check in checks {
            let subject = format!(
                "{} / {} / {}{}",
                check.key.as_str(),
                check.slot.as_str(),
                path.as_str(),
                check
                    .member
                    .as_ref()
                    .map(|p| format!("!{}", p.as_str()))
                    .unwrap_or_default()
            );
            let verified = (|| {
                if let Some(member) = &check.member {
                    if archive.is_none() {
                        archive = Some(ZipContentSource::open_captured(
                            &workspace,
                            &path,
                            ArchiveLimits {
                                file_bytes: request.files.file_bytes,
                                total_bytes: request.files.total_bytes.saturating_sub(decoded),
                                entries: request.files.entries,
                                depth: request.files.depth,
                                ..Default::default()
                            },
                            cancel,
                        )?);
                    }
                    let archive = archive.as_mut().unwrap();
                    let before = archive.inspected_bytes();
                    let verified = archive.verify_member(member, &check.expected, cancel);
                    decoded = decoded
                        .checked_add(archive.inspected_bytes() - before)
                        .context("Checked-byte count overflow")?;
                    ensure!(
                        decoded <= request.files.total_bytes,
                        "Checked content byte limit exceeded"
                    );
                    verified.map(|digests| CheckedContent {
                        digests,
                        bytes: archive.inspected_bytes() - before,
                    })
                } else {
                    validate_expectation(
                        &check.expected,
                        request.files.file_bytes,
                        SourceEvidencePolicy::Compatibility,
                        InitialObservation::RequireEvidence,
                    )?;
                    let digests = workspace.verify_file(&path, &check.expected, cancel)?;
                    let Some(Observation::File(file)) =
                        workspace.observations().entries().get(&path)
                    else {
                        unreachable!("verified captured file")
                    };
                    Ok(CheckedContent {
                        digests,
                        bytes: file.bytes,
                    })
                }
            })();
            match verified {
                Ok(digests) => {
                    if verified_paths.insert((path.clone(), check.member.clone())) {
                        result.report.verified_files += 1;
                    }
                    result.observed.insert((check.key, check.slot), digests);
                    result.passed(subject, "Content matches its recorded assertions");
                }
                Err(error) => result.failed(subject, error),
            }
        }
    }
    result.workspace = Some(workspace);
    Ok(result)
}
