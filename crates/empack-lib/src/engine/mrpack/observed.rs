//! Retained backend content is an observed obligation, not invented authoring intent.
use super::*;
use crate::engine::{
    backend::{BackendDownload, BackendFile, ProviderObservation},
    content::SourceEvidencePolicy,
};
use empack_core::{
    digest::ExpectedDigest,
    requirements::{ChoiceKey, Environments, OptionalChoice},
};

#[derive(Debug, Clone)]
pub struct ObservedFileEvidence {
    pub metadata_path: PortableRelPath,
    pub provider: Option<ProviderObservation>,
    pub declared: ExpectedDigest,
    pub actual: ContentId,
    pub requirements: Requirements,
}
/// A backend record whose declared bytes have been verified. Original weak evidence is retained.
pub struct ObservedFile {
    pub(super) input: InventoryInput,
    pub(super) evidence: ObservedFileEvidence,
    pub(super) acquired: AcquiredBuildFile,
}
impl ObservedFile {
    pub fn verify(
        record: BackendFile,
        acquired: AcquiredBuildFile,
        choice: ChoiceKey,
        policy: SourceEvidencePolicy,
    ) -> Result<Self> {
        ensure!(
            acquired
                .content
                .observed_digests()
                .values()
                .contains(&record.digest),
            "Observed backend bytes do not match their declaration"
        );
        if policy == SourceEvidencePolicy::StrongSourceRequired {
            ensure!(
                matches!(
                    record.digest.algorithm(),
                    DigestAlgorithm::Sha256 | DigestAlgorithm::Sha512
                ),
                "Observed backend content has only weaker source evidence"
            );
        }
        let participation = match record.optional {
            Some(option) => Requirement::Optional(OptionalChoice {
                key: choice,
                default_enabled: option.default_enabled,
                description: option.description,
            }),
            None => Requirement::Required,
        };
        let requirements = Requirements {
            client: if record.environments == Environments::Server {
                Requirement::Unsupported
            } else {
                participation.clone()
            },
            server: if record.environments == Environments::Client {
                Requirement::Unsupported
            } else {
                participation
            },
        };
        let lease = acquired.content.lease();
        let representation = match record.download {
            BackendDownload::Url(url)
                if !acquired.permissions.readonly && !acquired.permissions.executable =>
            {
                crate::engine::documents::validate_download_url(&url)?;
                Representation::Download {
                    digests: acquired.content.observed_digests().clone(),
                    bytes: lease.len(),
                    urls: NonEmpty::new(vec![url])?,
                }
            }
            _ => Representation::Embedded {
                content: lease.id(),
                bytes: lease.len(),
                permissions: acquired.permissions,
            },
        };
        // The retained declaration, rather than computed export hashes, remains the source evidence.
        Ok(Self {
            input: InventoryInput {
                owner: ContentOwner::Source(format!("backend:{}", record.metadata_path.as_str())),
                destination: record.destination,
                layer: ContentLayer::Common,
                requirements: requirements.clone(),
                representation,
            },
            evidence: ObservedFileEvidence {
                metadata_path: record.metadata_path,
                provider: record.provider,
                declared: record.digest,
                actual: lease.id(),
                requirements,
            },
            acquired,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{build_file, project};
    use super::*;
    fn record(url: &str) -> BackendFile {
        let text = format!(
            r#"filename = "asset.zip"
side = "client"
[download]
url = "{url}"
hash-format = "md5"
hash = "321c3cf486ed509164edec1e1981fec8"
[option]
optional = true
default = true
description = "Observed option"
"#
        );
        BackendFile::parse(
            PortableRelPath::parse("extras/asset.pw.toml", PathSyntax::ProjectContent).unwrap(),
            text.as_bytes(),
        )
        .unwrap()
    }
    #[test]
    fn observed_optional_references_preserve_requirements_and_weaker_declarations() {
        let observed = ObservedFile::verify(
            record("https://example.com/asset.zip"),
            build_file(b"payload"),
            ChoiceKey::parse("observed-extra").unwrap(),
            SourceEvidencePolicy::Compatibility,
        )
        .unwrap();
        let plan = MrpackPlan::prepare_with_observed(
            &project(false, false),
            &BTreeMap::new(),
            vec![],
            vec![observed],
            OptionalConversion::AcknowledgedMetadataLoss,
        )
        .unwrap();
        assert_eq!(
            plan.observed()[0].declared.algorithm(),
            DigestAlgorithm::Md5
        );
        let index: Value = serde_json::from_slice(&plan.index).unwrap();
        let entry = index["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["path"] == "extras/asset.zip")
            .unwrap();
        assert_eq!(
            entry["env"],
            json!({"client":"optional","server":"unsupported"})
        );
        assert_eq!(entry["fileSize"], 7);
        assert!(entry["hashes"]["sha512"].is_string());
        assert_eq!(plan.conversions().len(), 1);
    }
    #[test]
    fn observed_content_rejects_mismatches_strong_policy_bypass_and_secret_references() {
        for (bytes, url, policy) in [
            (
                &b"changed"[..],
                "https://example.com/asset.zip",
                SourceEvidencePolicy::Compatibility,
            ),
            (
                &b"payload"[..],
                "https://example.com/asset.zip",
                SourceEvidencePolicy::StrongSourceRequired,
            ),
            (
                &b"payload"[..],
                "https://example.com/asset.zip?token=do-not-log",
                SourceEvidencePolicy::Compatibility,
            ),
        ] {
            let error = ObservedFile::verify(
                record(url),
                build_file(bytes),
                ChoiceKey::parse("observed-extra").unwrap(),
                policy,
            )
            .err()
            .unwrap();
            assert!(!format!("{error:#}").contains("do-not-log"));
        }
    }
}
