//! Stable public classifications; human-readable error chains remain supplemental.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum DiagnosticCode {
    AuthorizationDenied,
    StaleSnapshot,
    DigestMismatch,
    SizeMismatch,
    InvalidDigest,
    UnsupportedConversion,
    PublicationConflict,
    RecoveryRequired,
    ExecutionUncertain,
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
        let code = if error.is::<crate::application::process_runtime::Interrupted>() {
            Interrupted
        } else if let Some(digest) = error.downcast_ref::<empack_core::digest::DigestError>() {
            if matches!(digest, empack_core::digest::DigestError::Mismatch(_)) {
                DigestMismatch
            } else {
                InvalidDigest
            }
        } else if error.is::<super::resources::AdmissionError>() {
            ResourceAdmission
        } else if error.is::<super::acquisition::TransferError>() {
            AcquisitionFailed
        } else if error.is::<super::publication::RecoveryRequired>() {
            RecoveryRequired
        } else {
            OperationFailed
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
