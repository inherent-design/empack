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
    let java = empack_core::java::JavaPolicy::for_game(intent.runtime.minecraft.as_str());
    let profiles = selected.iter().zip(request.outputs.as_slice()).map(|(selected, output)| {
        let target = output.target;
        let managed = crate::engine::build::instance_managed(target);
        let recipient = target.consumer() == Consumer::Empack || managed;
        let bootstrap = managed.then(|| json!({"jar":crate::engine::build::bootstrap::PATH,"engine_version":crate::engine::compatibility_version(),"sha256":env!("EMPACK_BOOTSTRAP_SHA256")}));
        let mut origins = BTreeMap::from([("consumer", selected.origin), ("delivery", selected.origin), ("environment", selected.origin), ("updates", selected.origin), ("format", selected.origin)]);
        for (field, changed) in [("delivery", args.delivery.is_some()), ("environment", args.environment.is_some()), ("updates", args.updates.is_some()), ("format", args.format.is_some())] { if changed { origins.insert(field, "cli-override"); } }
        if matches!(target.consumer(), Consumer::Modrinth | Consumer::CurseForge) { origins.insert("format", "consumer-required-zip"); }
        json!({"name":selected.name,"selection":selected.origin,"recipe":crate::engine::documents::recipe::encode_profile(&BuildProfile { recipe: target, archive: output.archive }),"origins":origins,"output":format!("dist/{}",output.artifact.as_str()),"optional_policy":optional_policy(request.optional_for(target)),"recipient_empack_required":recipient,"bootstrap":bootstrap,"java":json!({"preferred_major":intent.runtime.java_major,"minimum_major":java.required_minimum(),"supported_majors":java.supported(),"recommended_major":java.recommended(),"policy":empack_core::java::POLICY_ID,"meaning":"finite admitted policy, not certification of every JVM or mod combination"}),"runtime_preparation":if target.consumer()==Consumer::Prism { "Prism prepares client game/runtime files" } else if target.consumer()==Consumer::Server { "empack prepares server runtime; recipient supplies Java" } else { "consumer-defined" },"author_acquisition":"May require verified downloads even for references; not assessed by explanation"})
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
    let optional = optional_policy(&request.optional);
    Ok(
        json!({"schema":2,"pack_id":intent.metadata.id,"pack":intent.metadata.name,"version":intent.metadata.version,"profiles":profiles,"lock":if resolved.is_ok(){"current"} else if workspace.prior_lock().is_none(){"missing"}else{"stale"},"optional_policy":optional,"declared_manual_inputs":manual,"input_verification":"Not performed; unavailable provider downloads and cache contents remain unknown","clean":request.clean}),
    )
}

fn optional_policy(policy: &OptionalPolicy) -> Value {
    match policy {
        OptionalPolicy::Preserve => json!({"mode":"preserve"}),
        OptionalPolicy::Resolve {
            choices,
            use_defaults,
        } => json!({"mode":"resolve", "choices":choices, "use_defaults":use_defaults}),
    }
}
fn optional_text(value: &Value) -> Result<String> {
    let policy = match value["mode"].as_str() {
        Some("preserve") => OptionalPolicy::Preserve,
        Some("resolve") => OptionalPolicy::Resolve {
            choices: serde_json::from_value(value["choices"].clone())?,
            use_defaults: value["use_defaults"]
                .as_bool()
                .context("Missing optional default policy")?,
        },
        _ => anyhow::bail!("Missing optional policy in build explanation"),
    };
    Ok(super::text::optional(&policy))
}

pub(super) fn text(report: &Value) -> Result<String> {
    use crate::terminal::display_line;
    let mut lines = vec![format!(
        "Build plan: {} {}",
        display_line(report["pack"].as_str().unwrap()),
        display_line(report["version"].as_str().unwrap())
    )];
    let lock = match report["lock"].as_str().unwrap() {
        "current" => "current",
        "missing" => "not found",
        _ => "needs sync",
    };
    lines.push(format!("Lock: {lock}"));
    for profile in report["profiles"].as_array().unwrap() {
        lines.push(String::new());
        let origin = match profile["selection"].as_str().unwrap() {
            "default-profile" => "default profile",
            "consumer-profile" => "profile selected by consumer",
            "ad-hoc" => "ad hoc recipe",
            _ => "selected profile",
        };
        lines.push(format!(
            "{} ({origin}) -> {}",
            display_line(profile["name"].as_str().unwrap()),
            display_line(profile["output"].as_str().unwrap())
        ));
        let selected = crate::engine::documents::recipe::decode_profile(&profile["recipe"])?;
        lines.push(format!("  {}", super::text::recipe(selected.recipe)));
        lines.push(format!(
            "  Optional files: {}",
            optional_text(&profile["optional_policy"])?
        ));
        let overrides: Vec<_> = profile["origins"]
            .as_object()
            .unwrap()
            .iter()
            .filter(|(_, origin)| origin.as_str() == Some("cli-override"))
            .map(|(field, _)| display_line(field).into_owned())
            .collect();
        if !overrides.is_empty() {
            lines.push(format!("  Command overrides: {}", overrides.join(", ")));
        }
        if let Some(preferred) = profile["java"]["preferred_major"].as_u64() {
            let supported = profile["java"]["supported_majors"]
                .as_array()
                .map(|values| {
                    values
                        .iter()
                        .filter_map(Value::as_u64)
                        .map(|major| major.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            lines.push(if supported.is_empty() {
                format!("  Java: {preferred} preferred; no compatibility range established")
            } else {
                format!("  Java: {preferred} preferred; supported majors: {supported}")
            });
            if let Some(recommended) = profile["java"]["recommended_major"]
                .as_u64()
                .filter(|major| *major != preferred)
            {
                lines.push(format!("  Recommended Java: {recommended}"));
            }
        }
        if let Some(version) = profile["bootstrap"]["engine_version"].as_str() {
            lines.push(format!("  The included bootstrap finds or downloads empack {}. No separate empack installation is needed.", display_line(version)));
        } else if profile["recipient_empack_required"] == true {
            lines.push("  Installing this release requires empack.".into());
        }
        if let Some(preparation) = profile["runtime_preparation"]
            .as_str()
            .filter(|text| *text != "consumer-defined")
        {
            lines.push(format!("  {}", display_line(preparation)));
        }
    }
    if let Some(manual) = report["declared_manual_inputs"]
        .as_array()
        .filter(|files| !files.is_empty())
    {
        lines.extend([
            String::new(),
            "Manual downloads recorded in the lock:".into(),
        ]);
        for file in manual {
            if let Some(file) = file.as_str() {
                lines.push(format!("  {}", display_line(file)));
            }
        }
    }
    if report["clean"] == true {
        lines.push(
            "Obsolete artifacts are removed only after every new output passes verification."
                .into(),
        );
    }
    if report["lock"] != "current" {
        lines.push("Next: empack sync".into());
    }
    lines.push("Inputs have not been downloaded or checked by this explanation.".into());
    Ok(lines.join("\n"))
}
