//! Private extraction evidence binds member observations to a verified container.
use super::*;
use empack_core::model::ProviderArchiveSource;

#[derive(Clone)]
pub(in crate::engine) struct ArchiveEvidence {
    original: ExpectedContent,
}
#[derive(Clone)]
pub(super) struct MemberEvidence {
    archive: ArchiveEvidence,
    member: PortableRelPath,
}
impl ArchiveEvidence {
    pub(in crate::engine) fn captured(content: &AcquiredContent) -> Self {
        Self {
            original: ExpectedContent {
                digests: match content.evidence() {
                    IntegrityEvidence::MatchedExpected { expected, .. } => Some(expected.clone()),
                    IntegrityEvidence::ObservedOnly { .. } => None,
                },
                size: Some(content.lease().len()),
                accepted_observation: Some(content.lease().id()),
            },
        }
    }
    /// A strong member assertion can stand alone; otherwise extraction needs a strongly verified
    /// container. The resulting member still has observed-only evidence, never a fabricated digest.
    pub(in crate::engine) fn member_policy(
        &self,
        expected: &ExpectedContent,
        policy: SourceEvidencePolicy,
    ) -> Result<SourceEvidencePolicy> {
        if policy == SourceEvidencePolicy::StrongSourceRequired
            && validate_expectation(expected, u64::MAX, policy, InitialObservation::Accepted)
                .is_err()
        {
            validate_expectation(
                &self.original,
                u64::MAX,
                policy,
                InitialObservation::RequireEvidence,
            )?;
            Ok(SourceEvidencePolicy::Compatibility)
        } else {
            Ok(policy)
        }
    }
    /// Called by the bounded ZIP decoder only after it verifies this exact regular-file entry.
    pub(in crate::engine) fn bind(
        &self,
        mut content: AcquiredContent,
        member: PortableRelPath,
    ) -> AcquiredContent {
        content.archive_member = Some(Arc::new(MemberEvidence {
            archive: self.clone(),
            member,
        }));
        content
    }
}
impl AcquiredContent {
    /// Strong archive policy needs the private extraction witness, not merely a matching member
    /// cache address or a source declaration copied into an arbitrary member's metadata.
    pub(in crate::engine) fn provider_member_policy(
        &self,
        archive: &ProviderArchiveSource,
        member: &PortableRelPath,
        policy: SourceEvidencePolicy,
    ) -> Result<SourceEvidencePolicy> {
        if policy == SourceEvidencePolicy::Compatibility {
            return Ok(policy);
        }
        validate_expectation(
            &archive.expected,
            u64::MAX,
            policy,
            InitialObservation::RequireEvidence,
        )?;
        let proof = self
            .archive_member
            .as_ref()
            .context("Strong world-member verification requires its verified source archive")?;
        let actual = &proof.archive.original;
        ensure!(
            &proof.member == member
                && actual.digests == archive.expected.digests
                && archive
                    .expected
                    .size
                    .is_none_or(|size| Some(size) == actual.size)
                && archive
                    .expected
                    .accepted_observation
                    .as_ref()
                    .is_none_or(|id| Some(id) == actual.accepted_observation.as_ref()),
            "World member extraction evidence differs from its original archive"
        );
        Ok(SourceEvidencePolicy::Compatibility)
    }
}
