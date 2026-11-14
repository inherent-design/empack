//! Safe provider-data diagnostics retain structural fields, never raw values.
use super::CatalogError;
use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecordInvariant {
    NonEmpty,
    AtMostOnePrimary,
    UniqueFilename,
    UniqueSelection,
    ConsistentSelection,
    Pagination,
    PortableFilename,
    DownloadLocator,
    DigestSet,
    RecognizedValue,
    CanonicalIdentity,
    Timestamp,
    JsonSyntax,
    JsonType,
}
impl std::fmt::Display for RecordInvariant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NonEmpty => "at least one value is required",
            Self::AtMostOnePrimary => "at most one file may be primary",
            Self::UniqueFilename => "file names must be unique within a version",
            Self::UniqueSelection => {
                "selection identities must not repeat within one catalog query"
            }
            Self::ConsistentSelection => {
                "one selection must have consistent evidence across queries"
            }
            Self::Pagination => "page counts, indices and totals must remain consistent",
            Self::PortableFilename => "a file name must be one portable path component",
            Self::DownloadLocator => {
                "a download must be an admitted HTTPS locator without credentials or fragment"
            }
            Self::DigestSet => "a nonempty supported digest set with valid encodings is required",
            Self::RecognizedValue => "a supported value with the required type is required",
            Self::CanonicalIdentity => "canonical project and selection identities must agree",
            Self::Timestamp => "a valid RFC3339 publication timestamp is required",
            Self::JsonSyntax => "complete valid JSON is required",
            Self::JsonType => "a required field is missing or has the wrong type",
        })
    }
}

#[derive(Debug, Clone, thiserror::Error, Serialize)]
#[error("Provider field {field}: {invariant}")]
pub struct ProviderRecordProblem {
    pub field: String,
    pub invariant: RecordInvariant,
}
pub(super) fn record_error(field: impl Into<String>, invariant: RecordInvariant) -> anyhow::Error {
    anyhow::Error::new(CatalogError::InvalidRecord).context(ProviderRecordProblem {
        field: field.into(),
        invariant,
    })
}

pub(super) fn identity_error(field: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(CatalogError::Identity).context(ProviderRecordProblem {
        field: field.into(),
        invariant: RecordInvariant::CanonicalIdentity,
    })
}
pub(super) fn selection_context(pin: &super::ResolvedPin) -> String {
    match (&pin.project, &pin.selection) {
        (
            empack_core::identity::ProviderProjectId::Modrinth(project),
            empack_core::identity::PinSelector::ModrinthVersion(version),
        ) => format!("Invalid Modrinth version {version} in project {project}"),
        (
            empack_core::identity::ProviderProjectId::CurseForge(project),
            empack_core::identity::PinSelector::CurseForgeFile(file),
        ) => format!("Invalid CurseForge file {file} in project {project}"),
        _ => "Provider project and selection use different namespaces".into(),
    }
}

// Struct field names are known schema symbols. Dynamic map keys are not safe
// diagnostic text: even an erroneous hash-algorithm key could contain a secret URL.
fn safe_field(path: &serde_path_to_error::Path) -> String {
    use serde_path_to_error::Segment;
    const FIELDS: &[&str] = &[
        "data",
        "id",
        "project_id",
        "slug",
        "title",
        "name",
        "project_type",
        "loaders",
        "client_side",
        "server_side",
        "files",
        "filename",
        "primary",
        "hashes",
        "url",
        "size",
        "file_type",
        "game_versions",
        "environment",
        "dependencies",
        "version_id",
        "file_name",
        "dependency_type",
        "gameId",
        "classId",
        "modId",
        "fileName",
        "fileLength",
        "downloadUrl",
        "gameVersions",
        "algo",
        "value",
        "relationType",
        "hits",
        "offset",
        "limit",
        "pagination",
        "index",
        "pageSize",
        "resultCount",
        "totalCount",
        "releaseType",
        "fileDate",
        "isAvailable",
        "version_number",
        "version_type",
        "date_published",
        "status",
    ];
    let mut field = String::new();
    for segment in path.iter().take(16) {
        match segment {
            Segment::Seq { index } => field.push_str(&format!("[{index}]")),
            Segment::Map { key } if FIELDS.contains(&key.as_str()) => {
                if !field.is_empty() {
                    field.push('.');
                }
                field.push_str(key);
            }
            _ => field.push_str("[entry]"),
        }
    }
    if field.is_empty() { "$".into() } else { field }
}
pub(super) fn json<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    let mut decoder = serde_json::Deserializer::from_slice(bytes);
    let value = serde_path_to_error::deserialize(&mut decoder).map_err(|error| {
        let invariant = match error.inner().classify() {
            serde_json::error::Category::Data => RecordInvariant::JsonType,
            _ => RecordInvariant::JsonSyntax,
        };
        record_error(safe_field(error.path()), invariant)
    })?;
    // The path adapter parses one value; keep serde_json::from_slice's strict
    // refusal of trailing input. Neither error's value-bearing message is retained.
    decoder
        .end()
        .map_err(|_| record_error("$", RecordInvariant::JsonSyntax))?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_json_errors_identify_fields_without_echoing_values_or_map_keys() {
        #[derive(serde::Deserialize)]
        struct Input {
            files: Vec<File>,
        }
        #[derive(serde::Deserialize)]
        struct File {
            size: u64,
            hashes: std::collections::BTreeMap<String, String>,
        }
        let invalid = [
            (br#"{"files":[{"size":"https://user:secret@example.com/a?key=secret","hashes":{}}]}"#.as_slice(), "files[0].size"),
            (br#"{"files":[{"size":1,"hashes":{"https://example.com/?token=secret":7}}]}"#.as_slice(), "files[0].hashes[entry]"),
        ];
        for (bytes, field) in invalid {
            let error = json::<Input>(bytes).err().unwrap();
            assert!(error.is::<CatalogError>());
            assert_eq!(
                error.downcast_ref::<ProviderRecordProblem>().unwrap().field,
                field
            );
            let text = format!("{error:#}");
            assert!(
                !text.contains("secret") && !text.contains("https://"),
                "{text}"
            );
        }
        let value: Input = json(br#"{"files":[{"size":1,"hashes":{"sha1":"safe"}}]}"#).unwrap();
        assert_eq!(value.files[0].size, 1);
        assert_eq!(value.files[0].hashes["sha1"], "safe");
        assert!(json::<serde_json::Value>(br#"{} {"token":"secret"}"#).is_err());
    }
}
