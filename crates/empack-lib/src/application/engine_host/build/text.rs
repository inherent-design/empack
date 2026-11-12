//! Build presentation uses the prepared recipe; it does not resolve a second plan.
use super::*;
use crate::engine::api::{BuildPreview, ContentRequirementKind};
use crate::terminal::display_line;

pub(super) fn recipe(value: Recipe) -> String {
    let consumer = match value.consumer() {
        Consumer::Modrinth => "Modrinth",
        Consumer::CurseForge => "CurseForge",
        Consumer::Prism => "Prism",
        Consumer::Server => "Server",
        Consumer::Empack => "empack",
    };
    let environment = match value.environments() {
        Environments::Client => "client",
        Environments::Server => "server",
        Environments::Both => "client and server",
    };
    let delivery = match value.delivery() {
        Delivery::References => "reference downloads",
        Delivery::Bundled => "bundled files",
    };
    let updates = match value.update_authority() {
        UpdateAuthority::Snapshot => "fixed snapshot",
        UpdateAuthority::Platform => "platform updates (project association required)",
        UpdateAuthority::Empack => "empack updates (channel enrollment required)",
    };
    let target = if value.consumer() == Consumer::Server {
        consumer.to_owned()
    } else {
        format!("{consumer} ({environment})")
    };
    format!("{target}, {delivery}, {updates}")
}

pub(super) fn optional(value: &OptionalPolicy) -> String {
    match value {
        OptionalPolicy::Preserve => "keep choices for the recipient".into(),
        OptionalPolicy::Resolve {
            choices,
            use_defaults,
        } => {
            let mut values: Vec<_> = choices
                .iter()
                .map(|(key, enabled)| {
                    format!(
                        "{}={}",
                        display_line(key),
                        if *enabled { "on" } else { "off" }
                    )
                })
                .collect();
            values.push(
                if *use_defaults {
                    "use project defaults for other choices"
                } else {
                    "require an explicit value for every choice"
                }
                .into(),
            );
            values.join("; ")
        }
    }
}

pub(super) fn requirement(kind: ContentRequirementKind) -> &'static str {
    match kind {
        ContentRequirementKind::Download => "download needed",
        ContentRequirementKind::ProviderLookup => "provider lookup needed",
        ContentRequirementKind::Embedded => "archive member needed",
        ContentRequirementKind::Manual => "supply this file",
    }
}

pub(super) fn saved(files: usize) -> String {
    if files == 0 {
        "Saved build. Continue with empack build --continue.".into()
    } else {
        format!(
            "Saved build with {files} verified {}. Continue with empack build --continue.",
            if files == 1 { "file" } else { "files" }
        )
    }
}

pub(super) fn preview(view: &BuildPreview) -> String {
    let loader = crate::application::labels::loader(view.runtime.loader);
    let mut lines = vec![format!(
        "Build plan: Minecraft {} ({loader})",
        view.runtime.minecraft.as_str()
    )];
    for output in &view.outputs {
        lines.push(format!("  dist/{}", display_line(output.artifact.as_str())));
        lines.push(format!("    {}", recipe(output.target)));
        if let OptionalPolicy::Resolve {
            choices,
            use_defaults,
        } = view.request().optional_for(output.target)
            && (!choices.is_empty() || !use_defaults)
        {
            lines.push(format!(
                "    Optional files: {}",
                optional(view.request().optional_for(output.target))
            ));
        }
    }
    if view.needs_network {
        lines.push("Network access is needed.".into());
    }
    if view.runs_installer {
        lines.push("This build runs the selected Minecraft/loader installer.".into());
    }
    if view.request().evidence == SourceEvidencePolicy::StrongSourceRequired {
        lines.push("Source checks require declared SHA-256 or SHA-512 checksums.".into());
    }
    if view.request().mrpack_optional == OptionalConversion::AcknowledgedMetadataLoss
        && view
            .outputs
            .iter()
            .any(|output| output.target.consumer() == Consumer::Modrinth)
    {
        lines.push(
            "Mrpack conversion may omit optional-choice names, defaults and descriptions.".into(),
        );
    }
    if !view.unresolved.is_empty() {
        lines.push(format!("Unresolved files: {}", view.unresolved.len()));
        for input in view
            .content
            .iter()
            .filter(|input| view.unresolved.contains(&input.key))
        {
            lines.push(format!(
                "  {}: {}",
                display_line(&input_selector(&input.key)),
                requirement(input.kind)
            ));
            if let Some(names) = view.file_names().get(&input.key) {
                for name in names {
                    lines.push(format!("    Filename: {}", display_line(name)));
                }
            }
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn delivery_and_update_authority_are_distinct_in_recipe_explanations() {
        assert_eq!(
            recipe(Recipe::PRISM_REFERENCES),
            "Prism (client), reference downloads, fixed snapshot"
        );
        assert_eq!(
            recipe(Recipe::SERVER_BUNDLED),
            "Server, bundled files, fixed snapshot"
        );
        assert_eq!(
            recipe(
                Recipe::PRISM_BUNDLED
                    .with_update_authority(UpdateAuthority::Empack)
                    .unwrap()
            ),
            "Prism (client), bundled files, empack updates (channel enrollment required)"
        );
    }
    #[test]
    fn explicit_choices_and_default_policy_survive_without_debug_output() {
        let policy = OptionalPolicy::Resolve {
            choices: BTreeMap::from([("日本 extras".into(), false)]),
            use_defaults: false,
        };
        assert_eq!(
            optional(&policy),
            "日本 extras=off; require an explicit value for every choice"
        );
        assert_eq!(
            optional(&OptionalPolicy::Preserve),
            "keep choices for the recipient"
        );
    }
}
