use crate::primitives::ConfigError;
use anyhow::Error;
use std::process::ExitCode as ProcessExitCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EmpackExitCode {
    Success = 0,
    General = 1,
    Usage = 2,
    Network = 3,
    NotFound = 4,
    Interrupted = 130,
}

impl EmpackExitCode {
    pub fn as_i32(self) -> i32 {
        self as i32
    }

    pub fn as_process_exit_code(self) -> ProcessExitCode {
        ProcessExitCode::from(self as u8)
    }
}

pub fn classify_error(error: &Error) -> EmpackExitCode {
    if find_chain_error::<super::process_runtime::Interrupted>(error).is_some() {
        return EmpackExitCode::Interrupted;
    }
    if matches!(
        find_chain_error::<crate::engine::runtime::RuntimeError>(error),
        Some(crate::engine::runtime::RuntimeError::Cancelled)
    ) {
        return EmpackExitCode::Interrupted;
    }
    if find_chain_error::<super::cli::CommandInputRequired>(error).is_some()
        || find_chain_error::<crate::engine::documents::InvalidDocument>(error).is_some()
        || find_chain_error::<crate::engine::project::ProjectDocumentsError>(error).is_some()
        || find_chain_error::<empack_core::model::ModelError>(error).is_some()
        || find_chain_error::<empack_core::distribution::RecipeError>(error).is_some()
        || find_chain_error::<empack_core::path::PathError>(error).is_some()
        || find_chain_error::<empack_core::identity::IdentityError>(error).is_some()
    {
        return EmpackExitCode::Usage;
    }
    if let Some(catalog) = find_chain_error::<crate::engine::providers::CatalogError>(error) {
        use crate::engine::providers::CatalogError;
        return match catalog {
            CatalogError::NotFound | CatalogError::NoCompatibleSelection => {
                EmpackExitCode::NotFound
            }
            CatalogError::InvalidSelector
            | CatalogError::Limit
            | CatalogError::Unauthorized
            | CatalogError::ContentKindMismatch
            | CatalogError::UnsupportedKind
            | CatalogError::Ambiguous => EmpackExitCode::Usage,
            CatalogError::Deadline
            | CatalogError::RateLimited
            | CatalogError::Server(_)
            | CatalogError::Status(_)
            | CatalogError::Network
            | CatalogError::Redirect
            | CatalogError::InvalidRecord
            | CatalogError::IncompleteLookup
            | CatalogError::Identity => EmpackExitCode::Network,
        };
    }
    if let Some(selection) = find_chain_error::<crate::engine::removal::SelectionError>(error) {
        return match selection {
            crate::engine::removal::SelectionError::Missing(_) => EmpackExitCode::NotFound,
            crate::engine::removal::SelectionError::Ambiguous { .. } => EmpackExitCode::Usage,
        };
    }
    if let Some(transfer) = find_chain_error::<crate::engine::acquisition::TransferError>(error) {
        use crate::engine::acquisition::TransferError;
        return match transfer {
            TransferError::InvalidLocator
            | TransferError::ByteLimit
            | TransferError::Unauthorized => EmpackExitCode::Usage,
            TransferError::NotFound => EmpackExitCode::NotFound,
            TransferError::Deadline
            | TransferError::RateLimited
            | TransferError::Server(_)
            | TransferError::Status(_)
            | TransferError::Network
            | TransferError::RedirectLimit => EmpackExitCode::Network,
        };
    }

    if find_chain_error::<reqwest::Error>(error).is_some() {
        return EmpackExitCode::Network;
    }

    if find_chain_error::<ConfigError>(error).is_some() {
        return EmpackExitCode::Usage;
    }
    EmpackExitCode::General
}

fn find_chain_error<T>(error: &Error) -> Option<&T>
where
    T: std::error::Error + Send + Sync + 'static,
{
    // Anyhow retains typed context as well as source errors. Iterating std::error::Error
    // sources alone loses context values created by Option::context.
    error
        .downcast_ref::<T>()
        .or_else(|| error.chain().find_map(|cause| cause.downcast_ref::<T>()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn config_parse_errors_are_usage_errors() {
        let error = anyhow::Error::new(ConfigError::ParseError {
            value: "command line".into(),
            reason: "unexpected argument".into(),
        });
        assert_eq!(classify_error(&error), EmpackExitCode::Usage);
    }
    #[test]
    fn incidental_error_text_does_not_define_exit_status() {
        for message in [
            "unknown build target inside a template",
            "failed to download appears in user data",
            "no results found for query: a diagnostic",
        ] {
            assert_eq!(
                classify_error(&anyhow::anyhow!(message)),
                EmpackExitCode::General
            );
        }
    }
}

#[cfg(test)]
mod engine_errors {
    use super::*;
    use crate::engine::{
        project::ProjectDocumentsError, providers::CatalogError, runtime::RuntimeError,
    };
    #[test]
    fn native_error_classes_survive_host_context() {
        use anyhow::Context;
        let missing = Option::<()>::None
            .context(ProjectDocumentsError::MissingIntent)
            .unwrap_err();
        assert_eq!(
            classify_error(&missing.context("Read native project")),
            EmpackExitCode::Usage
        );
        for (error, expected) in [
            (
                anyhow::Error::new(ProjectDocumentsError::MissingIntent),
                EmpackExitCode::Usage,
            ),
            (
                anyhow::Error::new(ProjectDocumentsError::MissingLock),
                EmpackExitCode::Usage,
            ),
            (
                anyhow::Error::new(CatalogError::Network),
                EmpackExitCode::Network,
            ),
            (
                anyhow::Error::new(CatalogError::Server(503)),
                EmpackExitCode::Network,
            ),
            (
                anyhow::Error::new(CatalogError::Unauthorized),
                EmpackExitCode::Usage,
            ),
            (
                anyhow::Error::new(CatalogError::NotFound),
                EmpackExitCode::NotFound,
            ),
            (
                anyhow::Error::new(CatalogError::NoCompatibleSelection),
                EmpackExitCode::NotFound,
            ),
            (
                anyhow::Error::new(CatalogError::Ambiguous),
                EmpackExitCode::Usage,
            ),
            (
                anyhow::Error::new(RuntimeError::Cancelled),
                EmpackExitCode::Interrupted,
            ),
        ] {
            assert_eq!(
                classify_error(&error.context("Host operation failed")),
                expected
            );
        }
    }
}

#[cfg(test)]
mod native_transfers {
    use super::*;
    use crate::engine::acquisition::TransferError;
    #[test]
    fn transfer_errors_use_typed_status_even_under_misleading_context() {
        for (error, expected) in [
            (TransferError::InvalidLocator, EmpackExitCode::Usage),
            (TransferError::ByteLimit, EmpackExitCode::Usage),
            (TransferError::Unauthorized, EmpackExitCode::Usage),
            (TransferError::NotFound, EmpackExitCode::NotFound),
            (TransferError::Deadline, EmpackExitCode::Network),
            (TransferError::RateLimited, EmpackExitCode::Network),
            (TransferError::Server(503), EmpackExitCode::Network),
            (TransferError::Status(418), EmpackExitCode::Network),
            (TransferError::Network, EmpackExitCode::Network),
            (TransferError::RedirectLimit, EmpackExitCode::Network),
        ] {
            assert_eq!(
                classify_error(
                    &anyhow::Error::new(error).context("Unknown build target in download context")
                ),
                expected
            );
        }
    }
}

#[cfg(test)]
mod recipe_errors {
    #[test]
    fn invalid_recipe_policy_is_a_usage_failure_with_context() {
        let error = anyhow::Error::new(empack_core::distribution::RecipeError::Delivery)
            .context("Requested consumer recipe");
        assert_eq!(super::classify_error(&error), super::EmpackExitCode::Usage);
    }
}
