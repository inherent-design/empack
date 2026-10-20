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
    pub replacements: BTreeMap<DependencyKey, ProviderProjectId>,
    pub releases: Option<ReleasePolicy>,
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
                java_major: 21,
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
        let java_major = intent.runtime.java_major;
        intent.runtime = self.runtime.clone();
        intent.runtime.java_major = java_major;
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
        let report = delta::MigrationDelta::between(source.intent(), prior.lock(), proposed);
        if self.json {
            session
                .display()
                .status()
                .message(&serde_json::to_string_pretty(&report)?);
        } else {
            for line in report.text_lines() {
                session.display().status().info(&line);
            }
        }
        Ok(())
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
