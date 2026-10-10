//! User selectors resolve only through native logical records and canonical provider identities.
use anyhow::{Result, ensure};
use empack_core::{
    identity::{CurseForgeProjectId, ModrinthProjectId, ProviderProjectId},
    model::{DependencyKey, NonEmpty, ResolvedIdentity, ResolvedProject},
};
use std::collections::BTreeSet;

#[derive(Debug, Clone)]
pub enum RemovalSelector {
    /// Exact logical key, without fallback to another namespace.
    Key(DependencyKey),
    /// Exact key first, then a provider-qualified ID or case-insensitive title.
    Query(String),
    /// Canonical provider identity, independent of labels and filenames.
    Provider(ProviderProjectId),
}
#[derive(Debug, thiserror::Error)]
pub enum SelectionError {
    #[error("No tracked dependency matches '{0}'")]
    Missing(String),
    #[error("Ambiguous removal selector '{query}'; use an exact logical key")]
    Ambiguous {
        query: String,
        candidates: Vec<DependencyKey>,
    },
}
#[derive(Debug)]
pub(in crate::engine) struct ResolvedSelections {
    pub locked: Vec<DependencyKey>,
}
pub(in crate::engine) fn resolve(
    project: &ResolvedProject,
    selectors: &NonEmpty<RemovalSelector>,
) -> Result<ResolvedSelections> {
    let dependencies = &project.lock().dependencies;
    let mut selected = BTreeSet::new();
    for selector in selectors.as_slice() {
        let (query, provider) = match selector {
            RemovalSelector::Key(key) => {
                ensure!(
                    dependencies.contains_key(key),
                    SelectionError::Missing(key.as_str().into())
                );
                selected.insert(key.clone());
                continue;
            }
            RemovalSelector::Provider(provider) => {
                (format!("{provider:?}"), Some(provider.clone()))
            }
            RemovalSelector::Query(query) => {
                ensure!(
                    !query.trim().is_empty() && query.len() <= 4096,
                    "Removal selector must contain 1–4096 bytes"
                );
                if let Some(key) = dependencies.keys().find(|key| key.as_str() == query) {
                    selected.insert(key.clone());
                    continue;
                }
                let provider = if let Some(id) = query.strip_prefix("modrinth:") {
                    Some(ProviderProjectId::Modrinth(ModrinthProjectId::parse(id)?))
                } else if let Some(id) = query.strip_prefix("curseforge:") {
                    Some(ProviderProjectId::CurseForge(CurseForgeProjectId::parse(
                        id,
                    )?))
                } else {
                    None
                };
                (query.clone(), provider)
            }
        };
        let candidates: Vec<_> = dependencies
            .iter()
            .filter(|(_, dependency)| match &provider {
                Some(provider) => {
                    dependency.identity == ResolvedIdentity::Provider(provider.clone())
                }
                None => dependency.title.eq_ignore_ascii_case(&query),
            })
            .map(|(key, _)| key.clone())
            .collect();
        ensure!(
            candidates.len() <= 1,
            SelectionError::Ambiguous {
                query: query.clone(),
                candidates: candidates.clone()
            }
        );
        selected.insert(
            candidates
                .into_iter()
                .next()
                .ok_or(SelectionError::Missing(query))?,
        );
    }
    Ok(ResolvedSelections {
        locked: selected.into_iter().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::mrpack::tests::project;
    fn queries(values: &[&str]) -> NonEmpty<RemovalSelector> {
        NonEmpty::new(
            values
                .iter()
                .map(|value| RemovalSelector::Query((*value).into()))
                .collect(),
        )
        .unwrap()
    }
    #[test]
    fn exact_keys_and_titles_deduplicate_and_unknown_names_fail_the_batch() {
        let project = project(false, false);
        assert_eq!(
            resolve(&project, &queries(&["assets", "ASSETS"]))
                .unwrap()
                .locked,
            vec![DependencyKey::parse("assets").unwrap()]
        );
        for value in ["a.pw.toml", "a", "", &"a".repeat(4097)] {
            assert!(resolve(&project, &queries(&["assets", value])).is_err());
        }
        assert!(
            resolve(
                &project,
                &NonEmpty::new(vec![RemovalSelector::Key(
                    DependencyKey::parse("ASSETS").unwrap()
                )])
                .unwrap()
            )
            .is_err()
        );
    }
    #[test]
    fn provider_identity_is_qualified_and_titles_never_override_exact_keys() {
        let project = crate::engine::addition::tests::fixture(
            &[
                ("alias", "Project1", "Version1"),
                ("other", "Project2", "Version2"),
            ],
            &["alias", "other"],
            &[],
            true,
        );
        let canonical = ProviderProjectId::Modrinth(ModrinthProjectId::parse("Project1").unwrap());
        assert_eq!(
            resolve(
                &project,
                &NonEmpty::new(vec![
                    RemovalSelector::Provider(canonical),
                    RemovalSelector::Query("modrinth:Project1".into())
                ])
                .unwrap()
            )
            .unwrap()
            .locked,
            vec![DependencyKey::parse("alias").unwrap()]
        );
        assert!(resolve(&project, &queries(&["modrinth:slug"])).is_err());
        let mut lock = project.lock().clone();
        for dependency in lock.dependencies.values_mut() {
            dependency.title = "shared title".into();
        }
        let project = ResolvedProject::validate(
            project.intent().clone(),
            lock,
            project.lock().intent_revision,
        )
        .unwrap();
        assert!(matches!(
            resolve(&project, &queries(&["shared title"]))
                .unwrap_err()
                .downcast_ref::<SelectionError>(),
            Some(SelectionError::Ambiguous { .. })
        ));
        assert_eq!(
            resolve(&project, &queries(&["other"])).unwrap().locked,
            vec![DependencyKey::parse("other").unwrap()]
        );
    }
}
