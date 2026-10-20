//! Local authoring explanations share codec normalization and lock satisfaction rules.
use super::*;
use serde::Serialize;

/// Metadata evidence only. No provider freshness, payload verification or native mutation.
#[derive(Debug, Serialize)]
pub struct AuthoringInspection {
    pub schema: u64,
    pub author_schema: u64,
    pub pack: String,
    pub lock_status: &'static str,
    pub lock_reason: Option<String>,
    pub runtime_satisfied: Option<bool>,
    pub dependencies: Vec<DependencyInspection>,
}
#[derive(Debug, Serialize)]
pub struct DependencyInspection {
    pub key: String,
    pub explicit: bool,
    /// Full normalized request for inspection; these defaults are not written to the manifest.
    pub effective: Option<Value>,
    pub canonical_project: Option<Value>,
    pub source_binding_current: Option<bool>,
    pub locked_selection: Option<Value>,
    pub release_policy: Option<&'static str>,
    pub policy_origin: Option<&'static str>,
    pub selection_satisfied: Option<bool>,
    pub reason: Option<String>,
}
impl DocumentCodec {
    /// Inspect current authoring against retained exact metadata without network access.
    pub fn inspect_authoring(
        &self,
        source: &DecodedIntent,
        prior: Option<&DecodedLock>,
    ) -> AuthoringInspection {
        let intent = source.intent();
        let normalized = intent::encode(intent);
        let project_policy_authored =
            parse(source.original()).is_ok_and(|value| value.get("dependency-policy").is_some());
        let bound = prior.map(|prior| prior.bind(source));
        let mut keys: BTreeSet<_> = intent.roots.keys().cloned().collect();
        if let Some(prior) = prior {
            keys.extend(prior.lock().dependencies.keys().cloned());
        }
        AuthoringInspection {
            schema: 1,
            author_schema: INTENT_SCHEMA,
            pack: intent.metadata.name.clone(),
            lock_status: match &bound { None => "missing", Some(Ok(_)) => "current", Some(Err(_)) => "stale" },
            lock_reason: bound.and_then(|value| value.err()).map(|error| format!("{error:#}")),
            runtime_satisfied: prior.map(|prior| empack_core::synchronization::runtime_satisfies(&intent.runtime, &prior.lock().runtime) && intent.runtime.acceptable_versions == prior.lock().acceptable_versions),
            dependencies: keys.into_iter().map(|key| {
                let root = intent.roots.get(&key);
                let locked = prior.and_then(|prior| prior.lock().dependencies.get(&key));
                let validation = root.map(|root| locked.ok_or_else(|| ModelError("No recorded selection".into())).and_then(|locked| root.validate_selection(&key, locked)));
                DependencyInspection {
                    key: key.as_str().into(),
                    explicit: root.is_some(),
                    effective: root.map(|_| normalized["dependencies"][key.as_str()].clone()),
                    canonical_project: root.and_then(|root| root.source.canonical_project(locked)).map(provider_value)
                        .or_else(|| locked.and_then(|locked| match &locked.identity { ResolvedIdentity::Provider(id) => Some(provider_value(id)), _ => None })),
                    source_binding_current: root.filter(|root| matches!(root.source, SourceIntent::ProviderSelector(_))).map(|root| root.source.canonical_project(locked).is_some()),
                    locked_selection: locked.and_then(|locked| locked.selected.as_ref()).map(|selected| pin_value(&selected.selection)),
                    release_policy: root.filter(|root| !matches!(root.selection, DependencySelection::ProviderPin(_)) && matches!(root.source, SourceIntent::Provider(_) | SourceIntent::ProviderSelector(_) | SourceIntent::Search { .. })).map(|_| intent.release_policy(Some(&key), None).as_str()),
                    policy_origin: root.filter(|root| !matches!(root.selection, DependencySelection::ProviderPin(_)) && matches!(root.source, SourceIntent::Provider(_) | SourceIntent::ProviderSelector(_) | SourceIntent::Search { .. })).map(|root| if root.releases.is_some() { "dependency" } else { if project_policy_authored { "project" } else { "schema-default" } }),
                    selection_satisfied: validation.as_ref().map(Result::is_ok),
                    reason: validation.and_then(Result::err).map(|error| error.to_string()).or_else(|| root.is_none().then(|| "Retained lock entry; absence from roots is not deletion authority".into())),
                }
            }).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inspection_distinguishes_missing_stale_and_satisfied_constraints() {
        let project = crate::engine::addition::tests::fixture(
            &[("root", "Project1", "Version1")],
            &["root"],
            &[],
            true,
        );
        let source = DocumentCodec
            .decode_intent(
                &DocumentCodec.encode_intent(project.intent()).unwrap(),
                "source",
            )
            .unwrap();
        let lock = DocumentCodec
            .decode_prior_lock(&DocumentCodec.encode_lock(&project).unwrap(), "lock")
            .unwrap();
        let missing = DocumentCodec.inspect_authoring(&source, None);
        assert_eq!(missing.lock_status, "missing");
        assert_eq!(missing.dependencies[0].selection_satisfied, Some(false));
        let current = DocumentCodec.inspect_authoring(&source, Some(&lock));
        assert_eq!(current.lock_status, "current");
        assert!(
            current.dependencies[0]
                .effective
                .as_ref()
                .unwrap()
                .is_object()
        );
        assert_eq!(current.dependencies[0].selection_satisfied, Some(true));
        let mut intent = source.intent().clone();
        intent.metadata.description = Some("Only display metadata changed".into());
        let changed = DocumentCodec
            .decode_intent(&DocumentCodec.encode_intent(&intent).unwrap(), "changed")
            .unwrap();
        let report = DocumentCodec.inspect_authoring(&changed, Some(&lock));
        assert_eq!(report.lock_status, "stale");
        assert_eq!(report.dependencies[0].selection_satisfied, Some(true));
        assert_eq!(report.runtime_satisfied, Some(true));
        intent.roots.values_mut().next().unwrap().selection = DependencySelection::ProviderPin(
            PinSelector::ModrinthVersion(ModrinthVersionId::parse("Version2").unwrap()),
        );
        let changed = DocumentCodec
            .decode_intent(&DocumentCodec.encode_intent(&intent).unwrap(), "changed")
            .unwrap();
        let report = DocumentCodec.inspect_authoring(&changed, Some(&lock));
        assert_eq!(report.dependencies[0].selection_satisfied, Some(false));
        assert!(
            report.dependencies[0]
                .reason
                .as_ref()
                .unwrap()
                .contains("pin")
        );
    }
}
