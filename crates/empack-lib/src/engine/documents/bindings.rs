//! Portable integration intent, composed before semantic validation or resolution.
use super::*;

pub(super) fn compose(mut author: Value, bindings: Option<&[u8]>) -> Result<Value> {
    author = authoring::expand(author)?;
    let roots = author["dependencies"].as_object_mut().unwrap();
    if let Some(bytes) = bindings {
        let value = parse(bytes).context("Invalid empack.bindings.yml")?;
        fields(&value, &["schema", "bindings", "policies"])?;
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
        if let Some(policies) = value.get("policies") {
            ensure!(
                author.get("installation-policies").is_none(),
                "Conflicting installation policies"
            );
            author["installation-policies"] = policies.clone();
        }
    }
    Ok(author)
}

/// Split normalized meaning without inventing roles or rewriting provider selectors.
pub(super) fn split(intent: &ProjectIntent) -> Result<AuthoringDocuments> {
    split_preserving(intent, &Value::Null)
}
/// Existing inline integration stays in its authored document during targeted edits.
pub(super) fn split_preserving(
    intent: &ProjectIntent,
    original: &Value,
) -> Result<AuthoringDocuments> {
    let mut author = intent::encode(intent);
    let mut bindings = serde_json::Map::new();
    for (key, value) in author["dependencies"].as_object_mut().unwrap() {
        let dependency = value.as_object_mut().unwrap();
        let mut binding = serde_json::Map::new();
        if original["dependencies"][key].get("placement").is_none() {
            let placement = dependency.remove("placement").unwrap();
            if placement != "automatic" {
                binding.insert("placement".into(), placement);
            }
        }
        let source = dependency["source"].as_object_mut().unwrap();
        if source["kind"] == "local-files"
            && original["dependencies"][key]["source"]
                .get("members")
                .is_none()
        {
            binding.insert("source-members".into(), source.remove("members").unwrap());
        }
        if !binding.is_empty() {
            bindings.insert(key.clone(), Value::Object(binding));
        }
    }
    let policies = if original.get("installation-policies").is_some() {
        json!({})
    } else {
        author
            .as_object_mut()
            .unwrap()
            .remove("installation-policies")
            .unwrap()
    };
    let author = authoring::compact(author)?;
    Ok(AuthoringDocuments {
        intent: serialize(&author)?,
        bindings: if bindings.is_empty() && policies == json!({}) {
            None
        } else {
            let mut value = json!({"schema":1,"bindings":bindings});
            if policies != json!({}) {
                value["policies"] = policies;
            }
            Some(serialize(&value)?)
        },
    })
}
