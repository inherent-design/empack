//! Runtime migration composes candidate intent; live author files remain unchanged until approval.
use super::*;
use crate::{
    application::cli::MigrationArgs,
    engine::{
        documents::{DecodedIntent, DecodedLock, DocumentCodec},
        runtime_catalog::RuntimeCatalog,
    },
};
use empack_core::{
    identity::{CurseForgeProjectId, ModrinthProjectId, ProviderProjectId},
    model::*,
};
use std::collections::BTreeMap;
mod delta;

pub(super) struct MigrationOptions {
    pub runtime: RuntimeIntent,
    pub java: Option<u16>,
    pub replacements: BTreeMap<DependencyKey, ProviderProjectId>,
    pub releases: Option<ReleasePolicy>,
    pub json: bool,
}
impl MigrationOptions {
    pub fn parse(args: MigrationArgs, mode: ApplicationMode) -> Result<Self> {
        if args.json && mode != ApplicationMode::Preview {
            return Err(crate::primitives::ConfigError::ParseError {
                value: "migrate --json".into(),
                reason: "requires --dry-run".into(),
            }
            .into());
        }
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
            java: args.java,
            runtime: RuntimeIntent {
                java_major: args.java.unwrap_or_else(|| {
                    empack_core::java::JavaPolicy::for_game(&args.minecraft)
                        .recommended()
                        .unwrap_or(8)
                }),
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
            releases: args.release_policy.map(Into::into),
            json: args.json,
        })
    }
    pub fn compose(
        &self,
        source: &DecodedIntent,
        prior: Option<&DecodedLock>,
    ) -> Result<DecodedIntent> {
        let prior = prior
            .context("Migration needs a current lock; synchronize the author project first")?;
        prior.bind(source)?;
        let mut intent = source.intent().clone();
        let java_major = intent.runtime.java_major;
        intent.runtime = self.runtime.clone();
        intent.runtime.java_major = self.java.unwrap_or(java_major);
        empack_core::java::JavaPolicy::for_game(intent.runtime.minecraft.as_str())
            .select(Some(intent.runtime.java_major))
            .context("The selected runtime invalidates the preferred Java major; review the recommendation and supply migrate --java explicitly")?;
        for (key, id) in &self.replacements {
            let root = intent
                .roots
                .get_mut(key)
                .context("Replacement key is not an explicit dependency")?;
            ensure!(
                matches!(
                    root.source,
                    SourceIntent::Provider(_) | SourceIntent::ProviderSelector(_)
                ),
                "Migration can replace only provider-owned roots"
            );
            ensure!(
                root.source
                    .canonical_project(prior.lock().dependencies.get(key))
                    != Some(id),
                "Replacement selects the existing project; omit it to retain its policy"
            );
            root.source = SourceIntent::Provider(id.clone());
            root.selection = DependencySelection::Automatic;
        }
        DocumentCodec
            .replace_intent(source, &intent)?
            .decode("migration candidate")
    }
    pub fn report(
        &self,
        source: &DecodedIntent,
        prior: &DecodedLock,
        proposed: &ResolvedProject,
        materialize: bool,
    ) -> MigrationOutput {
        MigrationOutput {
            delta: delta::MigrationDelta::between(source.intent(), prior.lock(), proposed),
            json: self.json,
            materialize,
        }
    }
}
/// Semantic observations are reported with the actual native preparation result, not as readiness alone.
pub(in crate::application::engine_host) struct MigrationOutput {
    delta: delta::MigrationDelta,
    pub json: bool,
    materialize: bool,
}
impl MigrationOutput {
    pub fn present(
        &self,
        session: &dyn Session,
        native: Result<&crate::engine::api::SyncPreview, &anyhow::Error>,
    ) -> Result<()> {
        #[derive(serde::Serialize)]
        struct Report<'a> {
            #[serde(flatten)]
            delta: &'a delta::MigrationDelta,
            proposal: &'static str,
            file_changes: Vec<crate::application::engine_host::file_report::Change>,
            references: Vec<String>,
            materialization_requested: bool,
            native_publication_performed: bool,
            diagnostic: Option<crate::engine::diagnostics::Diagnostic>,
            problem: Option<String>,
        }
        let (proposal, file_changes, references, diagnostic, problem) = match native {
            Ok(view) => (
                if view.files.changes().is_empty() {
                    "unchanged"
                } else {
                    "ready"
                },
                crate::application::engine_host::file_report::changes(&view.files)?,
                view.references
                    .iter()
                    .map(|key| format!("{}/{}", key.dependency.as_str(), key.slot.as_str()))
                    .collect(),
                None,
                None,
            ),
            Err(error) => (
                "blocked",
                Vec::new(),
                Vec::new(),
                Some(crate::engine::diagnostics::Diagnostic::from_error(
                    error,
                    crate::engine::diagnostics::DiagnosticPhase::Preparation,
                )),
                Some(format!("{error:#}").chars().take(4096).collect()),
            ),
        };
        if self.json {
            session
                .display()
                .status()
                .message(&crate::terminal::json_text(&Report {
                    delta: &self.delta,
                    proposal,
                    file_changes,
                    references,
                    materialization_requested: self.materialize,
                    native_publication_performed: false,
                    diagnostic,
                    problem,
                })?);
        } else {
            session
                .display()
                .status()
                .message(&format!("Runtime migration: {proposal}"));
            for line in self.delta.text_lines() {
                session.display().status().info(&line);
            }
            if let Ok(view) = native {
                show_changes(session, &view.files)?;
            }
        }
        Ok(())
    }
}

pub async fn migrate(session: &dyn Session, args: MigrationArgs) -> Result<()> {
    let materialize = args.materialize;
    let options = MigrationOptions::parse(
        args,
        ApplicationMode::for_action(session.config().app_config()),
    )?;
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
