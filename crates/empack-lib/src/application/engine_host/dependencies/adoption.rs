//! Stream the captured resolution changes before adoption approval.
use crate::application::labels;
use crate::engine::api::{AdoptObservedPreview, AdoptionResolution};
use empack_core::{digest::ExpectedDigest, model::AcquisitionSpec};

pub(super) fn describe(view: &AdoptObservedPreview, mut emit: impl FnMut(String)) {
    for selection in &view.selections {
        emit(format!("Dependency record: {}", selection.key.as_str()));
        if let Some(before) = &selection.before {
            describe_resolution("Before", before, &mut emit);
        } else {
            emit("Before: not recorded".into());
        }
        describe_resolution("After", &selection.after, &mut emit);
    }
}
fn describe_resolution(label: &str, value: &AdoptionResolution, emit: &mut impl FnMut(String)) {
    let dependency = &value.dependency;
    emit(format!(
        "{label}: {} ({}, {})",
        dependency.title,
        labels::content(dependency.kind),
        super::identity_label(&dependency.identity)
    ));
    emit(format!(
        "{label} version: {}",
        dependency
            .selected
            .as_ref()
            .map(labels::pin)
            .unwrap_or_else(|| "not recorded".into())
    ));
    emit(format!(
        "{label} required dependencies: {}",
        if value.required.is_empty() {
            if value.coverage == empack_core::model::Coverage::CompleteForSelection {
                "none declared".into()
            } else {
                "none recorded".into()
            }
        } else {
            value
                .required
                .iter()
                .map(|key| key.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        }
    ));
    emit(format!(
        "{label} dependency information: {}",
        labels::coverage(value.coverage)
    ));
    for file in dependency.files.as_slice() {
        let slot = file.slot.as_str();
        emit(format!(
            "{label} file {slot}: {}",
            file.expected
                .size
                .map(|size| format!("expected {size} bytes"))
                .unwrap_or_else(|| "size not declared".into())
        ));
        if let Some(digests) = &file.expected.digests {
            for digest in digests.values() {
                emit(format!(
                    "{label} file {slot}: declared {} {}",
                    digest.algorithm().name(),
                    digest.hex()
                ));
            }
        } else {
            emit(format!("{label} file {slot}: no source checksum declared"));
        }
        if let Some(id) = &file.expected.accepted_observation {
            emit(format!(
                "{label} file {slot}: accepted SHA-256 {}",
                ExpectedDigest::Sha256(*id.bytes()).hex()
            ));
        }
        match &file.acquisition {
            AcquisitionSpec::Provider {
                pin,
                slot: role,
                alternatives,
            } => {
                emit(format!(
                    "{label} file {slot}: provider {}, role {}",
                    labels::pin(pin),
                    role.as_str()
                ));
                if !alternatives.is_empty() {
                    describe_locations(label, slot, alternatives, emit);
                }
            }
            AcquisitionSpec::ProviderArchiveMember { archive, member } => emit(format!(
                "{label} file {slot}: provider archive {}, role {}, member {}",
                labels::pin(&archive.pin),
                archive.slot.as_str(),
                member.as_str()
            )),
            AcquisitionSpec::Url(origins) => {
                describe_locations(label, slot, origins.as_slice(), emit)
            }
            AcquisitionSpec::Local(path) => {
                emit(format!("{label} file {slot}: local {}", path.as_str()))
            }
            AcquisitionSpec::Embedded { archive, member } => emit(format!(
                "{label} file {slot}: archive {} member {}",
                archive.as_str(),
                member.as_str()
            )),
            // Instructions and provenance can contain credentials or signed locators.
            AcquisitionSpec::Manual { pin, .. } => emit(format!(
                "{label} file {slot}: manual download{}",
                pin.as_ref()
                    .map(|pin| format!(" for {}", labels::pin(pin)))
                    .unwrap_or_default()
            )),
        }
        for placement in file.placements.as_slice() {
            emit(format!(
                "{label} file {slot}: layer {}, path {}; client {}; server {}",
                labels::layer(placement.layer),
                placement.destination.relative().as_str(),
                labels::requirement(&placement.requirements.client),
                labels::requirement(&placement.requirements.server)
            ));
        }
    }
}

fn describe_locations(
    label: &str,
    slot: &str,
    locations: &[String],
    emit: &mut impl FnMut(String),
) {
    let hosts = locations
        .iter()
        .map(|location| {
            reqwest::Url::parse(location)
                .map(|url| url.origin().ascii_serialization())
                .unwrap_or_else(|_| "unavailable".into())
        })
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join(", ");
    emit(format!(
        "{label} file {slot}: download hosts {hosts} (full URLs hidden)"
    ));
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
            incompatibilities: Vec::new(),
            source_selector: None,
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
            "expected 27 bytes",
            "required-library",
            "dependency information: not recorded",
            "dependency information: complete for this version",
            "server required",
            &ExpectedDigest::Sha256([7; 32]).hex(),
        ] {
            assert!(text.contains(expected), "missing {expected}: {text}");
        }
        assert!(!text.contains("token=secret"));
        assert!(text.contains("Before file"));
        assert!(text.contains("After file"));
    }
    #[test]
    fn download_locations_hide_credentials_paths_and_signed_queries() {
        let urls = vec![
            "https://username:password@cdn.example.invalid/private-file?token=secret#fragment"
                .into(),
            "https://cdn.example.invalid/other?private=hidden".into(),
        ];
        let mut lines = Vec::new();
        describe_locations("After", "main", &urls, &mut |line| lines.push(line));
        assert_eq!(
            lines,
            ["After file main: download hosts https://cdn.example.invalid (full URLs hidden)"]
        );
        assert_eq!(urls.len(), 2);
        assert!(urls[0].contains("token=secret"));
    }
}
