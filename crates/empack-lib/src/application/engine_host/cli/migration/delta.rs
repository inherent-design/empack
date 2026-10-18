//! Semantic migration reporting covers every shipped lock entry, including retained libraries.
use super::*;
use crate::engine::providers::ObservedVersion;
use serde::Serialize;
use std::collections::{BTreeSet, VecDeque};

#[derive(Serialize)]
pub(super) struct MigrationDelta {
    schema: u32,
    current_runtime: RuntimeState,
    proposed_runtime: RuntimeState,
    dependencies: Vec<DependencyDelta>,
    limitations: &'static str,
}
#[derive(Serialize)]
struct RuntimeState {
    minecraft: String,
    loader: String,
    loader_version: Option<String>,
    acceptable_versions: Vec<String>,
}
impl RuntimeState {
    fn from_lock(lock: &ResolutionLock) -> Self {
        Self {
            minecraft: lock.runtime.minecraft.as_str().into(),
            loader: format!("{:?}", lock.runtime.loader).to_lowercase(),
            loader_version: lock
                .runtime
                .loader_version
                .as_ref()
                .map(|v| v.as_str().into()),
            acceptable_versions: lock
                .acceptable_versions
                .iter()
                .map(|v| v.as_str().into())
                .collect(),
        }
    }
    fn text(&self) -> String {
        format!(
            "{} / {} {} (accepted: {})",
            self.minecraft,
            self.loader,
            self.loader_version.as_deref().unwrap_or(""),
            self.acceptable_versions.join(", ")
        )
    }
}
#[derive(Serialize)]
struct DependencyDelta {
    key: String,
    change: &'static str,
    requirement: &'static str,
    source_replaced: bool,
    manual_compatibility: bool,
    before: Option<DependencyState>,
    after: Option<DependencyState>,
}
#[derive(Serialize)]
struct DependencyState {
    title: String,
    role: &'static str,
    kind: String,
    selection: Option<ObservedVersion>,
    placements: Vec<PlacementState>,
    reason_chain: Option<Vec<String>>,
    dependency_evidence: &'static str,
}
#[derive(Serialize)]
struct PlacementState {
    file: String,
    destination: String,
    layer: &'static str,
    client: RequirementState,
    server: RequirementState,
}
#[derive(Serialize)]
struct RequirementState {
    state: &'static str,
    choice: Option<String>,
    default_enabled: Option<bool>,
    description: Option<String>,
}
impl RequirementState {
    fn new(value: &Requirement) -> Self {
        match value {
            Requirement::Optional(choice) => Self {
                state: "optional",
                choice: Some(choice.key.as_str().into()),
                default_enabled: Some(choice.default_enabled),
                description: choice.description.clone(),
            },
            Requirement::Required | Requirement::Unsupported => Self {
                state: if matches!(value, Requirement::Required) {
                    "required"
                } else {
                    "unsupported"
                },
                choice: None,
                default_enabled: None,
                description: None,
            },
        }
    }
    fn text(&self) -> String {
        self.choice.as_ref().map_or_else(
            || self.state.into(),
            |choice| {
                format!(
                    "optional:{choice} (default={}; {})",
                    self.default_enabled.unwrap_or(false),
                    self.description.as_deref().unwrap_or("")
                )
            },
        )
    }
}
// One shortest known path per entry avoids enumerating all paths through cyclic graphs.
fn reason_chains(
    intent: &ProjectIntent,
    lock: &ResolutionLock,
) -> BTreeMap<DependencyKey, Vec<String>> {
    let mut paths = BTreeMap::new();
    let mut queue = VecDeque::new();
    for key in intent.roots.keys() {
        paths.insert(key.clone(), vec![key.as_str().into()]);
        queue.push_back(key.clone());
    }
    while let Some(key) = queue.pop_front() {
        for child in lock.required_edges.get(&key).into_iter().flatten() {
            if !paths.contains_key(child) {
                let mut path = paths[&key].clone();
                path.push(child.as_str().into());
                paths.insert(child.clone(), path);
                queue.push_back(child.clone());
            }
        }
    }
    paths
}
fn state(
    key: &DependencyKey,
    intent: &ProjectIntent,
    lock: &ResolutionLock,
    chains: &BTreeMap<DependencyKey, Vec<String>>,
) -> Option<DependencyState> {
    lock.dependencies
        .get(key)
        .map(|dependency| DependencyState {
            title: dependency.title.clone(),
            role: if intent.roots.contains_key(key) {
                "root"
            } else {
                "transitive"
            },
            kind: format!("{:?}", dependency.kind).to_lowercase(),
            selection: dependency.selected.as_ref().map(ObservedVersion::from_pin),
            placements: dependency
                .files
                .as_slice()
                .iter()
                .flat_map(|file| {
                    file.placements
                        .as_slice()
                        .iter()
                        .map(|placement| PlacementState {
                            file: file.slot.as_str().into(),
                            destination: placement.destination.relative().as_str().into(),
                            layer: match placement.layer {
                                ContentLayer::Common => "common",
                                ContentLayer::CommonOverride => "common-override",
                                ContentLayer::Client => "client",
                                ContentLayer::Server => "server",
                            },
                            client: RequirementState::new(&placement.requirements.client),
                            server: RequirementState::new(&placement.requirements.server),
                        })
                })
                .collect(),
            reason_chain: chains.get(key).cloned(),
            dependency_evidence: match lock.coverage.get(key) {
                Some(Coverage::CompleteForSelection) => "complete",
                Some(Coverage::Partial) => "partial",
                _ => "unknown",
            },
        })
}
impl MigrationDelta {
    pub fn between(
        source: &ProjectIntent,
        prior: &ResolutionLock,
        proposed: &ResolvedProject,
    ) -> Self {
        let old_chains = reason_chains(source, prior);
        let new_chains = reason_chains(proposed.intent(), proposed.lock());
        let complete = new_chains
            .keys()
            .all(|key| proposed.lock().coverage.get(key) == Some(&Coverage::CompleteForSelection));
        let keys: BTreeSet<_> = prior
            .dependencies
            .keys()
            .chain(proposed.lock().dependencies.keys())
            .collect();
        Self {
            schema: 2,
            current_runtime: RuntimeState::from_lock(prior),
            proposed_runtime: RuntimeState::from_lock(proposed.lock()),
            dependencies: keys
                .into_iter()
                .map(|key| {
                    let old = prior.dependencies.get(key);
                    let new = proposed.lock().dependencies.get(key);
                    DependencyDelta {
                        key: key.as_str().into(),
                        change: match (old, new) {
                            (None, _) => "added",
                            (_, None) => "removed",
                            (Some(old), Some(new)) if old == new => "retained",
                            _ => "changed",
                        },
                        requirement: if new.is_none() {
                            "removed"
                        } else if proposed.intent().roots.contains_key(key) {
                            "explicit-root"
                        } else if new_chains.contains_key(key) {
                            if old_chains.contains_key(key) {
                                "required"
                            } else {
                                "newly-required"
                            }
                        } else if complete {
                            "no-longer-required-retained"
                        } else {
                            "requirement-unknown-retained"
                        },
                        source_replaced: match (
                            source.roots.get(key),
                            proposed.intent().roots.get(key),
                        ) {
                            (Some(old), Some(new)) => old.source != new.source,
                            _ => false,
                        },
                        manual_compatibility: new.or(old).is_some_and(|d| d.selected.is_none()),
                        before: state(key, source, prior, &old_chains),
                        after: state(key, proposed.intent(), proposed.lock(), &new_chains),
                    }
                })
                .collect(),
            limitations: "Provider declarations are evidence, not proof of gameplay compatibility. Manual sources require author verification. Unknown dependency coverage cannot prove a retained entry is no longer required. Retained placements still ship; nothing is deleted automatically. This does not port private files, configuration or worlds, validate a full Java compatibility range, or authorize publication. Native conflicts are checked during preparation.",
        }
    }
    pub fn text_lines(&self) -> Vec<String> {
        let mut lines = vec![format!(
            "Migration candidate: {} -> {}",
            self.current_runtime.text(),
            self.proposed_runtime.text()
        )];
        for row in &self.dependencies {
            lines.push(format!(
                "{}: {}, {}{}{}",
                row.key,
                row.change,
                row.requirement,
                if row.source_replaced {
                    "; explicit replacement"
                } else {
                    ""
                },
                if row.manual_compatibility {
                    "; manual compatibility verification required"
                } else {
                    ""
                }
            ));
            for (label, value) in [("before", &row.before), ("after", &row.after)] {
                if let Some(value) = value {
                    let pin = value.selection.as_ref().map_or_else(
                        || "manual source".into(),
                        |v| format!("{}:{}@{}", v.provider, v.project, v.selection),
                    );
                    lines.push(format!(
                        "  {label}: {} [{} / {}] {pin}; dependency evidence: {}",
                        value.title, value.role, value.kind, value.dependency_evidence
                    ));
                    for placement in &value.placements {
                        lines.push(format!(
                            "    {} -> {} [{}; client={}; server={}]",
                            placement.file,
                            placement.destination,
                            placement.layer,
                            placement.client.text(),
                            placement.server.text()
                        ));
                    }
                    lines.push(format!(
                        "    required via: {}",
                        value
                            .reason_chain
                            .as_ref()
                            .map_or_else(|| "no known root path".into(), |path| path.join(" -> "))
                    ));
                } else {
                    lines.push(format!("  {label}: absent"));
                }
            }
        }
        lines.push(self.limitations.into());
        lines
    }
}
