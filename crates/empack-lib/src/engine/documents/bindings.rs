//! Portable integration intent, composed before semantic validation or resolution.
use super::*;

pub(super) fn compose(mut author: Value, bindings: Option<&[u8]>) -> Result<Value> {
    ensure!(
        required(&author, "schema")?.as_u64() == Some(INTENT_SCHEMA),
        "Unsupported intent schema; expected {INTENT_SCHEMA}"
    );
    // Compact syntax expands into the same normalized domain model as explicit syntax.
    let roots = author
        .get_mut("dependencies")
        .and_then(Value::as_object_mut)
        .context("Dependencies must be a mapping")?;
    for (key, value) in roots.iter_mut() {
        if value.is_string() {
            *value = json!({"source":value.clone()});
        }
        let dependency = value
            .as_object_mut()
            .context("Dependency must be a source or mapping")?;
        if let Some(Value::String(source)) = dependency.get("source") {
            let (provider, project) = source
                .split_once(':')
                .context("Provider source must be modrinth:ID or curseforge:ID")?;
            ensure!(
                matches!(provider, "modrinth" | "curseforge"),
                "Unsupported compact source for '{key}'"
            );
            dependency.insert(
                "source".into(),
                json!({"kind":"provider","identity":{"provider":provider,"project":project}}),
            );
        }
        dependency.entry("content").or_insert(json!("mod"));
        dependency
            .entry("version")
            .or_insert(json!({"mode":"follow-compatible"}));
        dependency.entry("placement").or_insert(json!("automatic"));
        if let Some(side) = dependency.remove("side") {
            ensure!(
                !dependency.contains_key("environment"),
                "Dependency '{key}' specifies both side and environment"
            );
            let requirements = match text(&side)? {
                "client" => json!({"client":"required","server":"unsupported"}),
                "server" => json!({"client":"unsupported","server":"required"}),
                "both" => json!({"client":"required","server":"required"}),
                _ => bail!("Dependency '{key}' has an unknown side"),
            };
            dependency.insert("environment".into(), requirements);
        }
        dependency
            .entry("environment")
            .or_insert(json!({"client":"required","server":"required"}));
    }
    if let Some(bytes) = bindings {
        let value = parse(bytes).context("Invalid empack.bindings.yml")?;
        fields(&value, &["schema", "bindings"])?;
        ensure!(
            required(&value, "schema")?.as_u64() == Some(1),
            "Unsupported bindings schema; expected 1"
        );
        for (key, binding) in object(required(&value, "bindings")?)? {
            fields(binding, &["placement", "source-members"])?;
            ensure!(
                binding.get("placement").is_some() || binding.get("source-members").is_some(),
                "Empty binding for '{key}'"
            );
            let dependency = roots
                .get_mut(key)
                .with_context(|| format!("Binding references unknown dependency '{key}'"))?
                .as_object_mut()
                .unwrap();
            if let Some(placement) = binding.get("placement") {
                ensure!(
                    dependency["placement"] == "automatic",
                    "Conflicting placement in empack.yml and empack.bindings.yml for '{key}'"
                );
                dependency.insert("placement".into(), placement.clone());
            }
            if let Some(members) = binding.get("source-members") {
                let source = dependency
                    .get_mut("source")
                    .and_then(Value::as_object_mut)
                    .context("Member binding requires a local-files source")?;
                ensure!(
                    source.get("kind") == Some(&json!("local-files"))
                        && !source.contains_key("members"),
                    "Conflicting or inapplicable source-member binding for '{key}'"
                );
                source.insert("members".into(), members.clone());
            }
        }
    }
    Ok(author)
}

/// Split normalized meaning without inventing roles or rewriting provider selectors.
pub(super) fn split(intent: &ProjectIntent) -> Result<AuthoringDocuments> {
    let mut author = intent::encode(intent);
    let mut bindings = serde_json::Map::new();
    for (key, value) in author["dependencies"].as_object_mut().unwrap() {
        let dependency = value.as_object_mut().unwrap();
        let mut binding = serde_json::Map::new();
        let placement = dependency.remove("placement").unwrap();
        if placement != "automatic" {
            binding.insert("placement".into(), placement);
        }
        let source = dependency["source"].as_object_mut().unwrap();
        if source["kind"] == "local-files" {
            binding.insert("source-members".into(), source.remove("members").unwrap());
        }
        if !binding.is_empty() {
            bindings.insert(key.clone(), Value::Object(binding));
        }
        if source["kind"] == "provider" {
            let id = &source["identity"];
            let project = match &id["project"] {
                Value::String(v) => v.clone(),
                other => other.to_string(),
            };
            let compact = format!("{}:{project}", id["provider"].as_str().unwrap());
            dependency.insert("source".into(), json!(compact));
        }
        if dependency["content"] == "mod" {
            dependency.remove("content");
        }
        if dependency["version"] == json!({"mode":"follow-compatible"}) {
            dependency.remove("version");
        }
        let requirements = &dependency["environment"];
        let side = if *requirements == json!({"client":"required","server":"required"}) {
            Some("both")
        } else if *requirements == json!({"client":"required","server":"unsupported"}) {
            Some("client")
        } else if *requirements == json!({"client":"unsupported","server":"required"}) {
            Some("server")
        } else {
            None
        };
        if let Some(side) = side {
            dependency.remove("environment");
            if side != "both" {
                dependency.insert("side".into(), json!(side));
            }
        }
        if dependency.len() == 1 && dependency.get("source").is_some_and(Value::is_string) {
            *value = dependency["source"].clone();
        }
    }
    for name in ["layout", "extensions"] {
        if author[name].as_object().is_some_and(|v| v.is_empty()) {
            author.as_object_mut().unwrap().remove(name);
        }
    }
    if author["sources"]["exclude"] == json!([]) {
        author.as_object_mut().unwrap().remove("sources");
    }
    for name in ["author", "description"] {
        if author["pack"][name].is_null() {
            author["pack"].as_object_mut().unwrap().remove(name);
        }
    }
    if author["runtime"]["acceptable-versions"] == json!([]) {
        author["runtime"]
            .as_object_mut()
            .unwrap()
            .remove("acceptable-versions");
    }
    if author["runtime"]["loader"]["version"].is_null() {
        author["runtime"]["loader"]
            .as_object_mut()
            .unwrap()
            .remove("version");
    }
    if author["distribution"]["native"].is_null() {
        author["distribution"]
            .as_object_mut()
            .unwrap()
            .remove("native");
    }
    Ok(AuthoringDocuments {
        intent: serialize(&author)?,
        bindings: if bindings.is_empty() {
            None
        } else {
            Some(serialize(&json!({"schema":1,"bindings":bindings}))?)
        },
    })
}
