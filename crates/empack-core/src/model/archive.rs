//! An archive source assertion is not a digest declaration for each extracted member.
use super::*;
use crate::path::PathSyntax;

/// Original provider-owned archive, retained separately from member content observations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderArchiveSource {
    /// Exact owning provider selection.
    pub pin: ResolvedPin,
    /// Provider file role (usually its archive filename), not the extracted member role.
    pub slot: FileSlot,
    /// Original archive assertions, including their original weaker algorithms.
    pub expected: ExpectedContent,
    /// Stable credential-free download alternatives; adapters may refresh transient locators.
    pub alternatives: Vec<String>,
}
pub(super) fn validate_members(dependency: &LockedDependency) -> Result<(), ModelError> {
    let mut source = None;
    let mut prefix = None;
    let mut members = BTreeSet::new();
    let mut count = 0;
    for file in dependency.files.as_slice() {
        let AcquisitionSpec::ProviderArchiveMember { archive, member } = &file.acquisition else {
            continue;
        };
        count += 1;
        archive.pin.validate()?;
        if dependency.kind != ContentKind::World
            || dependency.selected.as_ref() != Some(&archive.pin)
        {
            return Err(invalid(
                "World archive selection differs from its owning dependency",
            ));
        }
        if archive.expected.digests.is_none() || archive.expected.size == Some(0) {
            return Err(invalid(
                "Provider world archive lacks original source assertions",
            ));
        }
        if file.expected.digests.is_some()
            || file.expected.accepted_observation.is_none()
            || file.provenance.declared_digests.is_some()
        {
            return Err(invalid(
                "World members retain observations; archive digests belong to the archive",
            ));
        }
        let relative = PortableRelPath::parse(file.slot.as_str(), PathSyntax::ArchiveMember)
            .map_err(|_| invalid("World member role is not a relative member path"))?;
        let parent = member
            .as_str()
            .strip_suffix(relative.as_str())
            .filter(|prefix| prefix.is_empty() || prefix.ends_with('/'))
            .ok_or_else(|| invalid("World member path differs from its relative role"))?;
        if source.is_some_and(|previous| previous != archive)
            || prefix.is_some_and(|previous| previous != parent)
        {
            return Err(invalid(
                "World members disagree about archive identity, assertions or root",
            ));
        }
        if !members.insert(member) {
            return Err(invalid("World archive member is repeated"));
        }
        source = Some(archive);
        prefix = Some(parent);
    }
    if count > 0 {
        if count != dependency.files.as_slice().len() {
            return Err(invalid(
                "Provider world cannot mix archive members and opaque files",
            ));
        }
        let level = dependency
            .files
            .as_slice()
            .iter()
            .find(|file| file.slot.as_str() == "level.dat")
            .ok_or_else(|| invalid("World archive has no root level.dat"))?;
        if level.expected.size.is_none_or(|size| size == 0) {
            return Err(invalid(
                "World root level.dat must have a known nonzero size",
            ));
        }
    }
    Ok(())
}
pub(super) fn validate_placements(
    kind: ContentKind,
    roots: &NonEmpty<Placement>,
    selected: &LockedDependency,
) -> Result<(), ModelError> {
    if kind != ContentKind::World {
        return Err(invalid("Archive-root placement requires world content"));
    }
    for file in selected.files.as_slice() {
        if !matches!(
            file.acquisition,
            AcquisitionSpec::ProviderArchiveMember { .. }
        ) {
            return Err(invalid(
                "Archive-root placement requires interpreted provider members",
            ));
        }
        let expected: Vec<_> = roots
            .as_slice()
            .iter()
            .map(|root| {
                let mut value = root.clone();
                value.destination = InstallDestination::parse(&alloc::format!(
                    "{}/{}",
                    root.destination.relative().as_str(),
                    file.slot.as_str()
                ))
                .map_err(|_| invalid("Invalid world member destination"))?;
                Ok(value)
            })
            .collect::<Result<_, ModelError>>()?;
        if expected.len() != file.placements.as_slice().len()
            || expected.iter().any(|value| {
                file.placements
                    .as_slice()
                    .iter()
                    .filter(|actual| *actual == value)
                    .count()
                    != 1
            })
        {
            return Err(invalid(
                "World member placement differs from its requested root",
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_automatic(
    folder: &str,
    selected: &LockedDependency,
) -> Result<(), ModelError> {
    let first = &selected.files.as_slice()[0];
    let suffix = alloc::format!("/{}", first.slot.as_str());
    let roots = first
        .placements
        .as_slice()
        .iter()
        .map(|placement| {
            let base = placement
                .destination
                .relative()
                .as_str()
                .strip_suffix(&suffix)
                .ok_or_else(|| invalid("World member has no destination root"))?;
            if placement.layer != ContentLayer::Common
                || base.rsplit_once('/').map(|(parent, _)| parent) != Some(folder)
            {
                return Err(invalid(
                    "World root does not satisfy the automatic content directory",
                ));
            }
            let mut root = placement.clone();
            root.destination = InstallDestination::parse(base)
                .map_err(|_| invalid("Invalid automatic world root"))?;
            Ok(root)
        })
        .collect::<Result<Vec<_>, ModelError>>()?;
    validate_placements(ContentKind::World, &NonEmpty::new(roots)?, selected)
}
