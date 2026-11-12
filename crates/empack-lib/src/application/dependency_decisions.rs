//! Human context for unresolved provider relations; retained issues remain typed.
use super::labels;
use crate::engine::providers::{ClosureIssue, ClosureIssueKind};

#[derive(Debug)]
pub(super) struct UnresolvedDependencies {
    pub(super) issues: Vec<ClosureIssue>,
}
impl std::error::Error for UnresolvedDependencies {}
impl std::fmt::Display for UnresolvedDependencies {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Dependency check could not finish")?;
        for issue in self.issues.iter().take(8) {
            write!(f, "\n  {}", labels::pin(&issue.from))?;
            if let Some(dependency) = &issue.dependency {
                if let Some(project) = &dependency.project {
                    write!(f, " -> {}", labels::project(project))?;
                }
                if let Some(pin) = &dependency.pin {
                    let version = match pin {
                        empack_core::identity::PinSelector::ModrinthVersion(id) => id.to_string(),
                        empack_core::identity::PinSelector::CurseForgeFile(id) => id.to_string(),
                    };
                    write!(f, " (version {version})")?;
                }
                // Raw provider filenames can contain locators or unrelated text.
                // Keep them in the typed issue rather than echoing them here.
            }
            f.write_str(": ")?;
            match &issue.kind {
                ClosureIssueKind::IncompleteEvidence(coverage) => write!(
                    f,
                    "required-dependency information is {}",
                    labels::coverage(*coverage)
                )?,
                ClosureIssueKind::MissingIdentity => {
                    f.write_str("a required dependency has no project or version identity")?
                }
                ClosureIssueKind::AmbiguousContentKind => {
                    f.write_str("the dependency's content type needs an explicit choice")?
                }
                ClosureIssueKind::SelectionConflict { selected, required } => {
                    let required = empack_core::model::ResolvedPin {
                        project: selected.project.clone(),
                        selection: required.clone(),
                    };
                    write!(
                        f,
                        "selected {}; requires {}. Choose compatible versions together",
                        labels::pin(selected),
                        labels::pin(&required)
                    )?;
                }
                ClosureIssueKind::IncompatibleRequirement { required } => write!(
                    f,
                    "required {} does not match the requested Minecraft/loader settings",
                    labels::pin(required)
                )?,
                ClosureIssueKind::UninterpretedRelation => {
                    f.write_str("the provider relationship needs review before continuing")?
                }
            }
        }
        if self.issues.len() > 8 {
            write!(
                f,
                "\n  And {} more dependency issues",
                self.issues.len() - 8
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use empack_core::{
        identity::{ModrinthProjectId, ProviderProjectId},
        model::{Coverage, ResolvedPin},
    };
    fn pin(version: &str) -> ResolvedPin {
        let project = ProviderProjectId::Modrinth(ModrinthProjectId::parse("Project1").unwrap());
        ResolvedPin {
            selection: project.parse_pin(version).unwrap(),
            project,
        }
    }
    #[test]
    fn unresolved_relations_retain_exact_selections_without_debug_tuples() {
        let selected = pin("Version1");
        let required = pin("Version2");
        let error = UnresolvedDependencies {
            issues: vec![
                ClosureIssue {
                    from: selected.clone(),
                    dependency: None,
                    kind: ClosureIssueKind::SelectionConflict {
                        selected: selected.clone(),
                        required: required.selection.clone(),
                    },
                },
                ClosureIssue {
                    from: selected.clone(),
                    dependency: None,
                    kind: ClosureIssueKind::IncompleteEvidence(Coverage::Unknown),
                },
                ClosureIssue {
                    from: selected,
                    dependency: None,
                    kind: ClosureIssueKind::IncompatibleRequirement { required },
                },
            ],
        };
        let text = error.to_string();
        assert!(
            text.contains(
                "selected modrinth:Project1@Version1; requires modrinth:Project1@Version2"
            ),
            "{text}"
        );
        assert!(text.contains("required-dependency information is not recorded"));
        assert!(text.contains("does not match the requested Minecraft/loader settings"));
        assert!(!text.contains("SelectionConflict"));
        assert!(!text.contains("Some("));
        let retained = anyhow::Error::new(error).context("Requested dependencies were not saved");
        assert_eq!(
            retained
                .downcast_ref::<UnresolvedDependencies>()
                .unwrap()
                .issues
                .len(),
            3
        );
    }
    #[test]
    fn large_error_summaries_do_not_discard_the_other_typed_issues() {
        let error = UnresolvedDependencies {
            issues: (0..12)
                .map(|_| ClosureIssue {
                    from: pin("Version1"),
                    dependency: None,
                    kind: ClosureIssueKind::MissingIdentity,
                })
                .collect(),
        };
        assert!(error.to_string().ends_with("And 4 more dependency issues"));
        assert_eq!(error.issues.len(), 12);
    }
}
