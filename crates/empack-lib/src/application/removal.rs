//! Resolve logical removal selectors before granting any mutation authority.
use crate::empack::config::{Dependency, DependencyEntry, DependencyRecord};
use crate::empack::installed::{DependencyIdentity, InstalledDependency};
use anyhow::{Result, ensure};
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone)]
pub struct RemovalPlan {
    pub query: String,
    pub manifest_key: Option<String>,
    pub entry: Option<DependencyEntry>,
    pub installed: Option<InstalledDependency>,
}

fn identity(record: &DependencyRecord) -> DependencyIdentity {
    DependencyIdentity {
        platform: record.platform,
        project_id: record.project_id.clone(),
        project_type: record.project_type,
    }
}

pub fn plan_removals(
    queries: &[String],
    manifest: &BTreeMap<String, DependencyEntry>,
    installed: &[InstalledDependency],
) -> Result<Vec<RemovalPlan>> {
    let mut plans = Vec::new();
    let mut selected = HashSet::new();
    for query in queries {
        let exact = manifest.get_key_value(query);
        let titles: Vec<_> = manifest
            .iter()
            .filter(|(_, entry)| entry.title().eq_ignore_ascii_case(query))
            .collect();
        let stems: Vec<_> = installed
            .iter()
            .filter(|entry| entry.key == *query)
            .collect();
        ensure!(stems.len() <= 1, "Ambiguous installed name: {query}");
        let by_stem: Vec<_> = stems
            .first()
            .into_iter()
            .flat_map(|observed| {
                manifest.iter().filter(move |(_, entry)| match entry {
                    DependencyEntry::Resolved(record) => {
                        observed.identity.as_ref() == Some(&identity(record))
                    }
                    _ => false,
                })
            })
            .collect();
        let chosen = if let Some(exact) = exact {
            Some(exact)
        } else {
            let mut candidates = titles;
            for candidate in by_stem {
                if !candidates.iter().any(|(key, _)| *key == candidate.0) {
                    candidates.push(candidate);
                }
            }
            ensure!(
                candidates.len() <= 1,
                "Ambiguous removal selector '{query}'; use an exact manifest key"
            );
            candidates.into_iter().next()
        };
        let observed = match chosen.map(|(_, entry)| entry) {
            Some(DependencyEntry::Resolved(record)) => {
                let id = identity(record);
                ensure!(manifest.values().filter(|entry| matches!(entry, DependencyEntry::Resolved(r) if identity(r) == id)).count() == 1,
                    "Multiple manifest entries declare the selected identity: {query}");
                let matches: Vec<_> = installed
                    .iter()
                    .filter(|entry| entry.identity.as_ref() == Some(&id))
                    .collect();
                ensure!(
                    matches.len() <= 1,
                    "Multiple installed files declare the selected identity: {query}"
                );
                matches.into_iter().next()
            }
            Some(_) => None,
            None => stems.first().copied(),
        };
        ensure!(
            chosen.is_some() || observed.is_some(),
            "No tracked or installed dependency matches '{query}'"
        );
        let token = chosen
            .map(|(key, _)| format!("manifest:{key}"))
            .unwrap_or_else(|| format!("installed:{}", observed.unwrap().key));
        if selected.insert(token) {
            plans.push(RemovalPlan {
                query: query.clone(),
                manifest_key: chosen.map(|(key, _)| key.clone()),
                entry: chosen.map(|(_, entry)| entry.clone()),
                installed: observed.cloned(),
            });
        }
    }
    Ok(plans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::empack::config::DependencyStatus;
    use crate::primitives::{ProjectPlatform, ProjectType};

    fn entry(id: &str) -> DependencyEntry {
        DependencyEntry::Resolved(DependencyRecord {
            status: DependencyStatus::Resolved,
            title: "Shared title".into(),
            platform: ProjectPlatform::Modrinth,
            project_id: id.into(),
            project_type: ProjectType::Mod,
            version: None,
        })
    }
    #[test]
    fn absent_identity_only_removes_intent_even_when_label_collides() {
        let manifest = BTreeMap::from([("alias".into(), entry("P"))]);
        let installed = vec![InstalledDependency {
            key: "alias".into(),
            identity: None,
            version: None,
        }];
        let plans = plan_removals(
            &["alias".into(), "Shared title".into()],
            &manifest,
            &installed,
        )
        .unwrap();
        assert_eq!(plans.len(), 1);
        assert!(plans[0].installed.is_none());
        assert_eq!(plans[0].manifest_key.as_deref(), Some("alias"));
    }
    #[test]
    fn ambiguity_and_unknown_names_fail_before_execution() {
        let manifest = BTreeMap::from([("a".into(), entry("P")), ("b".into(), entry("Q"))]);
        assert!(plan_removals(&["Shared title".into()], &manifest, &[]).is_err());
        assert!(plan_removals(&["a".into(), "unknown".into()], &manifest, &[]).is_err());
        assert!(plan_removals(&["a".into()], &manifest, &[]).is_ok());
    }
    #[test]
    fn duplicate_manifest_identity_fails_closed() {
        let manifest = BTreeMap::from([("a".into(), entry("P")), ("b".into(), entry("P"))]);
        assert!(plan_removals(&["a".into()], &manifest, &[]).is_err());
    }
}
