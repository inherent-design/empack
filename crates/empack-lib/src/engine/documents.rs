//! Versioned intent/lock codecs. Decoding creates semantic data, never publication authority.
use anyhow::{Context, Result, bail, ensure};
use empack_core::{
    digest::{ContentId, DigestSet, ExpectedDigest},
    identity::*,
    model::*,
    path::{InstallDestination, PathSyntax, PortableRelPath},
    projection::BuildTarget,
    requirements::*,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

mod intent;
mod lock;

/// Malformed authored documents are usage failures, distinct from filesystem failures.
#[derive(Debug, thiserror::Error)]
#[error("Invalid {kind} document: {origin}")]
pub struct InvalidDocument {
    kind: &'static str,
    origin: String,
}

/// Maximum encoded document length, before YAML allocation.
pub const MAX_DOCUMENT_BYTES: usize = 16 * 1024 * 1024;
/// Normalized authoring schema. It is separate from the program version.
pub const INTENT_SCHEMA: u64 = 2;
/// Exact selection schema.
pub const LOCK_SCHEMA: u64 = 1;

/// Parsed intent retains raw bytes independently of canonical semantic meaning.
#[derive(Debug, Clone)]
pub struct DecodedIntent {
    intent: ProjectIntent,
    semantic_revision: SemanticRevision,
    raw_revision: DocumentRevision,
    original: Vec<u8>,
}
impl DecodedIntent {
    /// Checked semantic intent.
    pub fn intent(&self) -> &ProjectIntent {
        &self.intent
    }
    /// Canonical intent revision used by exact locks.
    pub fn semantic_revision(&self) -> SemanticRevision {
        self.semantic_revision
    }
    /// Raw revision that binds every source edit, including comments.
    pub fn raw_revision(&self) -> DocumentRevision {
        self.raw_revision
    }
    /// Original bytes for no-op preservation and source diagnostics.
    pub fn original(&self) -> &[u8] {
        &self.original
    }
}
/// A structurally checked prior lock can be inspected even after authoring intent changes.
#[derive(Debug, Clone)]
pub struct DecodedLock {
    lock: ResolutionLock,
    raw_revision: DocumentRevision,
}
impl DecodedLock {
    pub fn lock(&self) -> &ResolutionLock {
        &self.lock
    }
    pub fn raw_revision(&self) -> DocumentRevision {
        self.raw_revision
    }
    /// Only binding to current intent establishes a coherent resolved project.
    pub fn bind(&self, source: &DecodedIntent) -> Result<ResolvedProject> {
        Ok(ResolvedProject::validate(
            source.intent.clone(),
            self.lock.clone(),
            source.semantic_revision,
        )?)
    }
}
/// Whether a prepared edit preserves the original syntax or explicitly reformats it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentEdit {
    /// No semantic change; original comments and formatting are retained byte-for-byte.
    Unchanged,
    /// The caller must include document reformatting in its plan/receipt.
    Reformatted,
}
/// Prepared replacement bytes with their expected source revision, not permission to write.
#[derive(Debug, Clone)]
pub struct PreparedDocument {
    /// Compare against actual source bytes before publishing.
    pub expected: DocumentRevision,
    /// Candidate document bytes.
    pub bytes: Vec<u8>,
    /// Explicit syntax-preservation result.
    pub edit: DocumentEdit,
}
/// Explicit file-role choices for one provider project; destinations carry no mutation authority.
#[derive(Clone)]
pub struct ProviderFileSelection {
    pub requirements: Requirements,
    pub files: BTreeMap<String, NonEmpty<Placement>>,
}
/// Stateless normalized YAML codec. Unknown fields fail outside `extensions`.
pub struct DocumentCodec;
impl DocumentCodec {
    /// Decode per-file destinations using the same strict placement/requirement wire contract.
    pub fn decode_provider_files(
        &self,
        bytes: &[u8],
        origin: &str,
    ) -> Result<ProviderFileSelection> {
        (|| {
            ensure!(
                bytes.len() <= 1 << 20,
                "Provider file plan exceeds size limit"
            );
            let value = parse(bytes)?;
            fields(&value, &["schema", "environment", "files"])?;
            ensure!(
                required(&value, "schema")? == &json!(1),
                "Unsupported file plan schema"
            );
            let requirements = requirements(required(&value, "environment")?)?;
            let values = object(required(&value, "files")?)?;
            ensure!(
                !values.is_empty() && values.len() <= 128,
                "Select between one and 128 provider files"
            );
            let mut files = BTreeMap::new();
            for (name, values) in values {
                FileSlot::parse(name)?;
                let values = values
                    .as_array()
                    .context("File placements must be a list")?;
                ensure!(values.len() <= 128, "Too many file placements");
                files.insert(
                    name.clone(),
                    NonEmpty::new(values.iter().map(placement).collect::<Result<Vec<_>>>()?)?,
                );
            }
            Ok(ProviderFileSelection {
                requirements,
                files,
            })
        })()
        .with_context(|| InvalidDocument {
            kind: "provider file plan",
            origin: origin.into(),
        })
    }
    /// Parse intent with a named origin for diagnostics. No legacy fallback is attempted.
    pub fn decode_intent(&self, bytes: &[u8], origin: &str) -> Result<DecodedIntent> {
        let value = parse(bytes).with_context(|| InvalidDocument {
            kind: "intent",
            origin: origin.into(),
        })?;
        let intent = intent::decode(&value).with_context(|| InvalidDocument {
            kind: "intent",
            origin: origin.into(),
        })?;
        intent.validate()?;
        let canonical = intent::encode(&intent);
        // Validate programmatic values through the same wire boundary as loaded values.
        ensure!(
            intent::decode(&canonical)? == intent,
            "Intent has a noncanonical semantic value"
        );
        Ok(DecodedIntent {
            semantic_revision: SemanticRevision(fingerprint(b"empack.intent.v2", &canonical)),
            raw_revision: DocumentRevision(Sha256::digest(bytes).into()),
            intent,
            original: bytes.to_vec(),
        })
    }
    /// Encode a checked normalized intent. This returns bytes and performs no filesystem write.
    pub fn encode_intent(&self, intent: &ProjectIntent) -> Result<Vec<u8>> {
        intent.validate()?;
        let value = intent::encode(intent);
        ensure!(
            intent::decode(&value)? == *intent,
            "Intent has a noncanonical semantic value"
        );
        serialize(&value)
    }
    /// Prepare a raw-revision-bound edit. No-op edits preserve original bytes exactly.
    pub fn replace_intent(
        &self,
        source: &DecodedIntent,
        next: &ProjectIntent,
    ) -> Result<PreparedDocument> {
        let (bytes, edit) = if source.intent() == next {
            (source.original.clone(), DocumentEdit::Unchanged)
        } else {
            (self.encode_intent(next)?, DocumentEdit::Reformatted)
        };
        Ok(PreparedDocument {
            expected: source.raw_revision,
            bytes,
            edit,
        })
    }
    /// Decode and validate an exact lock against this particular semantic intent.
    pub fn decode_lock(
        &self,
        bytes: &[u8],
        source: &DecodedIntent,
        origin: &str,
    ) -> Result<ResolvedProject> {
        self.decode_prior_lock(bytes, origin)?.bind(source)
    }
    /// Preserve exact previous selections for planning after an authoring edit; never call them current.
    pub fn decode_prior_lock(&self, bytes: &[u8], origin: &str) -> Result<DecodedLock> {
        let value = parse(bytes).with_context(|| InvalidDocument {
            kind: "lock",
            origin: origin.into(),
        })?;
        let lock = lock::decode(&value).with_context(|| InvalidDocument {
            kind: "lock",
            origin: origin.into(),
        })?;
        lock.validate_structure()?;
        Ok(DecodedLock {
            lock,
            raw_revision: DocumentRevision(Sha256::digest(bytes).into()),
        })
    }
    /// Serialize only an internally validated exact resolution.
    pub fn encode_lock(&self, project: &ResolvedProject) -> Result<Vec<u8>> {
        let value = lock::encode(project.lock());
        let decoded = lock::decode(&value)?;
        ensure!(
            decoded == *project.lock(),
            "Lock has a noncanonical semantic value"
        );
        serialize(&value)
    }
}
fn parse(bytes: &[u8]) -> Result<Value> {
    ensure!(
        bytes.len() <= MAX_DOCUMENT_BYTES,
        "Document exceeds size limit"
    );
    let text = std::str::from_utf8(bytes).context("Document must be UTF-8")?;
    Ok(serde_saphyr::from_str(text)?)
}
fn serialize(value: &Value) -> Result<Vec<u8>> {
    let bytes = serde_saphyr::to_string(value)?.into_bytes();
    ensure!(
        bytes.len() <= MAX_DOCUMENT_BYTES,
        "Document exceeds size limit"
    );
    Ok(bytes)
}
/// Tagged, length-delimited canonical hashing; object order is sorted explicitly.
fn fingerprint(domain: &[u8], value: &Value) -> [u8; 32] {
    fn blob(hash: &mut Sha256, bytes: &[u8]) {
        hash.update((bytes.len() as u64).to_be_bytes());
        hash.update(bytes);
    }
    fn walk(hash: &mut Sha256, value: &Value) {
        match value {
            Value::Null => hash.update([0]),
            Value::Bool(value) => hash.update([1, u8::from(*value)]),
            Value::Number(value) => {
                hash.update([2]);
                blob(hash, value.to_string().as_bytes());
            }
            Value::String(value) => {
                hash.update([3]);
                blob(hash, value.as_bytes());
            }
            Value::Array(values) => {
                hash.update([4]);
                hash.update((values.len() as u64).to_be_bytes());
                for value in values {
                    walk(hash, value);
                }
            }
            Value::Object(values) => {
                hash.update([5]);
                hash.update((values.len() as u64).to_be_bytes());
                let sorted: BTreeMap<_, _> = values.iter().collect();
                for (key, value) in sorted {
                    blob(hash, key.as_bytes());
                    walk(hash, value);
                }
            }
        }
    }
    let mut hash = Sha256::new();
    blob(&mut hash, domain);
    walk(&mut hash, value);
    hash.finalize().into()
}
fn fields<'a>(value: &'a Value, allowed: &[&str]) -> Result<&'a serde_json::Map<String, Value>> {
    let object = value.as_object().context("Expected an object")?;
    for key in object.keys() {
        ensure!(allowed.contains(&key.as_str()), "Unknown field '{key}'");
    }
    Ok(object)
}
fn required<'a>(value: &'a Value, name: &str) -> Result<&'a Value> {
    value
        .get(name)
        .with_context(|| format!("Missing field '{name}'"))
}
fn text(value: &Value) -> Result<&str> {
    value.as_str().context("Expected a string")
}
fn label(value: &Value) -> Result<&str> {
    let s = text(value)?;
    ensure!(
        !s.trim().is_empty() && !s.chars().any(char::is_control),
        "Expected nonempty text without control characters"
    );
    Ok(s)
}
fn optional_text(value: &Value, key: &str) -> Result<Option<String>> {
    value
        .get(key)
        .filter(|v| !v.is_null())
        .map(|v| text(v).map(str::to_owned))
        .transpose()
}
fn array(value: &Value) -> Result<&[Value]> {
    value
        .as_array()
        .map(Vec::as_slice)
        .context("Expected an array")
}
fn string_list(value: &Value) -> Result<Vec<String>> {
    array(value)?
        .iter()
        .map(|v| text(v).map(str::to_owned))
        .collect()
}
fn object(value: &Value) -> Result<&serde_json::Map<String, Value>> {
    value.as_object().context("Expected an object")
}
fn path(value: &Value) -> Result<PortableRelPath> {
    Ok(PortableRelPath::parse(
        text(value)?,
        PathSyntax::ProjectContent,
    )?)
}
fn digest_set(value: &Value) -> Result<DigestSet> {
    let pairs = object(value)?
        .iter()
        .map(|(algorithm, value)| Ok((algorithm.as_str(), text(value)?)))
        .collect::<Result<Vec<_>>>()?;
    Ok(DigestSet::parse(pairs)?)
}

fn digests(value: &DigestSet) -> Value {
    Value::Object(
        value
            .values()
            .iter()
            .map(|v| (v.algorithm().name().into(), Value::String(v.hex())))
            .collect(),
    )
}
fn hash32(value: &Value) -> Result<[u8; 32]> {
    match ExpectedDigest::parse("sha256", text(value)?)? {
        ExpectedDigest::Sha256(value) => Ok(value),
        _ => unreachable!(),
    }
}
fn hex32(value: [u8; 32]) -> String {
    ExpectedDigest::Sha256(value).hex()
}
fn provider(value: &Value) -> Result<ProviderProjectId> {
    fields(value, &["provider", "project"])?;
    let project = text(required(value, "project")?)?;
    match text(required(value, "provider")?)? {
        "modrinth" => Ok(ProviderProjectId::Modrinth(ModrinthProjectId::parse(
            project,
        )?)),
        "curseforge" => Ok(ProviderProjectId::CurseForge(CurseForgeProjectId::parse(
            project,
        )?)),
        _ => bail!("Unknown provider"),
    }
}
fn provider_value(value: &ProviderProjectId) -> Value {
    match value {
        ProviderProjectId::Modrinth(id) => json!({"provider":"modrinth", "project":id.as_str()}),
        ProviderProjectId::CurseForge(id) => {
            json!({"provider":"curseforge", "project":id.to_string()})
        }
    }
}
fn pin(value: &Value) -> Result<PinSelector> {
    fields(value, &["provider", "id"])?;
    match text(required(value, "provider")?)? {
        "modrinth" => Ok(PinSelector::ModrinthVersion(ModrinthVersionId::parse(
            text(required(value, "id")?)?,
        )?)),
        "curseforge" => Ok(PinSelector::CurseForgeFile(CurseForgeFileId::parse(text(
            required(value, "id")?,
        )?)?)),
        _ => bail!("Unknown pin provider"),
    }
}
fn pin_value(value: &PinSelector) -> Value {
    match value {
        PinSelector::ModrinthVersion(id) => json!({"provider":"modrinth", "id":id.as_str()}),
        PinSelector::CurseForgeFile(id) => json!({"provider":"curseforge", "id":id.to_string()}),
    }
}
fn kind(value: &Value) -> Result<ContentKind> {
    match text(value)? {
        "mod" => Ok(ContentKind::Mod),
        "resource-pack" => Ok(ContentKind::ResourcePack),
        "shader-pack" => Ok(ContentKind::ShaderPack),
        "data-pack" => Ok(ContentKind::DataPack),
        "world" => Ok(ContentKind::World),
        "config" => Ok(ContentKind::Config),
        "other-file" => Ok(ContentKind::OtherFile),
        _ => bail!("Unknown content kind"),
    }
}
pub(in crate::engine) fn kind_name(value: ContentKind) -> &'static str {
    match value {
        ContentKind::Mod => "mod",
        ContentKind::ResourcePack => "resource-pack",
        ContentKind::ShaderPack => "shader-pack",
        ContentKind::DataPack => "data-pack",
        ContentKind::World => "world",
        ContentKind::Config => "config",
        ContentKind::OtherFile => "other-file",
    }
}
fn loader(value: &Value) -> Result<LoaderKind> {
    match text(value)? {
        "vanilla" => Ok(LoaderKind::Vanilla),
        "fabric" => Ok(LoaderKind::Fabric),
        "quilt" => Ok(LoaderKind::Quilt),
        "forge" => Ok(LoaderKind::Forge),
        "neoforge" => Ok(LoaderKind::NeoForge),
        _ => bail!("Unknown loader family"),
    }
}
fn loader_name(value: LoaderKind) -> &'static str {
    match value {
        LoaderKind::Vanilla => "vanilla",
        LoaderKind::Fabric => "fabric",
        LoaderKind::Quilt => "quilt",
        LoaderKind::Forge => "forge",
        LoaderKind::NeoForge => "neoforge",
    }
}
fn requirement(value: &Value) -> Result<Requirement> {
    if value == "required" {
        return Ok(Requirement::Required);
    }
    if value == "unsupported" {
        return Ok(Requirement::Unsupported);
    }
    fields(value, &["optional", "default-enabled", "description"])?;
    Ok(Requirement::Optional(OptionalChoice {
        key: ChoiceKey::parse(text(required(value, "optional")?)?)?,
        default_enabled: required(value, "default-enabled")?
            .as_bool()
            .context("Expected a boolean")?,
        description: optional_text(value, "description")?,
    }))
}
fn requirement_value(value: &Requirement) -> Value {
    match value {
        Requirement::Required => json!("required"),
        Requirement::Unsupported => json!("unsupported"),
        Requirement::Optional(choice) => {
            json!({"optional":choice.key.as_str(),"default-enabled":choice.default_enabled,"description":choice.description})
        }
    }
}
fn requirements(value: &Value) -> Result<Requirements> {
    fields(value, &["client", "server"])?;
    Ok(Requirements {
        client: requirement(required(value, "client")?)?,
        server: requirement(required(value, "server")?)?,
    })
}
fn requirements_value(value: &Requirements) -> Value {
    json!({"client":requirement_value(&value.client),"server":requirement_value(&value.server)})
}
fn placement(value: &Value) -> Result<Placement> {
    fields(value, &["destination", "layer", "environment"])?;
    Ok(Placement {
        destination: InstallDestination::parse(text(required(value, "destination")?)?)?,
        layer: match text(required(value, "layer")?)? {
            "common" => ContentLayer::Common,
            "common-override" => ContentLayer::CommonOverride,
            "client" => ContentLayer::Client,
            "server" => ContentLayer::Server,
            _ => bail!("Unknown content layer"),
        },
        requirements: requirements(required(value, "environment")?)?,
    })
}
fn placement_value(value: &Placement) -> Value {
    json!({"destination":value.destination.relative().as_str(),"layer": match value.layer { ContentLayer::Common => "common", ContentLayer::CommonOverride => "common-override", ContentLayer::Client => "client", ContentLayer::Server => "server" }, "environment": requirements_value(&value.requirements)})
}
fn urls(value: &Value) -> Result<Vec<String>> {
    let urls = string_list(value)?;
    for value in &urls {
        validate_download_url(value)?;
    }
    Ok(urls)
}
pub(crate) fn validate_download_url(value: &str) -> Result<()> {
    let url = reqwest::Url::parse(value)?;
    ensure!(
        url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none(),
        "Persistent download alternatives require credential-free HTTPS locators"
    );
    // Signed locators are supplied by the execution context, not a durable lock.
    ensure!(
        !url.query_pairs().any(|(key, _)| {
            // URL parsing percent-decodes keys; normalize common separator/case variants too.
            let key: String = key
                .chars()
                .filter(char::is_ascii_alphanumeric)
                .flat_map(char::to_lowercase)
                .collect();
            matches!(
                key.as_str(),
                "key"
                    | "auth"
                    | "sig"
                    | "pwd"
                    | "jwt"
                    | "code"
                    | "ticket"
                    | "session"
                    | "sessionid"
                    | "awsaccesskeyid"
                    | "googleaccessid"
            ) || [
                "token",
                "credential",
                "secret",
                "signature",
                "authorization",
                "password",
                "passwd",
                "apikey",
            ]
            .iter()
            .any(|part| key.contains(part))
        }),
        "Ephemeral credentials cannot be persisted in download alternatives"
    );
    Ok(())
}

fn extension(value: &Value) -> ExtensionValue {
    match value {
        Value::Null => ExtensionValue::Null,
        Value::Bool(v) => ExtensionValue::Bool(*v),
        Value::Number(v) => ExtensionValue::Number(v.to_string()),
        Value::String(v) => ExtensionValue::Text(v.clone()),
        Value::Array(v) => ExtensionValue::List(v.iter().map(extension).collect()),
        Value::Object(v) => {
            ExtensionValue::Object(v.iter().map(|(k, v)| (k.clone(), extension(v))).collect())
        }
    }
}
fn extension_value(value: &ExtensionValue) -> Value {
    match value {
        ExtensionValue::Null => Value::Null,
        ExtensionValue::Bool(v) => json!(v),
        ExtensionValue::Number(v) => v
            .parse::<serde_json::Number>()
            .map(Value::Number)
            .unwrap_or(Value::Null),
        ExtensionValue::Text(v) => json!(v),
        ExtensionValue::List(v) => Value::Array(v.iter().map(extension_value).collect()),
        ExtensionValue::Object(v) => Value::Object(
            v.iter()
                .map(|(k, v)| (k.clone(), extension_value(v)))
                .collect(),
        ),
    }
}

#[cfg(test)]
mod tests;
