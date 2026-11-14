//! Exact typed removal selections; display labels never establish an operation target.
use anyhow::{Result, ensure};
use empack_core::{
    identity::ProviderProjectId,
    model::{DependencyKey, NonEmpty, ResolvedIdentity, ResolvedProject},
};
use std::collections::BTreeSet;

#[derive(Debug, Clone)]
pub enum RemovalSelector {
    /// Exact logical key, without fallback to another namespace.
    Key(DependencyKey),
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
        let provider = match selector {
            RemovalSelector::Key(key) => {
                ensure!(
                    dependencies.contains_key(key),
                    SelectionError::Missing(key.as_str().into())
                );
                selected.insert(key.clone());
                continue;
            }
            RemovalSelector::Provider(provider) => provider,
        };
        let query = format!("{provider:?}");
        let candidates: Vec<_> = dependencies
            .iter()
            .filter(|(_, dependency)| {
                dependency.identity == ResolvedIdentity::Provider(provider.clone())
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
    use empack_core::identity::ModrinthProjectId;
    fn keys(values: &[&str]) -> NonEmpty<RemovalSelector> {
        NonEmpty::new(
            values
                .iter()
                .map(|value| RemovalSelector::Key(DependencyKey::parse(value).unwrap()))
                .collect(),
        )
        .unwrap()
    }
    #[test]
    fn exact_keys_ignore_titles_and_unknown_keys_fail_the_batch() {
        let project = project(false, false);
        assert_eq!(
            resolve(&project, &keys(&["assets", "assets"]))
                .unwrap()
                .locked,
            vec![DependencyKey::parse("assets").unwrap()]
        );
        for value in ["a.pw.toml", "a", "ASSETS", "", &"a".repeat(4097)] {
            if let Ok(key) = DependencyKey::parse(value) {
                let request = NonEmpty::new(vec![
                    RemovalSelector::Key(DependencyKey::parse("assets").unwrap()),
                    RemovalSelector::Key(key),
                ])
                .unwrap();
                assert!(resolve(&project, &request).is_err(), "{value}");
            }
        }
    }
    #[test]
    fn provider_identity_is_explicit_and_display_titles_never_select_keys() {
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
                    RemovalSelector::Key(DependencyKey::parse("alias").unwrap())
                ])
                .unwrap()
            )
            .unwrap()
            .locked,
            vec![DependencyKey::parse("alias").unwrap()]
        );
        let mut lock = project.lock().clone();
        for dependency in lock.dependencies.values_mut() {
            dependency.title = "SharedTitle".into();
        }
        let project = ResolvedProject::validate(
            project.intent().clone(),
            lock,
            project.lock().intent_revision,
        )
        .unwrap();
        assert!(matches!(
            resolve(&project, &keys(&["SharedTitle"]))
                .unwrap_err()
                .downcast_ref::<SelectionError>(),
            Some(SelectionError::Missing(_))
        ));
        assert_eq!(
            resolve(&project, &keys(&["other"])).unwrap().locked,
            vec![DependencyKey::parse("other").unwrap()]
        );
    }
}
