use crate::empack::builds::BuildError;
use crate::empack::config::ConfigError as ProjectConfigError;
use crate::empack::import::ImportError;
use crate::empack::packwiz::PackwizError;
use crate::empack::parsing::ParseError as DomainParseError;
use crate::empack::search::SearchError;
use crate::empack::state::StateError;
use crate::networking::NetworkingError;
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
    if let Some(search_error) = find_chain_error::<SearchError>(error) {
        return match search_error {
            SearchError::NoResults { .. } => EmpackExitCode::NotFound,
            SearchError::NetworkError { .. }
            | SearchError::RequestError { .. }
            | SearchError::MissingApiKey { .. } => EmpackExitCode::Network,
            SearchError::LowConfidence { .. }
            | SearchError::ExtraWords { .. }
            | SearchError::IncompatibleProject { .. }
            | SearchError::Other(_) => EmpackExitCode::Usage,
            SearchError::JsonError { .. } => EmpackExitCode::General,
        };
    }

    if find_chain_error::<NetworkingError>(error).is_some()
        || find_chain_error::<reqwest::Error>(error).is_some()
    {
        return EmpackExitCode::Network;
    }

    if let Some(import_error) = find_chain_error::<ImportError>(error) {
        return match import_error {
            ImportError::DownloadFailed(_) => EmpackExitCode::Network,
            ImportError::ArchiveRead(_)
            | ImportError::CurseForgeManifestMissing
            | ImportError::ModrinthManifestMissing
            | ImportError::ParseFailed(_)
            | ImportError::MissingField { .. }
            | ImportError::UnknownLoader(_)
            | ImportError::AlreadyEmpackProject
            | ImportError::UnrecognizedSource(_) => EmpackExitCode::Usage,
        };
    }

    if let Some(build_error) = find_chain_error::<BuildError>(error) {
        return classify_build_error(build_error);
    }

    if let Some(state_error) = find_chain_error::<StateError>(error) {
        return classify_state_error(state_error);
    }

    if find_chain_error::<PackwizError>(error).is_some() {
        return EmpackExitCode::General;
    }

    if find_chain_error::<ConfigError>(error).is_some()
        || find_chain_error::<ProjectConfigError>(error).is_some()
        || find_chain_error::<DomainParseError>(error).is_some()
    {
        return EmpackExitCode::Usage;
    }

    let chain_text = error
        .chain()
        .map(|cause| cause.to_string())
        .collect::<Vec<_>>()
        .join(" | ");

    classify_message(&chain_text)
}

fn classify_build_error(error: &BuildError) -> EmpackExitCode {
    match error {
        BuildError::UnsupportedTarget { .. }
        | BuildError::ConfigError { .. }
        | BuildError::ValidationError { .. }
        | BuildError::PackInfoError { .. } => EmpackExitCode::Usage,
        BuildError::IoError { .. }
        | BuildError::CommandFailed { .. }
        | BuildError::MissingTool { .. } => EmpackExitCode::General,
    }
}

fn classify_state_error(error: &StateError) -> EmpackExitCode {
    match error {
        StateError::InvalidDirectory { .. }
        | StateError::InvalidTransition { .. }
        | StateError::MissingFile { .. }
        | StateError::ConfigError { .. }
        | StateError::ConfigManagementError { .. } => EmpackExitCode::Usage,
        StateError::BuildError { source } => classify_build_error(source),
        StateError::IoError { .. } | StateError::CommandFailed { .. } => EmpackExitCode::General,
    }
}

fn find_chain_error<T>(error: &Error) -> Option<&T>
where
    T: std::error::Error + 'static,
{
    error.chain().find_map(|cause| cause.downcast_ref::<T>())
}

fn classify_message(message: &str) -> EmpackExitCode {
    let normalized = message.to_ascii_lowercase();

    if normalized.contains("no results found for query:") {
        return EmpackExitCode::NotFound;
    }

    if normalized.contains("network request failed")
        || normalized.contains("http request failed")
        || normalized.contains("failed to download")
        || normalized.contains("failed to fetch")
        || normalized.contains("api key required")
        || normalized.contains("http client unavailable")
        || normalized.contains("failed to create http client")
        || normalized.contains("error sending request for url")
        || normalized.contains("failed to connect to")
        || normalized.contains("http error:")
    {
        return EmpackExitCode::Network;
    }

    if normalized.contains("not in a modpack directory")
        || normalized.contains("project initialization is incomplete")
        || normalized.contains("no mods specified")
        || normalized.contains("no build targets specified")
        || normalized.contains("unknown build target")
        || normalized.contains("loader version is required")
        || normalized.contains("direct .zip urls require --type")
        || normalized.contains("direct .zip urls support only --type")
        || normalized.contains("adding non-.zip direct-download files is not supported")
        || normalized
            .contains("tracked local dependencies are not yet supported for mrpack exports")
        || normalized.contains("tracked local dependency failed validation")
        || normalized.contains("tracked local dependencies failed validation")
        || normalized.contains("no pending restricted build to continue")
    {
        return EmpackExitCode::Usage;
    }

    EmpackExitCode::General
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::empack::search::SearchError;

    #[test]
    fn classify_error_maps_search_no_results_to_not_found() {
        let error = anyhow::Error::new(SearchError::NoResults {
            query: "sodium".to_string(),
        });
        assert_eq!(classify_error(&error), EmpackExitCode::NotFound);
    }

    #[test]
    fn classify_error_maps_missing_api_key_to_network() {
        let error = anyhow::Error::new(SearchError::MissingApiKey {
            platform: "curseforge".to_string(),
        });
        assert_eq!(classify_error(&error), EmpackExitCode::Network);
    }

    #[test]
    fn classify_error_maps_build_validation_to_usage() {
        let error = anyhow::Error::new(BuildError::ValidationError {
            reason: "pack/ directory not found".to_string(),
        });
        assert_eq!(classify_error(&error), EmpackExitCode::Usage);
    }

    #[test]
    fn classify_error_maps_packwiz_failure_to_general() {
        let error = anyhow::Error::new(PackwizError::CommandFailed {
            command: "packwiz remove sodium".to_string(),
            stderr: "mod not found".to_string(),
        });
        assert_eq!(classify_error(&error), EmpackExitCode::General);
    }

    #[test]
    fn classify_error_maps_config_parse_to_usage() {
        let error = anyhow::Error::new(ConfigError::ParseError {
            value: "command line".to_string(),
            reason: "error: unexpected argument".to_string(),
        });
        assert_eq!(classify_error(&error), EmpackExitCode::Usage);
    }

    #[test]
    fn classify_error_uses_message_fallback_for_project_state_strings() {
        let error = anyhow::anyhow!("Not in a modpack directory");
        assert_eq!(classify_error(&error), EmpackExitCode::Usage);
    }

    #[test]
    fn classify_error_does_not_overclassify_generic_http_prefix_messages() {
        let error = anyhow::anyhow!("http proxy configuration is invalid");
        assert_eq!(classify_error(&error), EmpackExitCode::General);
    }

    #[test]
    fn classify_error_maps_invalid_direct_zip_type_to_usage() {
        let error = anyhow::anyhow!(
            "Direct .zip URLs support only --type resourcepack, shader, or datapack"
        );
        assert_eq!(classify_error(&error), EmpackExitCode::Usage);
    }

    #[test]
    fn classify_error_maps_tracked_local_dependency_validation_strings_to_usage() {
        let singular = anyhow::anyhow!("1 tracked local dependency failed validation");
        assert_eq!(classify_error(&singular), EmpackExitCode::Usage);

        let plural = anyhow::anyhow!("2 tracked local dependencies failed validation");
        assert_eq!(classify_error(&plural), EmpackExitCode::Usage);
    }
}
