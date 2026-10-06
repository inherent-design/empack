//! User selectors nominate logical selections; they never authorize backend filenames.
use crate::engine::backend::BackendFile;
use anyhow::{Result, ensure};
use empack_core::model::{DependencyKey, NonEmpty, ResolvedProject};
use std::collections::BTreeSet;

#[derive(Debug, Clone)]
pub enum RemovalSelector {
    /// Select this logical key without falling back to another namespace.
    Key(DependencyKey),
    /// Exact logical key first, otherwise an ASCII-case-insensitive title or exact metadata stem.
    Query(String),
}

#[derive(Debug, thiserror::Error)]
pub enum SelectionError {
    #[error("No locked dependency matches '{0}'")]
    Missing(String),
    #[error("Ambiguous removal selector '{query}'; use an exact logical key")]
    Ambiguous {
        query: String,
        candidates: Vec<DependencyKey>,
        untracked: bool,
    },
    #[error(
        "Installed name '{0}' has no locked owner; explicit observed-content removal is required"
    )]
    Untracked(String),
}

pub(super) fn resolve(
    project: &ResolvedProject,
    records: &[BackendFile],
    selectors: &NonEmpty<RemovalSelector>,
) -> Result<NonEmpty<DependencyKey>> {
    let dependencies = &project.lock().dependencies;
    let mut selected = BTreeSet::new();
    for selector in selectors.as_slice() {
        let query = match selector {
            RemovalSelector::Key(key) => {
                ensure!(
                    dependencies.contains_key(key),
                    SelectionError::Missing(key.as_str().into())
                );
                selected.insert(key.clone());
                continue;
            }
            RemovalSelector::Query(query) => query,
        };
        ensure!(
            !query.trim().is_empty() && query.len() <= 4096,
            "Removal selector must contain 1–4096 bytes"
        );
        if let Some(key) = dependencies.keys().find(|key| key.as_str() == query) {
            selected.insert(key.clone());
            continue;
        }
        let mut candidates: BTreeSet<_> = dependencies
            .iter()
            .filter(|(_, value)| value.title.eq_ignore_ascii_case(query))
            .map(|(key, _)| key.clone())
            .collect();
        let mut untracked = false;
        for record in records.iter().filter(|record| {
            record
                .metadata_path
                .as_str()
                .rsplit('/')
                .next()
                .and_then(|name| name.strip_suffix(".pw.toml"))
                == Some(query.as_str())
        }) {
            match record.locked_owner(project)? {
                Some((key, _)) => {
                    candidates.insert(key.clone());
                }
                None => untracked = true,
            }
        }
        if candidates.len() > 1 || (untracked && !candidates.is_empty()) {
            return Err(SelectionError::Ambiguous {
                query: query.clone(),
                candidates: candidates.into_iter().collect(),
                untracked,
            }
            .into());
        }
        if untracked {
            return Err(SelectionError::Untracked(query.clone()).into());
        }
        let key = candidates
            .into_iter()
            .next()
            .ok_or_else(|| SelectionError::Missing(query.clone()))?;
        selected.insert(key);
    }
    Ok(NonEmpty::new(selected.into_iter().collect())?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{documents::DocumentCodec, mrpack::tests::project};
    use empack_core::{
        model::*,
        path::{InstallDestination, PathSyntax, PortableRelPath},
    };

    fn key(value: &str) -> DependencyKey {
        DependencyKey::parse(value).unwrap()
    }
    fn queries(values: &[&str]) -> NonEmpty<RemovalSelector> {
        NonEmpty::new(
            values
                .iter()
                .map(|value| RemovalSelector::Query((*value).into()))
                .collect(),
        )
        .unwrap()
    }
    fn record(stem: &str, file: &str) -> BackendFile {
        BackendFile::parse(
            PortableRelPath::parse(&format!("resourcepacks/{stem}.pw.toml"), PathSyntax::ProjectContent).unwrap(),
            format!("filename = '{file}'\nside = 'client'\n[download]\nurl = 'https://example.com/file'\nhash-format = 'sha256'\nhash = '{}'", "00".repeat(32)).as_bytes(),
        ).unwrap()
    }
    fn pair() -> ResolvedProject {
        let original = project(false, false);
        let mut intent = original.intent().clone();
        intent
            .roots
            .insert(key("second"), intent.roots[&key("assets")].clone());
        let decoded = DocumentCodec
            .decode_intent(&DocumentCodec.encode_intent(&intent).unwrap(), "fixture")
            .unwrap();
        let mut lock = original.lock().clone();
        let mut dependency = lock.dependencies[&key("assets")].clone();
        dependency.identity = ResolvedIdentity::Url(key("second"));
        dependency.title = "Second title".into();
        let mut files = dependency.files.as_slice().to_vec();
        for file in &mut files {
            let mut placements = file.placements.as_slice().to_vec();
            for placement in &mut placements {
                placement.destination = InstallDestination::parse(&format!(
                    "second/{}",
                    placement.destination.relative().as_str()
                ))
                .unwrap();
            }
            file.placements = NonEmpty::new(placements).unwrap();
        }
        dependency.files = NonEmpty::new(files).unwrap();
        lock.dependencies.insert(key("second"), dependency);
        lock.coverage.insert(key("second"), Coverage::Unknown);
        lock.intent_revision = decoded.semantic_revision();
        ResolvedProject::validate(intent, lock, decoded.semantic_revision()).unwrap()
    }
    #[test]
    fn aliases_titles_and_stems_deduplicate_to_one_logical_selection() {
        let project = project(false, false);
        let records = [record("installed-name", "a.zip")];
        assert_eq!(
            resolve(
                &project,
                &records,
                &queries(&["assets", "ASSETS", "installed-name"])
            )
            .unwrap()
            .as_slice(),
            &[key("assets")]
        );
        assert!(matches!(
            resolve(&project, &records, &queries(&["installed-NAME"]))
                .unwrap_err()
                .downcast_ref(),
            Some(SelectionError::Missing(_))
        ));
    }
    #[test]
    fn exact_key_wins_even_when_a_different_installation_uses_that_stem() {
        let project = pair();
        let records = [record("second", "a.zip")];
        assert_eq!(
            resolve(&project, &records, &queries(&["second"]))
                .unwrap()
                .as_slice(),
            &[key("second")]
        );
        let records = [record("assets", "untracked.zip")];
        assert_eq!(
            resolve(&project, &records, &queries(&["assets"]))
                .unwrap()
                .as_slice(),
            &[key("assets")]
        );
    }
    #[test]
    fn title_stem_conflicts_report_both_choices_without_guessing() {
        let project = pair();
        let records = [record("Second title", "a.zip")];
        let error = resolve(&project, &records, &queries(&["Second title"])).unwrap_err();
        let Some(SelectionError::Ambiguous {
            candidates,
            untracked,
            ..
        }) = error.downcast_ref()
        else {
            panic!("{error:#}")
        };
        assert_eq!(candidates, &[key("assets"), key("second")]);
        assert!(!untracked);
        let records = [record("Assets", "untracked.zip")];
        assert!(matches!(
            resolve(&project, &records, &queries(&["Assets"]))
                .unwrap_err()
                .downcast_ref(),
            Some(SelectionError::Ambiguous {
                untracked: true,
                ..
            })
        ));
    }
    #[test]
    fn unknown_or_untracked_selection_cannot_return_a_successful_subset() {
        let project = project(false, false);
        let records = [record("untracked", "untracked.zip")];
        assert!(matches!(
            resolve(&project, &records, &queries(&["assets", "untracked"]))
                .unwrap_err()
                .downcast_ref(),
            Some(SelectionError::Untracked(_))
        ));
        assert!(resolve(&project, &records, &queries(&["assets", "missing"])).is_err());
        assert!(resolve(&project, &records, &queries(&[""])).is_err());
        assert!(resolve(&project, &records, &queries(&[&"a".repeat(4097)])).is_err());
        let exact = NonEmpty::new(vec![RemovalSelector::Key(key("ASSETS"))]).unwrap();
        assert!(resolve(&project, &records, &exact).is_err());
    }
    #[test]
    fn conflicting_backend_requirements_cannot_nominate_a_locked_owner() {
        let project = project(false, false);
        let mut record = record("installed-name", "a.zip");
        record.environments = empack_core::requirements::Environments::Both;
        assert!(resolve(&project, &[record], &queries(&["installed-name"])).is_err());
    }
}
