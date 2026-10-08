//! Stream the captured resolution changes before adoption approval.
use crate::engine::api::{AdoptObservedPreview, AdoptionResolution};
use empack_core::{digest::ExpectedDigest, model::AcquisitionSpec};

pub(super) fn describe(view: &AdoptObservedPreview, mut emit: impl FnMut(String)) {
    for selection in &view.selections {
        emit(format!("Resolution change: {}", selection.key.as_str()));
        if let Some(before) = &selection.before {
            describe_resolution("Before", before, &mut emit);
        } else {
            emit("Before: no recorded resolution".into());
        }
        describe_resolution("After", &selection.after, &mut emit);
    }
}
fn describe_resolution(label: &str, value: &AdoptionResolution, emit: &mut impl FnMut(String)) {
    let dependency = &value.dependency;
    emit(format!(
        "{label}: {} ({:?}, {:?}); selected pin: {:?}",
        dependency.title, dependency.kind, dependency.identity, dependency.selected
    ));
    emit(format!(
        "{label} required dependencies: {}; evidence: {:?}",
        if value.required.is_empty() {
            "none recorded".into()
        } else {
            value
                .required
                .iter()
                .map(|key| key.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        },
        value.coverage
    ));
    for file in dependency.files.as_slice() {
        let slot = file.slot.as_str();
        emit(format!(
            "{label} file {slot}: expected size {:?}",
            file.expected.size
        ));
        if let Some(digests) = &file.expected.digests {
            for digest in digests.values() {
                emit(format!(
                    "{label} file {slot}: {:?} {}",
                    digest.algorithm(),
                    digest.hex()
                ));
            }
        } else {
            emit(format!(
                "{label} file {slot}: no independent digest assertion"
            ));
        }
        if let Some(id) = &file.expected.accepted_observation {
            emit(format!(
                "{label} file {slot}: accepted SHA256 observation {}",
                ExpectedDigest::Sha256(*id.bytes()).hex()
            ));
        }
        match &file.acquisition {
            AcquisitionSpec::Provider {
                pin,
                slot: role,
                alternatives,
            } => emit(format!(
                "{label} file {slot}: provider {:?}, role {}, origins {:?}",
                pin,
                role.as_str(),
                alternatives
            )),
            AcquisitionSpec::Url(origins) => emit(format!(
                "{label} file {slot}: origins {:?}",
                origins.as_slice()
            )),
            AcquisitionSpec::Local(path) => {
                emit(format!("{label} file {slot}: local {}", path.as_str()))
            }
            AcquisitionSpec::Embedded { archive, member } => emit(format!(
                "{label} file {slot}: archive {} member {}",
                archive.as_str(),
                member.as_str()
            )),
            // Freeform instructions and provenance are not acquisition identity, and may contain
            // locators that should not be expanded into automatic diagnostics.
            AcquisitionSpec::Manual { pin, .. } => {
                emit(format!("{label} file {slot}: manual selection {:?}", pin))
            }
        }
        for placement in file.placements.as_slice() {
            emit(format!(
                "{label} file {slot}: {:?}/{}; client {:?}; server {:?}",
                placement.layer,
                placement.destination.relative().as_str(),
                placement.requirements.client,
                placement.requirements.server
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::api::AdoptionSelection;
    use empack_core::{
        identity::{ModrinthProjectId, ProviderProjectId},
        model::*,
        path::InstallDestination,
        requirements::{Requirement, Requirements},
    };
    use std::collections::BTreeSet;

    #[test]
    fn resolution_details_expose_changed_pins_digests_requirements_and_edges() {
        let identity = ProviderProjectId::Modrinth(ModrinthProjectId::parse("Project1").unwrap());
        let dependency = LockedDependency {
            title: "Selected content".into(),
            kind: ContentKind::Mod,
            identity: ResolvedIdentity::Provider(identity.clone()),
            selected: None,
            files: NonEmpty::new(vec![ResolvedFile {
                slot: FileSlot::parse("main").unwrap(),
                acquisition: AcquisitionSpec::Manual {
                    pin: None,
                    instructions: "manual".into(),
                },
                expected: ExpectedContent {
                    digests: None,
                    size: Some(7),
                    accepted_observation: None,
                },
                provenance: Provenance {
                    source: "test".into(),
                    location: None,
                    declared_digests: None,
                    conversions: Vec::new(),
                },
                placements: NonEmpty::new(vec![Placement {
                    destination: InstallDestination::parse("mods/selected.jar").unwrap(),
                    layer: ContentLayer::Common,
                    requirements: Requirements {
                        client: Requirement::Required,
                        server: Requirement::Unsupported,
                    },
                }])
                .unwrap(),
            }])
            .unwrap(),
        };
        let mut before = AdoptionResolution {
            dependency,
            required: BTreeSet::new(),
            coverage: Coverage::Unknown,
        };
        before.dependency.selected = Some(ResolvedPin {
            project: identity.clone(),
            selection: identity.parse_pin("Version1").unwrap(),
        });
        let mut after = before.clone();
        after.dependency.selected = Some(ResolvedPin {
            project: identity.clone(),
            selection: identity.parse_pin("Version2").unwrap(),
        });
        after
            .required
            .insert(DependencyKey::parse("required-library").unwrap());
        after.coverage = Coverage::CompleteForSelection;
        let mut files = after.dependency.files.as_slice().to_vec();
        let expected =
            empack_core::digest::DigestSet::new(vec![ExpectedDigest::Sha256([7; 32])]).unwrap();
        files[0].expected.digests = Some(expected);
        files[0].expected.size = Some(27);
        let mut placements = files[0].placements.as_slice().to_vec();
        placements[0].requirements.server = Requirement::Required;
        files[0].placements = NonEmpty::new(placements).unwrap();
        files[0].acquisition = AcquisitionSpec::Manual {
            pin: after.dependency.selected.clone(),
            instructions: "https://example.invalid/?token=secret".into(),
        };
        after.dependency.files = NonEmpty::new(files).unwrap();
        let selection = AdoptionSelection {
            key: DependencyKey::parse("alias").unwrap(),
            before: Some(before.clone()),
            after: after.clone(),
        };
        let mut lines = Vec::new();
        describe_resolution("Before", selection.before.as_ref().unwrap(), &mut |line| {
            lines.push(line)
        });
        describe_resolution("After", &selection.after, &mut |line| lines.push(line));
        let text = lines.join("\n");
        for expected in [
            "Version1",
            "Version2",
            "Some(27)",
            "required-library",
            "Unknown",
            "CompleteForSelection",
            "server Required",
            &ExpectedDigest::Sha256([7; 32]).hex(),
        ] {
            assert!(text.contains(expected), "missing {expected}: {text}");
        }
        assert!(!text.contains("token=secret"));
        assert!(text.contains("Before file"));
        assert!(text.contains("After file"));
    }
}
