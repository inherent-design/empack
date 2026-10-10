//! Stable public classifications; human-readable error chains remain supplemental.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum DiagnosticCode {
    AuthorizationDenied,
    /// Signature/envelope authentication failed against enrolled publisher keys.
    PublisherAuthenticationFailed,
    /// Exact release payload bytes differ from the selected content address.
    ReleaseIdentityMismatch,
    /// The selected executable cannot satisfy the release's engine requirement.
    IncompatibleEngine,
    StaleSnapshot,
    DigestMismatch,
    SizeMismatch,
    InvalidDigest,
    UnsupportedConversion,
    PublicationConflict,
    RecoveryRequired,
    ExecutionUncertain,
    RuntimeRecoveryRequired,
    Interrupted,
    ResourceAdmission,
    AcquisitionFailed,
    OperationFailed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiagnosticPhase {
    Preparation,
    Authorization,
    Acquisition,
    Verification,
    Publication,
    Execution,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecoveryClassification {
    /// The diagnostic alone makes no claim about publication state.
    Unclassified,
    NotPublished,
    PartiallyCompleted,
    InspectRecovery,
    Required,
}
/// Values are optional when the failing boundary cannot establish them. This envelope
/// never copies an arbitrary error message or download locator into serializable evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{code:?} during {phase:?}")]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub phase: DiagnosticPhase,
    pub object: Option<String>,
    pub expected: Option<String>,
    pub observed: Option<String>,
    pub recovery: RecoveryClassification,
}
impl Diagnostic {
    pub fn new(code: DiagnosticCode, phase: DiagnosticPhase) -> Self {
        Self {
            code,
            phase,
            object: None,
            expected: None,
            observed: None,
            recovery: RecoveryClassification::Unclassified,
        }
    }
    /// Inspect typed causes only; message wording is never a classification input.
    pub fn from_error(error: &anyhow::Error, phase: DiagnosticPhase) -> Self {
        use DiagnosticCode::*;
        if let Some(value) = error.downcast_ref::<Self>() {
            return value.clone();
        }
        if error.is::<super::api::RuntimeRecoveryRequired>() {
            let mut diagnostic = Self::new(RuntimeRecoveryRequired, DiagnosticPhase::Execution);
            diagnostic.recovery = RecoveryClassification::Required;
            return diagnostic;
        }
        // Runtime admission keeps its typed cause even through the transparent runtime wrapper.
        let admission = error
            .downcast_ref::<super::resources::AdmissionError>()
            .or_else(
                || match error.downcast_ref::<super::runtime::RuntimeError>() {
                    Some(super::runtime::RuntimeError::Admission(cause)) => Some(cause),
                    _ => None,
                },
            );
        if let Some(admission) = admission {
            use super::resources::AdmissionError;
            let mut diagnostic = Self::new(ResourceAdmission, phase);
            let values = match *admission {
                AdmissionError::TooLarge {
                    resource,
                    requested,
                    maximum,
                } => Some((resource, requested, maximum)),
                AdmissionError::Busy {
                    resource,
                    requested,
                    available,
                } => Some((resource, requested, available)),
                _ => None,
            };
            if let Some((resource, requested, capacity)) = values {
                diagnostic.object = Some(resource.to_string());
                diagnostic.expected = Some(resource.quantity(capacity));
                diagnostic.observed = Some(resource.quantity(requested));
            }
            return diagnostic;
        }
        let code = if error.is::<crate::application::process_runtime::Interrupted>() {
            Interrupted
        } else if let Some(digest) = error.downcast_ref::<empack_core::digest::DigestError>() {
            if matches!(digest, empack_core::digest::DigestError::Mismatch(_)) {
                DigestMismatch
            } else {
                InvalidDigest
            }
        } else if error.is::<super::acquisition::TransferError>() {
            AcquisitionFailed
        } else if error.is::<super::publication::RecoveryRequired>() {
            RecoveryRequired
        } else {
            OperationFailed
        };
        let phase = if error.is::<super::acquisition::TransferError>() {
            DiagnosticPhase::Acquisition
        } else if error.is::<empack_core::digest::DigestError>() {
            DiagnosticPhase::Verification
        } else if error.is::<super::publication::RecoveryRequired>() {
            DiagnosticPhase::Publication
        } else {
            phase
        };
        Self::new(code, phase)
    }
}
impl super::api::ExecutionOutcome {
    /// Machine-readable classification with publication state taken from the owned outcome.
    pub fn diagnostic(&self) -> Option<Diagnostic> {
        use super::api::ExecutionOutcome::*;
        let (mut diagnostic, recovery) = match self {
            Completed(_) | NeedsInput(_) => return None,
            PartiallyCompleted { cause, .. } => (
                Diagnostic::from_error(cause, DiagnosticPhase::Execution),
                RecoveryClassification::PartiallyCompleted,
            ),
            FailedBeforePublication(cause) => (
                Diagnostic::from_error(cause, DiagnosticPhase::Execution),
                RecoveryClassification::NotPublished,
            ),
            InterruptedBeforePublication => (
                Diagnostic::new(DiagnosticCode::Interrupted, DiagnosticPhase::Execution),
                RecoveryClassification::NotPublished,
            ),
            ExecutionUncertain(cause) if cause.is::<super::api::RuntimeRecoveryRequired>() => (
                Diagnostic::from_error(cause, DiagnosticPhase::Execution),
                RecoveryClassification::Required,
            ),
            ExecutionUncertain(_) => (
                Diagnostic::new(
                    DiagnosticCode::ExecutionUncertain,
                    DiagnosticPhase::Publication,
                ),
                RecoveryClassification::InspectRecovery,
            ),
            RecoveryRequired { operation, cause } => {
                let mut diagnostic = Diagnostic::from_error(cause, DiagnosticPhase::Publication);
                if diagnostic.code == DiagnosticCode::OperationFailed {
                    diagnostic.code = DiagnosticCode::RecoveryRequired;
                }
                if diagnostic.object.is_none() {
                    diagnostic.object = Some(operation.clone());
                }
                (diagnostic, RecoveryClassification::Required)
            }
        };
        diagnostic.recovery = recovery;
        Some(diagnostic)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::api::ExecutionOutcome;
    #[test]
    fn runtime_retirement_has_a_distinct_machine_readable_recovery_route() {
        let cause = anyhow::Error::new(crate::application::process_runtime::Interrupted)
            .context(crate::engine::api::RuntimeRecoveryRequired);
        let diagnostic = ExecutionOutcome::ExecutionUncertain(cause)
            .diagnostic()
            .unwrap();
        assert_eq!(diagnostic.code, DiagnosticCode::RuntimeRecoveryRequired);
        assert_eq!(diagnostic.phase, DiagnosticPhase::Execution);
        assert_eq!(diagnostic.recovery, RecoveryClassification::Required);
    }
    #[test]
    fn admission_diagnostics_survive_runtime_and_context_wrappers() {
        use crate::engine::{
            resources::{AdmissionError, ResourceKind},
            runtime::RuntimeError,
        };
        for wrapped in [false, true] {
            for busy in [false, true] {
                let error = if busy {
                    AdmissionError::Busy {
                        resource: ResourceKind::MemoryBytes,
                        requested: 1152 << 20,
                        available: 512 << 20,
                    }
                } else {
                    AdmissionError::TooLarge {
                        resource: ResourceKind::MemoryBytes,
                        requested: 1152 << 20,
                        maximum: 512 << 20,
                    }
                };
                let cause = if wrapped {
                    anyhow::Error::new(RuntimeError::Admission(error))
                } else {
                    anyhow::Error::new(error)
                }
                .context("Cannot start installer");
                let outcome = ExecutionOutcome::FailedBeforePublication(cause);
                let diagnostic = outcome.diagnostic().unwrap();
                assert_eq!(diagnostic.code, DiagnosticCode::ResourceAdmission);
                assert_eq!(diagnostic.phase, DiagnosticPhase::Execution);
                assert_eq!(diagnostic.recovery, RecoveryClassification::NotPublished);
                assert_eq!(diagnostic.object.as_deref(), Some("estimated memory"));
                assert_eq!(
                    diagnostic.expected.as_deref(),
                    Some("512.0 MiB (536870912 bytes)")
                );
                assert_eq!(
                    diagnostic.observed.as_deref(),
                    Some("1152.0 MiB (1207959552 bytes)")
                );
            }
        }
    }

    #[test]
    fn classification_uses_causes_and_owned_outcome_not_message_text() {
        let error = anyhow::Error::new(empack_core::digest::DigestError::Mismatch(
            empack_core::digest::DigestAlgorithm::Sha256,
        ))
        .context("arbitrary wording");
        let diagnostic = ExecutionOutcome::FailedBeforePublication(error)
            .diagnostic()
            .unwrap();
        assert_eq!(diagnostic.code, DiagnosticCode::DigestMismatch);
        assert_eq!(diagnostic.recovery, RecoveryClassification::NotPublished);
        let misleading = anyhow::anyhow!("digest mismatch; https://host/?secret=hidden");
        let diagnostic = Diagnostic::from_error(&misleading, DiagnosticPhase::Execution);
        assert_eq!(diagnostic.code, DiagnosticCode::OperationFailed);
        assert!(
            !serde_json::to_string(&diagnostic)
                .unwrap()
                .contains("hidden")
        );
    }
    #[test]
    fn typed_transfer_phase_survives_execution_wrapping() {
        let cause = anyhow::Error::new(crate::engine::acquisition::TransferError::Server(503))
            .context("catalog acquisition");
        let diagnostic = ExecutionOutcome::FailedBeforePublication(cause)
            .diagnostic()
            .unwrap();
        assert_eq!(diagnostic.code, DiagnosticCode::AcquisitionFailed);
        assert_eq!(diagnostic.phase, DiagnosticPhase::Acquisition);
        assert_eq!(diagnostic.recovery, RecoveryClassification::NotPublished);
    }
    #[test]
    fn recovery_state_survives_a_specific_failure_code() {
        let cause = Diagnostic::new(
            DiagnosticCode::PublicationConflict,
            DiagnosticPhase::Publication,
        );
        let outcome = ExecutionOutcome::RecoveryRequired {
            operation: "op-example".into(),
            cause: cause.into(),
        };
        let diagnostic = outcome.diagnostic().unwrap();
        assert_eq!(diagnostic.code, DiagnosticCode::PublicationConflict);
        assert_eq!(diagnostic.recovery, RecoveryClassification::Required);
        assert_eq!(diagnostic.object.as_deref(), Some("op-example"));
        assert!(
            serde_json::to_string(&diagnostic)
                .unwrap()
                .contains("publication-conflict")
        );
    }
}
