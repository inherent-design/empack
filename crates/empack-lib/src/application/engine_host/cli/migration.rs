//! Runtime migration composes candidate intent; live author files remain unchanged until approval.
use super::*;
use crate::{
    application::cli::MigrationArgs,
    engine::{
        documents::{DecodedIntent, DecodedLock, DocumentCodec},
        providers::ObservedVersion,
        runtime_catalog::RuntimeCatalog,
    },
};
use empack_core::{
    identity::{CurseForgeProjectId, ModrinthProjectId, ProviderProjectId},
    model::*,
};
use serde::Serialize;
use std::collections::BTreeMap;

pub(super) struct MigrationOptions {
    pub runtime: RuntimeIntent,
    pub replacements: BTreeMap<DependencyKey, ProviderProjectId>,
    pub releases: ReleasePolicy,
    pub plan: bool,
    pub json: bool,
}
impl MigrationOptions {
    pub fn parse(args: MigrationArgs) -> Result<Self> {
        let loader = match args.loader.as_str() {
            "vanilla" => LoaderKind::Vanilla,
            "fabric" => LoaderKind::Fabric,
            "quilt" => LoaderKind::Quilt,
            "forge" => LoaderKind::Forge,
            "neoforge" => LoaderKind::NeoForge,
            _ => anyhow::bail!("Unknown loader family"),
        };
        ensure!(
            loader != LoaderKind::Vanilla || args.loader_version.is_none(),
            "Vanilla has no loader version"
        );
        let mut replacements = BTreeMap::new();
        for replacement in args.replacements {
            let (key, source) = replacement
                .split_once('=')
                .context("Replacement must be KEY=PROVIDER:ID")?;
            let (provider, id) = source
                .split_once(':')
                .context("Replacement must name a provider and canonical project ID")?;
            let id = match provider {
                "modrinth" => ProviderProjectId::Modrinth(ModrinthProjectId::parse(id)?),
                "curseforge" => ProviderProjectId::CurseForge(CurseForgeProjectId::parse(id)?),
                _ => anyhow::bail!("Replacement provider must be modrinth or curseforge"),
            };
            ensure!(
                replacements
                    .insert(DependencyKey::parse(key)?, id)
                    .is_none(),
                "Duplicate replacement key"
            );
        }
        Ok(Self {
            runtime: RuntimeIntent {
                minecraft: GameVersion::parse(&args.minecraft)?,
                loader,
                loader_version: args
                    .loader_version
                    .map(|value| LoaderVersion::parse(&value))
                    .transpose()?,
                acceptable_versions: args
                    .acceptable_versions
                    .iter()
                    .map(|v| GameVersion::parse(v))
                    .collect::<std::result::Result<_, _>>()?,
            },
            replacements,
            releases: args.release_policy.into(),
            plan: args.plan,
            json: args.json,
        })
    }
    pub fn compose(
        &self,
        source: &DecodedIntent,
        prior: Option<&DecodedLock>,
    ) -> Result<DecodedIntent> {
        prior
            .context("Migration needs a current lock; synchronize the author project first")?
            .bind(source)?;
        let mut intent = source.intent().clone();
        intent.runtime = self.runtime.clone();
        for (key, id) in &self.replacements {
            let root = intent
                .roots
                .get_mut(key)
                .context("Replacement key is not an explicit dependency")?;
            ensure!(
                matches!(root.source, SourceIntent::Provider(_)),
                "Migration can replace only provider-owned roots"
            );
            ensure!(
                root.source != SourceIntent::Provider(id.clone()),
                "Replacement selects the existing project; omit it to retain its policy"
            );
            root.source = SourceIntent::Provider(id.clone());
            root.version = VersionIntent::FollowCompatible;
        }
        DocumentCodec
            .replace_intent(source, &intent)?
            .decode("migration candidate")
    }
    pub fn present(
        &self,
        session: &dyn Session,
        source: &DecodedIntent,
        prior: &DecodedLock,
        proposed: &ResolvedProject,
    ) -> Result<()> {
        #[derive(Serialize)]
        struct Row {
            key: String,
            source_replaced: bool,
            current: Option<ObservedVersion>,
            selection: Option<ObservedVersion>,
            retained: bool,
            manual_compatibility: bool,
        }
        #[derive(Serialize)]
        struct Report {
            schema: u32,
            minecraft: String,
            loader: String,
            loader_version: Option<String>,
            dependencies: Vec<Row>,
            limitations: &'static str,
        }
        let report = Report {
            schema: 1,
            minecraft: proposed.lock().runtime.minecraft.as_str().into(),
            loader: self.runtime_name().into(),
            loader_version: proposed
                .lock()
                .runtime
                .loader_version
                .as_ref()
                .map(|v| v.as_str().into()),
            dependencies: proposed
                .intent()
                .roots
                .iter()
                .map(|(key, root)| Row {
                    key: key.as_str().into(),
                    source_replaced: source.intent().roots[key].source != root.source,
                    current: prior
                        .lock()
                        .dependencies
                        .get(key)
                        .and_then(|d| d.selected.as_ref())
                        .map(ObservedVersion::from_pin),
                    retained: prior
                        .lock()
                        .dependencies
                        .get(key)
                        .is_some_and(|d| d.selected == proposed.lock().dependencies[key].selected),
                    selection: proposed.lock().dependencies[key]
                        .selected
                        .as_ref()
                        .map(ObservedVersion::from_pin),
                    manual_compatibility: !matches!(root.source, SourceIntent::Provider(_)),
                })
                .collect(),
            limitations: "Provider declarations and dependency evidence are checked. This does not port private files, configuration or worlds, prove gameplay compatibility, or validate a full Java compatibility range. Native conflicts are checked during preparation; a plan is not an execution grant.",
        };
        if self.json {
            session
                .display()
                .status()
                .message(&serde_json::to_string_pretty(&report)?);
        } else {
            session.display().status().info(&format!(
                "Migration candidate: Minecraft {} / {} {}",
                report.minecraft,
                report.loader,
                report.loader_version.as_deref().unwrap_or("")
            ));
            for row in report.dependencies {
                session.display().status().info(&format!(
                    "{}: {}{}",
                    row.key,
                    row.selection.as_ref().map_or_else(
                        || "manual source; verify compatibility".into(),
                        |pin| format!("{}:{}@{}", pin.provider, pin.project, pin.selection)
                    ),
                    if row.source_replaced {
                        " (explicit replacement)"
                    } else {
                        ""
                    }
                ));
            }
            session.display().status().info(report.limitations);
        }
        Ok(())
    }
    fn runtime_name(&self) -> &'static str {
        match self.runtime.loader {
            LoaderKind::Vanilla => "vanilla",
            LoaderKind::Fabric => "fabric",
            LoaderKind::Quilt => "quilt",
            LoaderKind::Forge => "forge",
            LoaderKind::NeoForge => "neoforge",
        }
    }
}
pub async fn migrate(session: &dyn Session, args: MigrationArgs) -> Result<()> {
    let materialize = args.materialize;
    let options = MigrationOptions::parse(args)?;
    synchronization::synchronize_mode(
        session,
        materialize,
        dependencies::configured_services(session)?,
        RuntimeCatalog::new(HttpAcquisition::new()?),
        Some(options),
    )
    .await
}

#[cfg(test)]
mod tests;
