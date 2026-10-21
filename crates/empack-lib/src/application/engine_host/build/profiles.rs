//! Resolve authored profiles once; explanation and execution share the resulting outputs.
use super::*;
use empack_core::model::BuildProfile;
use serde_json::{Value, json};

pub(super) struct Selected {
    pub name: String,
    pub origin: &'static str,
    pub profile: BuildProfile,
}
fn consumer(name: &str) -> Result<Recipe> {
    Ok(match name {
        "modrinth" => Recipe::MODRINTH,
        "curseforge" => Recipe::CURSEFORGE,
        "prism" => Recipe::PRISM_REFERENCES,
        "server" => Recipe::SERVER_REFERENCES,
        "empack" => Recipe::EMPACK_REFERENCES,
        _ => anyhow::bail!("Unknown build consumer: {name}"),
    })
}
pub(super) fn select(intent: &ProjectIntent, args: &BuildArgs) -> Result<Vec<Selected>> {
    intent.distribution.validate()?;
    ensure!(
        args.profiles.is_empty() || (args.targets.is_empty() && !args.ad_hoc),
        "Select profiles or consumers, not both"
    );
    ensure!(
        !args.ad_hoc || !args.targets.is_empty(),
        "--ad-hoc requires a consumer"
    );
    let configured = |name: &String, origin| -> Result<Selected> {
        Ok(Selected {
            name: name.clone(),
            origin,
            profile: intent
                .distribution
                .profiles
                .get(name)
                .with_context(|| format!("Unknown build profile '{name}'"))?
                .clone(),
        })
    };
    let mut selected = if !args.profiles.is_empty() {
        args.profiles
            .iter()
            .map(|name| configured(name, "profile"))
            .collect::<Result<Vec<_>>>()?
    } else if args.targets.is_empty() {
        intent
            .distribution
            .defaults
            .as_slice()
            .iter()
            .map(|name| configured(name, "default-profile"))
            .collect::<Result<Vec<_>>>()?
    } else {
        args.targets.iter().map(|name| {
            let builtin = consumer(name)?;
            if args.ad_hoc {
                return Ok(Selected { name: name.clone(), origin: "ad-hoc", profile: BuildProfile { recipe: builtin, archive: DistributionArchive::Zip } });
            }
            let matches: Vec<_> = intent.distribution.profiles.iter().filter(|(_, profile)| profile.recipe.consumer() == builtin.consumer()).collect();
            ensure!(!matches.is_empty(), "No configured profile for '{name}'; choose --profile NAME or explicitly use --ad-hoc {name}");
            ensure!(matches.len() == 1, "Multiple profiles use '{name}'; select --profile NAME");
            configured(matches[0].0, "consumer-profile")
        }).collect::<Result<Vec<_>>>()?
    };
    let mut seen = std::collections::BTreeSet::new();
    selected.retain(|item| seen.insert((item.name.clone(), item.origin)));
    ensure!(
        !selected.is_empty(),
        "No default build profiles; select --profile NAME or --ad-hoc TARGET"
    );
    for item in &mut selected {
        let recipe = item.profile.recipe;
        let delivery = match args.delivery.as_deref() {
            None => recipe.delivery(),
            Some("references") => Delivery::References,
            Some("bundled") => Delivery::Bundled,
            _ => anyhow::bail!("Unknown delivery"),
        };
        let environment = match args.environment.as_deref() {
            None => recipe.environments(),
            Some("client") => Environments::Client,
            Some("server") => Environments::Server,
            Some("both") => Environments::Both,
            _ => anyhow::bail!("Unknown environment"),
        };
        let updates = match args.updates.as_deref() {
            None => recipe.update_authority(),
            Some("snapshot") => UpdateAuthority::Snapshot,
            Some("platform") => UpdateAuthority::Platform,
            Some("empack") => UpdateAuthority::Empack,
            _ => anyhow::bail!("Unknown update authority"),
        };
        item.profile.recipe = Recipe::new(recipe.consumer(), delivery, environment)?
            .with_update_authority(updates)?;
        item.profile.archive =
            if matches!(recipe.consumer(), Consumer::Modrinth | Consumer::CurseForge) {
                DistributionArchive::Zip
            } else {
                match args.format {
                    None => item.profile.archive,
                    Some(CliArchiveFormat::Zip) => DistributionArchive::Zip,
                    Some(CliArchiveFormat::TarGz) => DistributionArchive::TarGz,
                    Some(CliArchiveFormat::SevenZ) => DistributionArchive::SevenZip,
                }
            };
    }
    Ok(selected)
}
pub(super) fn explain(
    workspace: &crate::engine::project::WorkspaceSnapshot,
    args: &BuildArgs,
    request: &BuildRequest,
) -> Result<Value> {
    let intent = workspace.intent().intent();
    let selected = select(intent, args)?;
    let profiles = selected.iter().zip(request.outputs.as_slice()).map(|(selected, output)| {
        let target = output.target;
        let recipient = target.consumer() == Consumer::Empack || (matches!(target.consumer(), Consumer::Prism | Consumer::Server) && (target.delivery() == Delivery::References || target.update_authority() == UpdateAuthority::Empack));
        let mut origins = BTreeMap::from([("consumer", selected.origin), ("delivery", selected.origin), ("environment", selected.origin), ("updates", selected.origin), ("format", selected.origin)]);
        for (field, changed) in [("delivery", args.delivery.is_some()), ("environment", args.environment.is_some()), ("updates", args.updates.is_some()), ("format", args.format.is_some())] { if changed { origins.insert(field, "cli-override"); } }
        if matches!(target.consumer(), Consumer::Modrinth | Consumer::CurseForge) { origins.insert("format", "consumer-required-zip"); }
        json!({"name":selected.name,"selection":selected.origin,"recipe":crate::engine::documents::recipe::encode_profile(&BuildProfile { recipe: target, archive: output.archive }),"origins":origins,"output":format!("dist/{}",output.artifact.as_str()),"recipient_empack_required":recipient,"java":json!({"minimum_major":intent.runtime.java_major,"meaning":"declared minimum; not a complete compatibility range"}),"runtime_preparation":if target.consumer()==Consumer::Prism { "Prism prepares client game/runtime files" } else if target.consumer()==Consumer::Server { "empack prepares server runtime; recipient supplies Java" } else { "consumer-defined" },"author_acquisition":"May require verified downloads even for references; not assessed by explanation"})
    }).collect::<Vec<_>>();
    let resolved = workspace.require_resolved();
    let manual = resolved.as_ref().ok().map(|project| {
        project
            .lock()
            .dependencies
            .iter()
            .flat_map(|(key, dependency)| {
                dependency
                    .files
                    .as_slice()
                    .iter()
                    .filter(|file| {
                        matches!(
                            file.acquisition,
                            empack_core::model::AcquisitionSpec::Manual { .. }
                        )
                    })
                    .map(move |file| format!("{}/{}", key.as_str(), file.slot.as_str()))
            })
            .collect::<Vec<_>>()
    });
    let optional = match &request.optional {
        OptionalPolicy::Preserve => json!({"mode":"preserve"}),
        OptionalPolicy::Resolve {
            choices,
            use_defaults,
        } => json!({"mode":"resolve", "choices":choices, "use_defaults":use_defaults}),
    };
    Ok(
        json!({"schema":2,"pack_id":intent.metadata.id,"pack":intent.metadata.name,"version":intent.metadata.version,"profiles":profiles,"lock":if resolved.is_ok(){"current"} else if workspace.prior_lock().is_none(){"missing"}else{"stale"},"optional_policy":optional,"declared_manual_inputs":manual,"input_verification":"Not performed; unavailable provider downloads and cache contents remain unknown","clean":request.clean}),
    )
}
pub(super) fn text(report: &Value) -> String {
    let mut lines = vec![format!(
        "Build configuration for {} {} (lock: {})",
        report["pack"].as_str().unwrap(),
        report["version"].as_str().unwrap(),
        report["lock"].as_str().unwrap()
    )];
    for profile in report["profiles"].as_array().unwrap() {
        lines.push(format!(
            "{} [{}] → {}",
            profile["name"].as_str().unwrap(),
            profile["selection"].as_str().unwrap(),
            profile["output"].as_str().unwrap()
        ));
        for field in ["consumer", "delivery", "environment", "updates", "format"] {
            lines.push(format!(
                "  {field}: {} ({})",
                profile["recipe"][field].as_str().unwrap(),
                profile["origins"][field].as_str().unwrap()
            ));
        }
        lines.push(format!(
            "  Recipient empack required: {}; Java: {}",
            profile["recipient_empack_required"], profile["java"]
        ));
        lines.push(format!(
            "  {}",
            profile["runtime_preparation"].as_str().unwrap()
        ));
        lines.push(format!(
            "  {}",
            profile["author_acquisition"].as_str().unwrap()
        ));
    }
    lines.push(format!(
        "Optional choices: {}; known manual inputs: {}",
        report["optional_policy"], report["declared_manual_inputs"]
    ));
    lines.push(report["input_verification"].as_str().unwrap().into());
    lines.join("\n")
}
