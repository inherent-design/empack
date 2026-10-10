//! Validated consumer recipes. Payload delivery does not determine update authority.
use crate::requirements::Environments;
use core::fmt;

/// Consumer of a generated distribution, independent of its archive encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Consumer {
    /// A Modrinth-format archive; hosting eligibility is checked separately.
    Modrinth,
    /// A CurseForge client manifest archive; upload eligibility is separate.
    CurseForge,
    /// A native Prism launcher instance.
    Prism,
    /// A dedicated server installation.
    Server,
    /// A portable native empack release.
    Empack,
}

/// How dependency bytes reach the destination, not whether it follows updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// Acquire dependencies from exact references; authored assets may be embedded.
    References,
    /// Include selected pack bytes where permitted; not an offline game installation.
    Bundled,
}

/// The single authority that may select later releases for an installation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum UpdateAuthority {
    /// No subscription or automatic version selection.
    #[default]
    Snapshot,
    /// A platform project/version association, established separately from export.
    Platform,
    /// An explicitly configured, authenticated native empack subscription.
    Empack,
}

/// An unsupported consumer recipe, rejected before acquisition or publication.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecipeError {
    /// The consumer does not support the selected client/server environment.
    Environment,
    /// The consumer cannot express the selected payload delivery policy.
    Delivery,
    /// The consumer does not support the selected update authority.
    UpdateAuthority,
}
impl fmt::Display for RecipeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Environment => "Consumer does not support the selected environment",
            Self::Delivery => "Consumer does not support the selected dependency delivery",
            Self::UpdateAuthority => "Consumer does not support the selected update authority",
        })
    }
}
impl core::error::Error for RecipeError {}

/// A representable policy combination, not proof of artifact or hosting validity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recipe {
    consumer: Consumer,
    delivery: Delivery,
    environments: Environments,
    authority: UpdateAuthority,
}
impl Recipe {
    /// Validate delivery and environment with snapshot updates as the default.
    ///
    /// This pure constructor does not inspect content, contact providers or install
    /// anything. Consumer projection must still validate individual obligations.
    pub fn new(
        consumer: Consumer,
        delivery: Delivery,
        environments: Environments,
    ) -> Result<Self, RecipeError> {
        match consumer {
            Consumer::CurseForge | Consumer::Prism if environments != Environments::Client => {
                return Err(RecipeError::Environment);
            }
            Consumer::Server if environments != Environments::Server => {
                return Err(RecipeError::Environment);
            }
            _ => {}
        }
        if matches!(consumer, Consumer::Modrinth | Consumer::CurseForge)
            && delivery != Delivery::References
        {
            return Err(RecipeError::Delivery);
        }
        Ok(Self {
            consumer,
            delivery,
            environments,
            authority: UpdateAuthority::Snapshot,
        })
    }

    /// Select an explicit update authority without creating a subscription.
    ///
    /// Platform associations and authenticated empack channels must be bound by
    /// the host before activation. Unsupported consumer/authority pairs fail here.
    pub fn with_update_authority(
        mut self,
        authority: UpdateAuthority,
    ) -> Result<Self, RecipeError> {
        let supported = match authority {
            UpdateAuthority::Snapshot => true,
            UpdateAuthority::Platform => {
                matches!(self.consumer, Consumer::Modrinth | Consumer::CurseForge)
            }
            UpdateAuthority::Empack => {
                matches!(
                    self.consumer,
                    Consumer::Prism | Consumer::Server | Consumer::Empack
                )
            }
        };
        if !supported {
            return Err(RecipeError::UpdateAuthority);
        }
        self.authority = authority;
        Ok(self)
    }

    /// Consumer whose adapter must verify the resulting artifact.
    pub fn consumer(&self) -> Consumer {
        self.consumer
    }
    /// Dependency delivery, independent of subscription policy.
    pub fn delivery(&self) -> Delivery {
        self.delivery
    }
    /// Environments projected into the distribution.
    pub fn environments(&self) -> Environments {
        self.environments
    }
    /// Update authority selected explicitly or defaulted to snapshot.
    pub fn update_authority(&self) -> UpdateAuthority {
        self.authority
    }
}
