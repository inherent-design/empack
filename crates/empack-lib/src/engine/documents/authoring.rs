//! Schema-defined syntax expansion. This boundary has no catalog or filesystem access.
use super::*;

pub(super) fn expand(mut value: Value) -> Result<Value> {
    ensure!(
        value.get("schema").and_then(Value::as_u64) == Some(INTENT_SCHEMA),
        "Unsupported author schema; expected {INTENT_SCHEMA}. Create a schema {INTENT_SCHEMA} manifest; runtime migrate does not convert document schemas"
    );
    fields(
        &value,
        &[
            "schema",
            "pack",
            "runtime",
            "dependencies",
            "layout",
            "build",
            "extensions",
            "sources",
            "installation-policies",
            "dependency-policy",
        ],
    )?;
    let policy = value.get("dependency-policy").cloned().unwrap_or(json!({}));
    fields(&policy, &["releases"])?;
    let policy = policy
        .get("releases")
        .cloned()
        .unwrap_or(json!("prefer-stable"));
    release_policy(&policy)?;
    value["dependency-policy"] = policy;
    let pack = value.get_mut("pack").context("Missing pack")?;
    fields(pack, &["id", "name", "version", "authors", "description"])?;
    pack.as_object_mut()
        .unwrap()
        .entry("authors")
        .or_insert(json!([]));
    let runtime = value.get_mut("runtime").context("Missing runtime")?;
    fields(
        runtime,
        &["minecraft", "acceptable-versions", "loader", "java"],
    )?;
    let load = runtime
        .get_mut("loader")
        .context("Missing runtime.loader")?;
    if load.is_string() {
        *load = json!({"kind": load.clone()});
    }
    fields(load, &["kind", "pin"])?;
    if let Some(pin) = load.as_object_mut().unwrap().remove("pin") {
        load["version"] = pin;
    }
    let build = value
        .as_object_mut()
        .unwrap()
        .remove("build")
        .unwrap_or(json!({}));
    fields(&build, &["profiles", "default"])?;
    let profiles = build.get("profiles").cloned().unwrap_or(json!({}));
    let profiles = object(&profiles)?
        .iter()
        .map(|(name, profile)| {
            Ok((
                name.clone(),
                recipe::encode_profile(
                    &profile_value(profile).with_context(|| format!("build.profiles.{name}"))?,
                ),
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    value["distribution"] = json!({"profiles": profiles, "defaults": build.get("default").cloned().unwrap_or(json!([]))});
    value
        .as_object_mut()
        .unwrap()
        .entry("dependencies")
        .or_insert(json!({}));
    for (key, dependency) in value["dependencies"]
        .as_object_mut()
        .context("dependencies must be a mapping")?
    {
        *dependency =
            expand_dependency(key, dependency).with_context(|| format!("dependencies.{key}"))?;
    }
    Ok(value)
}

fn profile_value(value: &Value) -> Result<BuildProfile> {
    use empack_core::distribution::Recipe;
    let value = if value.is_string() {
        json!({"target":value})
    } else {
        value.clone()
    };
    fields(
        &value,
        &["target", "delivery", "environment", "updates", "format"],
    )?;
    let target = text(required(&value, "target")?)?;
    let preset = match target {
        "prism" => Recipe::PRISM_BUNDLED,
        "server" => Recipe::SERVER_BUNDLED,
        "modrinth" => Recipe::MODRINTH,
        "curseforge" => Recipe::CURSEFORGE,
        "empack" => Recipe::EMPACK_REFERENCES,
        _ => bail!("Unknown build target '{target}'"),
    };
    let mut expanded = recipe::encode(&preset);
    for key in ["delivery", "environment", "updates"] {
        if let Some(setting) = value.get(key) {
            expanded[key] = setting.clone();
        }
    }
    Ok(BuildProfile {
        recipe: recipe::decode(&expanded)?,
        archive: recipe::archive(value.get("format").map(text).transpose()?.unwrap_or("zip"))?,
    })
}

fn expand_dependency(key: &str, input: &Value) -> Result<Value> {
    let mut value = if input.is_string() {
        json!({"source":input})
    } else {
        input.clone()
    };
    fields(
        &value,
        &[
            "source",
            "path",
            "url",
            "mirrors",
            "sha256",
            "digests",
            "pin",
            "releases",
            "type",
            "side",
            "optional",
            "environment",
            "placement",
        ],
    )?;
    let source_count = ["source", "path", "url"]
        .iter()
        .filter(|key| value.get(**key).is_some())
        .count();
    ensure!(source_count == 1, "Specify exactly one source, path or url");
    let direct_url = value.get("url").is_some();
    let map = value.as_object_mut().unwrap();
    let source = if let Some(path) = map.remove("path") {
        json!({"kind":"local","path":path})
    } else if let Some(url) = map.remove("url") {
        let mut urls = vec![text(&url)?.to_owned()];
        if let Some(mirrors) = map.remove("mirrors") {
            urls.extend(string_list(&mirrors)?);
        }
        json!({"kind":"url","downloads":urls})
    } else {
        let source = map.remove("source").unwrap();
        if let Some(source) = source.as_str() {
            let (provider, project) = source
                .split_once(':')
                .context("Source must be modrinth:SELECTOR or curseforge:ID")?;
            ensure!(
                matches!(provider, "modrinth" | "curseforge"),
                "Unknown source provider"
            );
            json!({"kind":"provider-selector","request":{"provider":provider,"selector":project}})
        } else {
            source
        }
    };
    ensure!(!map.contains_key("mirrors"), "mirrors applies only to url");
    let provider = matches!(
        source.get("kind").and_then(Value::as_str),
        Some("provider" | "provider-selector")
    );
    let pin = map.remove("pin");
    ensure!(
        !map.contains_key("releases")
            || ((provider || source.get("kind").and_then(Value::as_str) == Some("search"))
                && pin.is_none()),
        "releases applies only to an unpinned provider request"
    );
    if let Some(policy) = map.get("releases") {
        release_policy(policy)?;
    }
    ensure!(
        pin.is_none() || provider,
        "pin applies only to a provider source"
    );
    let sha256 = map.remove("sha256");
    let digests = map.remove("digests");
    ensure!(
        sha256.is_none() || digests.is_none(),
        "Specify sha256 or digests, not both"
    );
    let digests = digests.or_else(|| sha256.map(|digest| json!({"sha256":digest})));
    ensure!(
        !provider || digests.is_none(),
        "Provider integrity is resolved from its selected files"
    );
    ensure!(
        !direct_url || digests.is_some(),
        "Direct URLs require sha256 or digests"
    );
    let selection = if let Some(pin) = pin {
        let id = text(&pin)?;
        json!({"mode":"provider-pin","pin":{"provider":if source["kind"]=="provider" {source["identity"]["provider"].clone()} else {source["request"]["provider"].clone()},"id":id}})
    } else if let Some(digests) = digests {
        json!({"mode":"content-assertions","digests":digests})
    } else {
        json!({"mode":"automatic"})
    };
    map.insert("source".into(), source);
    map.insert("selection".into(), selection);
    let kind = map.remove("type").unwrap_or(json!("mod"));
    map.insert("content".into(), kind);
    map.entry("placement").or_insert(json!("automatic"));
    let side = map.remove("side");
    let optional = map.remove("optional");
    ensure!(
        !map.contains_key("environment") || (side.is_none() && optional.is_none()),
        "side/optional conflict with environment"
    );
    if !map.contains_key("environment") {
        let side = side.as_ref().map(text).transpose()?.unwrap_or("both");
        ensure!(matches!(side, "both" | "client" | "server"), "Unknown side");
        let requirement = match optional {
            None | Some(Value::Bool(false)) => json!("required"),
            Some(Value::Bool(true)) => json!({"optional":key,"default-enabled":false}),
            Some(value) => {
                fields(&value, &["choice", "default"])?;
                let choice = text(required(&value, "choice")?)?;
                let enabled = value
                    .get("default")
                    .map(|v| v.as_bool().context("optional.default must be a boolean"))
                    .transpose()?
                    .unwrap_or(false);
                json!({"optional":choice,"default-enabled":enabled})
            }
        };
        map.insert("environment".into(), json!({"client":if side=="server" {json!("unsupported")} else {requirement.clone()},"server":if side=="client" {json!("unsupported")} else {requirement}}));
    }
    Ok(value)
}

/// Produce schema syntax from normalized data, without changing semantic meaning.
pub(super) fn compact(value: Value) -> Result<Value> {
    compact_preserving(value, &Value::Null)
}

/// Keep authored expanded forms in edit comparisons, so shorthand never replaces a block.
pub(super) fn compact_preserving(mut value: Value, original: &Value) -> Result<Value> {
    let policy = value
        .as_object_mut()
        .unwrap()
        .remove("dependency-policy")
        .unwrap();
    if policy != "prefer-stable" {
        value["dependency-policy"] = json!({"releases":policy});
    }
    let distribution = value
        .as_object_mut()
        .unwrap()
        .remove("distribution")
        .unwrap();
    let profiles = object(&distribution["profiles"])?
        .iter()
        .map(|(name, profile)| {
            let mut profile = profile.clone();
            let target = profile.as_object_mut().unwrap().remove("consumer").unwrap();
            profile["target"] = target.clone();
            let preset = json!({"target":target});
            if !original["build"]["profiles"][name].is_object()
                && profile_value(&profile)? == profile_value(&preset)?
            {
                profile = target;
            }
            Ok((name, profile))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    if !profiles.is_empty() || distribution["defaults"] != json!([]) {
        value["build"] = json!({"profiles":profiles,"default":distribution["defaults"]});
    }
    let load = &mut value["runtime"]["loader"];
    let pin = load.as_object_mut().unwrap().remove("version");
    if let Some(pin) = pin.filter(|v| !v.is_null()) {
        load["pin"] = pin;
    } else if !original["runtime"]["loader"].is_object() {
        *load = load["kind"].clone();
    }
    if value["pack"]["authors"] == json!([]) {
        value["pack"].as_object_mut().unwrap().remove("authors");
    }
    if value["pack"]["description"].is_null() {
        value["pack"].as_object_mut().unwrap().remove("description");
    }
    for (key, dependency) in value["dependencies"].as_object_mut().unwrap() {
        let authored = &original["dependencies"][key];
        let map = dependency.as_object_mut().unwrap();
        if map.get("releases").is_none_or(Value::is_null) {
            map.remove("releases");
        }
        let content = map.remove("content").unwrap();
        if content != "mod" {
            map.insert("type".into(), content);
        }
        let selection = map.remove("selection").unwrap();
        match selection["mode"].as_str().unwrap() {
            "provider-pin" => {
                map.insert("pin".into(), selection["pin"]["id"].clone());
            }
            "content-assertions" => {
                let digests = &selection["digests"];
                if authored.get("digests").is_none()
                    && object(digests)?.len() == 1
                    && digests.get("sha256").is_some()
                {
                    map.insert("sha256".into(), digests["sha256"].clone());
                } else {
                    map.insert("digests".into(), digests.clone());
                }
            }
            _ => {}
        }
        let source = map.remove("source").unwrap();
        match source["kind"].as_str().unwrap() {
            "provider-selector" if !authored["source"].is_object() => {
                let request = &source["request"];
                map.insert(
                    "source".into(),
                    json!(format!(
                        "{}:{}",
                        text(&request["provider"])?,
                        text(&request["selector"])?
                    )),
                );
            }
            "local" if !authored["source"].is_object() => {
                map.insert("path".into(), source["path"].clone());
            }
            "url"
                if selection["mode"] == "content-assertions" && !authored["source"].is_object() =>
            {
                let downloads = array(&source["downloads"])?;
                map.insert("url".into(), downloads[0].clone());
                if downloads.len() > 1 {
                    map.insert("mirrors".into(), json!(&downloads[1..]));
                }
            }
            _ => {
                map.insert("source".into(), source);
            }
        }
        if map.get("placement") == Some(&json!("automatic")) {
            map.remove("placement");
        }
        let env = requirements(&map["environment"])?;
        if let Ok(uniform) = env.uniform() {
            let optional = uniform.choice;
            let simple = optional.is_none_or(|choice| choice.description.is_none());
            if simple && authored.get("environment").is_none() {
                map.remove("environment");
                match uniform.environments {
                    Environments::Client => {
                        map.insert("side".into(), json!("client"));
                    }
                    Environments::Server => {
                        map.insert("side".into(), json!("server"));
                    }
                    Environments::Both => {}
                }
                if let Some(choice) = optional {
                    map.insert(
                        "optional".into(),
                        if !authored["optional"].is_object()
                            && choice.key.as_str() == key
                            && !choice.default_enabled
                        {
                            json!(true)
                        } else {
                            json!({"choice":choice.key.as_str(),"default":choice.default_enabled})
                        },
                    );
                }
            }
        }
        if !authored.is_object()
            && map.len() == 1
            && map.get("source").is_some_and(Value::is_string)
        {
            *dependency = map["source"].clone();
        }
    }
    for name in ["layout", "extensions", "installation-policies"] {
        if value[name].as_object().is_some_and(|v| v.is_empty()) {
            value.as_object_mut().unwrap().remove(name);
        }
    }
    if value["sources"]["exclude"] == json!([]) {
        value.as_object_mut().unwrap().remove("sources");
    }
    if value["runtime"]["acceptable-versions"] == json!([]) {
        value["runtime"]
            .as_object_mut()
            .unwrap()
            .remove("acceptable-versions");
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> Value {
        json!({"schema":5,"pack":{"id":"workshop","name":"Workshop","version":"Autumn Update 2","authors":["First Author","Second Author"]},"runtime":{"minecraft":"1.21.1","loader":"neoforge","java":21},"dependencies":{}})
    }
    fn decode(value: &Value) -> Result<DecodedIntent> {
        DocumentCodec.decode_intent(&serde_json::to_vec(value)?, "fixture")
    }
    #[test]
    fn author_only_projects_keep_identity_attribution_and_java_without_native_outputs() {
        let source = decode(&input()).unwrap();
        assert!(source.intent().distribution.profiles.is_empty());
        assert!(source.intent().distribution.defaults.is_empty());
        assert_eq!(source.intent().metadata.id, "workshop");
        assert_eq!(
            source.intent().metadata.authors,
            ["First Author", "Second Author"]
        );
        assert_eq!(source.intent().runtime.java_major, 21);
        let encoded = DocumentCodec.encode_authoring(source.intent()).unwrap();
        assert!(encoded.bindings.is_none());
        assert_eq!(
            DocumentCodec
                .decode_intent(&encoded.intent, "roundtrip")
                .unwrap()
                .intent(),
            source.intent()
        );
    }
    #[test]
    fn presets_and_expanded_profiles_have_identical_meaning() {
        let mut compact = input();
        compact["build"] =
            json!({"default":["client","server"],"profiles":{"client":"prism","server":"server"}});
        let mut expanded = compact.clone();
        expanded["build"]["profiles"] = json!({"client":{"target":"prism","delivery":"bundled","environment":"client","updates":"snapshot","format":"zip"},"server":{"target":"server","delivery":"bundled","environment":"server","updates":"snapshot","format":"zip"}});
        assert_eq!(
            decode(&compact).unwrap().semantic_revision(),
            decode(&expanded).unwrap().semantic_revision()
        );
        expanded["build"]["profiles"]["server"]["format"] = json!("tar.gz");
        assert_ne!(
            decode(&compact).unwrap().semantic_revision(),
            decode(&expanded).unwrap().semantic_revision()
        );
    }
    #[test]
    fn source_specific_forms_preserve_paths_pins_integrity_and_optional_choices() {
        let mut value = input();
        value["dependencies"] = json!({
            "menu":{"source":"modrinth:AANobbMI","pin":"Version1","side":"client"},
            "local":{"path":"vendor/custom.jar"},
            "textures":{"url":"https://example.org/textures.zip","mirrors":["https://mirror.example.org/textures.zip"],"sha256":"a".repeat(64),"type":"resource-pack","side":"client","optional":{"choice":"high-res","default":false}}
        });
        let source = decode(&value).unwrap();
        let roots = &source.intent().roots;
        assert!(matches!(
            roots[&DependencyKey::parse("menu").unwrap()].selection,
            DependencySelection::ProviderPin(_)
        ));
        assert!(
            matches!(&roots[&DependencyKey::parse("local").unwrap()].source, SourceIntent::Local(path) if path.as_str()=="vendor/custom.jar")
        );
        let choice = roots[&DependencyKey::parse("textures").unwrap()]
            .requirements
            .uniform()
            .unwrap()
            .choice
            .unwrap();
        assert_eq!(choice.key.as_str(), "high-res");
        assert!(!choice.default_enabled);
        let author = DocumentCodec.encode_authoring(source.intent()).unwrap();
        assert_eq!(
            DocumentCodec
                .decode_authoring(&author.intent, author.bindings.as_deref(), "roundtrip")
                .unwrap()
                .intent(),
            source.intent()
        );
    }
    #[test]
    fn inapplicable_fields_fail_before_resolution() {
        for invalid in [
            json!({"path":"vendor/a.jar","pin":"Version1"}),
            json!({"source":"modrinth:AANobbMI","path":"a.jar"}),
            json!({"url":"https://example.org/a.jar"}),
            json!({"path":"a.jar","mirrors":["https://example.org/a.jar"]}),
            json!({"source":"modrinth:AANobbMI","sha256":"a".repeat(64)}),
            json!({"source":"modrinth:AANobbMI","side":"client","environment":{"client":"required","server":"unsupported"}}),
            json!({"source":"modrinth:AANobbMI","optional":{"choice":"a","default":"false"}}),
        ] {
            let mut value = input();
            value["dependencies"]["example"] = invalid.clone();
            let error = decode(&value).unwrap_err();
            assert!(
                format!("{error:#}").contains("dependencies.example"),
                "{invalid}: {error:#}"
            );
        }
        let mut legacy = input();
        legacy["schema"] = json!(4);
        assert!(
            format!("{:#}", decode(&legacy).unwrap_err())
                .contains("runtime migrate does not convert")
        );
    }
    #[test]
    fn installation_policies_roundtrip_in_bindings() {
        let mut source = decode(&input()).unwrap().intent().clone();
        source.installation_policies.insert(
            PortableRelPath::parse("config/settings.toml", PathSyntax::ProjectContent).unwrap(),
            empack_core::instance::FilePolicy::Seed,
        );
        let documents = DocumentCodec.encode_authoring(&source).unwrap();
        let author = parse(&documents.intent).unwrap();
        assert!(author.get("installation-policies").is_none());
        assert_eq!(
            DocumentCodec
                .decode_authoring(
                    &documents.intent,
                    documents.bindings.as_deref(),
                    "roundtrip"
                )
                .unwrap()
                .intent(),
            &source
        );
    }
}

#[cfg(test)]
mod policy_tests {
    use super::*;
    #[test]
    fn dependency_preferences_compose_and_reject_pin_or_file_overrides() {
        let base = json!({"schema":5,"pack":{"id":"pack","name":"Pack","version":"1"},"runtime":{"minecraft":"1.21.1","loader":"fabric","java":21},"dependency-policy":{"releases":"stable-only"},"dependencies":{"a":{"source":"modrinth:AANobbMI","releases":"any"},"b":"modrinth:Root0002"}});
        let decoded = DocumentCodec
            .decode_intent(&serde_json::to_vec(&base).unwrap(), "policy")
            .unwrap();
        let intent = decoded.intent();
        assert_eq!(
            intent.release_policy(Some(&DependencyKey::parse("a").unwrap()), None),
            ReleasePolicy::Any
        );
        assert_eq!(
            intent.release_policy(Some(&DependencyKey::parse("b").unwrap()), None),
            ReleasePolicy::StableOnly
        );
        assert_eq!(
            intent.release_policy(
                Some(&DependencyKey::parse("a").unwrap()),
                Some(ReleasePolicy::PreferStable)
            ),
            ReleasePolicy::PreferStable
        );
        let encoded = DocumentCodec.encode_authoring(intent).unwrap();
        assert_eq!(
            DocumentCodec
                .decode_authoring(&encoded.intent, encoded.bindings.as_deref(), "roundtrip")
                .unwrap()
                .intent(),
            intent
        );
        for dependency in [
            json!({"source":"modrinth:AANobbMI","pin":"Version1","releases":"any"}),
            json!({"path":"vendor/mod.jar","releases":"any"}),
            json!({"source":"modrinth:AANobbMI","releases":"latest"}),
        ] {
            let mut invalid = base.clone();
            invalid["dependencies"]["a"] = dependency;
            assert!(
                DocumentCodec
                    .decode_intent(&serde_json::to_vec(&invalid).unwrap(), "invalid")
                    .is_err()
            );
        }
    }
}
