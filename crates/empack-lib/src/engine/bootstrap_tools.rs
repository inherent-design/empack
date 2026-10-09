//! Exact reviewed installer assets. Pins are maintained by empack, not upstream signatures.
use super::{
    acquisition::{DownloadRequest, HttpAcquisition, TransferLimits},
    content::{AcquiredContent, InitialObservation, SourceEvidencePolicy},
    runtime::WorkScope,
};
use anyhow::{Result, ensure};
use empack_core::{
    digest::DigestSet,
    model::{ExpectedContent, NonEmpty},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallerArtifact {
    Bootstrap,
    Installer,
}
/// Immutable catalog facts for an exact bundled tool. Upstream release assets currently have
/// no published checksum; this pin identifies bytes reviewed and tested by empack maintainers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstallerRelease {
    pub artifact: InstallerArtifact,
    pub version: &'static str,
    pub filename: &'static str,
    pub url: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}
impl InstallerArtifact {
    pub fn release(self) -> InstallerRelease {
        match self {
            Self::Bootstrap => InstallerRelease {
                artifact: self,
                version: "v0.0.3",
                filename: "packwiz-installer-bootstrap.jar",
                url: "https://github.com/packwiz/packwiz-installer-bootstrap/releases/download/v0.0.3/packwiz-installer-bootstrap.jar",
                size: 98_989,
                sha256: "a8fbb24dc604278e97f4688e82d3d91a318b98efc08d5dbfcbcbcab6443d116c",
            },
            Self::Installer => InstallerRelease {
                artifact: self,
                version: "v0.5.14",
                filename: "packwiz-installer.jar",
                url: "https://github.com/packwiz/packwiz-installer/releases/download/v0.5.14/packwiz-installer.jar",
                size: 4_378_828,
                sha256: "c9f646908d340d84773948a9a7d98bc1dae250d35e1016dc6e2b8459760b5598",
            },
        }
    }
    pub fn download(self, mut limits: TransferLimits) -> Result<DownloadRequest> {
        let release = self.release();
        ensure!(
            release.size <= limits.file_bytes,
            "Installer exceeds configured file allowance"
        );
        limits.file_bytes = release.size;
        Ok(DownloadRequest {
            alternatives: NonEmpty::new(vec![release.url.to_owned()])?,
            expected: release.expected()?,
            limits,
            evidence: SourceEvidencePolicy::StrongSourceRequired,
            initial: InitialObservation::RequireEvidence,
        })
    }
}
impl InstallerRelease {
    pub fn expected(&self) -> Result<ExpectedContent> {
        Ok(ExpectedContent {
            digests: Some(DigestSet::parse([("sha256", self.sha256)])?),
            size: Some(self.size),
            accepted_observation: None,
        })
    }
}
/// Both exact assets are verified before this value exists. It grants no project mutation.
#[derive(Clone)]
pub struct InstallerAssets {
    bootstrap: AcquiredContent,
    installer: AcquiredContent,
}
impl InstallerAssets {
    /// Packaging tests substitute deterministic bytes; production construction always checks catalog pins.
    #[cfg(test)]
    pub(super) fn fixture() -> Self {
        let acquire = |bytes: &[u8]| {
            super::content::verify_stream(
                &mut &*bytes,
                &ExpectedContent {
                    digests: None,
                    size: None,
                    accepted_observation: None,
                },
                100,
                SourceEvidencePolicy::Compatibility,
                InitialObservation::Accepted,
                &crate::application::process_runtime::Cancellation::default(),
            )
            .unwrap()
        };
        Self {
            bootstrap: acquire(b"bootstrap fixture"),
            installer: acquire(b"installer fixture"),
        }
    }
    pub fn verify(bootstrap: AcquiredContent, installer: AcquiredContent) -> Result<Self> {
        for (kind, content) in [
            (InstallerArtifact::Bootstrap, &bootstrap),
            (InstallerArtifact::Installer, &installer),
        ] {
            let release = kind.release();
            ensure!(
                content.lease().len() == release.size,
                "Installer size differs from reviewed release"
            );
            release
                .expected()?
                .digests
                .expect("catalog digest")
                .check(content.observed_digests().values())?;
        }
        Ok(Self {
            bootstrap,
            installer,
        })
    }
    /// A failed second transfer drops the first private lease; no partial bundle is returned.
    pub async fn acquire(
        transport: &HttpAcquisition,
        scope: &mut WorkScope,
        limits: TransferLimits,
    ) -> Result<Self> {
        let bootstrap = transport
            .acquire(scope, InstallerArtifact::Bootstrap.download(limits)?)
            .await?;
        let installer = transport
            .acquire(scope, InstallerArtifact::Installer.download(limits)?)
            .await?;
        Self::verify(bootstrap, installer)
    }
    pub fn content(&self, artifact: InstallerArtifact) -> &AcquiredContent {
        match artifact {
            InstallerArtifact::Bootstrap => &self.bootstrap,
            InstallerArtifact::Installer => &self.installer,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{application::process_runtime::Cancellation, engine::content::verify_stream};
    #[test]
    fn catalog_requests_bind_sizes_and_mismatched_bytes_cannot_become_assets() {
        let request = InstallerArtifact::Installer
            .download(TransferLimits::default())
            .unwrap();
        assert_eq!(
            request.expected.size,
            Some(InstallerArtifact::Installer.release().size)
        );
        assert_eq!(request.limits.file_bytes, request.expected.size.unwrap());
        assert!(
            InstallerArtifact::Installer
                .download(TransferLimits {
                    file_bytes: 100,
                    ..TransferLimits::default()
                })
                .is_err()
        );
        let value = verify_stream(
            &mut &b"not an installer"[..],
            &ExpectedContent {
                digests: None,
                size: None,
                accepted_observation: None,
            },
            100,
            SourceEvidencePolicy::Compatibility,
            InitialObservation::Accepted,
            &Cancellation::default(),
        )
        .unwrap();
        assert!(InstallerAssets::verify(value.clone(), value).is_err());
        let forged = vec![0; InstallerArtifact::Bootstrap.release().size as usize];
        let content = verify_stream(
            &mut forged.as_slice(),
            &ExpectedContent {
                digests: None,
                size: None,
                accepted_observation: None,
            },
            forged.len() as u64,
            SourceEvidencePolicy::Compatibility,
            InitialObservation::Accepted,
            &Cancellation::default(),
        )
        .unwrap();
        let error = match InstallerAssets::verify(content.clone(), content) {
            Ok(_) => panic!("accepted wrong tool bytes"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("sha256 digest mismatch"));
    }
}
