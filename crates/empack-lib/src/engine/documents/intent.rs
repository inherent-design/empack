use super::*;

pub(super) fn decode(value: &Value) -> Result<ProjectIntent> {
    fields(
        value,
        &[
            "schema",
            "pack",
            "runtime",
            "dependencies",
            "layout",
            "distribution",
            "extensions",
            "sources",
            "installation-policies",
            "dependency-policy",
        ],
    )?;
    ensure!(
        required(value, "schema")?.as_u64() == Some(INTENT_SCHEMA),
        "Unsupported intent schema; expected {INTENT_SCHEMA}"
    );
    let pack = required(value, "pack")?;
    fields(pack, &["id", "name", "version", "authors", "description"])?;
    let runtime = required(value, "runtime")?;
    fields(
        runtime,
        &["minecraft", "acceptable-versions", "loader", "java"],
    )?;
    let load = required(runtime, "loader")?;
    fields(load, &["kind", "version"])?;
    let distribution = required(value, "distribution")?;
    fields(distribution, &["profiles", "defaults"])?;
    let source_excludes = if let Some(sources) = value.get("sources") {
        fields(sources, &["exclude"])?;
        sources
            .get("exclude")
            .map(string_list)
            .transpose()?
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let installation_policies = value
        .get("installation-policies")
        .map(|v| {
            object(v)?
                .iter()
                .map(|(k, v)| {
                    let policy = match text(v)? {
                        "managed" => empack_core::instance::FilePolicy::Managed,
                        "seed" => empack_core::instance::FilePolicy::Seed,
                        _ => bail!("Unknown installation file policy"),
                    };
                    Ok((
                        PortableRelPath::parse(k, PathSyntax::ProjectContent)?,
                        policy,
                    ))
                })
                .collect::<Result<BTreeMap<_, _>>>()
        })
        .transpose()?
        .unwrap_or_default();
    let intent = ProjectIntent {
        dependency_policy: release_policy(required(value, "dependency-policy")?)?,
        source_excludes,
        installation_policies,
        metadata: PackMetadata {
            id: text(required(pack, "id")?)?.into(),
            name: text(required(pack, "name")?)?.into(),
            version: text(required(pack, "version")?)?.into(),
            authors: string_list(required(pack, "authors")?)?,
            description: optional_text(pack, "description")?,
        },
        runtime: RuntimeIntent {
            java_major: u16::try_from(
                required(runtime, "java")?
                    .as_u64()
                    .context("runtime.java must be an integer")?,
            )?,
            minecraft: GameVersion::parse(text(required(runtime, "minecraft")?)?)?,
            acceptable_versions: runtime
                .get("acceptable-versions")
                .map(|v| {
                    string_list(v)?
                        .iter()
                        .map(|v| GameVersion::parse(v).map_err(Into::into))
                        .collect::<Result<Vec<_>>>()
                })
                .transpose()?
                .unwrap_or_default(),
            loader: loader(required(load, "kind")?)?,
            loader_version: optional_text(load, "version")?
                .map(|v| LoaderVersion::parse(&v))
                .transpose()?,
        },
        roots: object(required(value, "dependencies")?)?
            .iter()
            .map(|(key, value)| {
                Ok((
                    DependencyKey::parse(key)?,
                    dependency(value).with_context(|| format!("Dependency '{key}'"))?,
                ))
            })
            .collect::<Result<_>>()?,
        layout: value
            .get("layout")
            .map(|v| {
                object(v)?
                    .iter()
                    .map(|(key, value)| Ok((kind(&json!(key))?, path(value)?)))
                    .collect::<Result<_>>()
            })
            .transpose()?
            .unwrap_or_default(),
        distribution: DistributionIntent {
            profiles: object(required(distribution, "profiles")?)?
                .iter()
                .map(|(name, value)| {
                    Ok((
                        name.clone(),
                        recipe::decode_profile(value)
                            .with_context(|| format!("Build profile '{name}'"))?,
                    ))
                })
                .collect::<Result<_>>()?,
            defaults: string_list(required(distribution, "defaults")?)?,
        },
        extensions: value
            .get("extensions")
            .map(|v| object(v).map(|v| v.iter().map(|(k, v)| (k.clone(), extension(v))).collect()))
            .transpose()?
            .unwrap_or_default(),
    };
    intent.validate()?;
    Ok(intent)
}
fn dependency(value: &Value) -> Result<DependencyIntent> {
    fields(
        value,
        &[
            "source",
            "content",
            "selection",
            "placement",
            "environment",
            "releases",
        ],
    )?;
    let source = required(value, "source")?;
    let source = match text(required(source, "kind")?)? {
        "provider-selector" => {
            fields(source, &["kind", "request"])?;
            SourceIntent::ProviderSelector(provider_selector(required(source, "request")?)?)
        }
        "provider" => {
            fields(source, &["kind", "identity"])?;
            SourceIntent::Provider(provider(required(source, "identity")?)?)
        }
        "search" => {
            fields(source, &["kind", "query", "providers"])?;
            SourceIntent::Search {
                query: label(required(source, "query")?)?.into(),
                providers: NonEmpty::new(
                    string_list(required(source, "providers")?)?
                        .iter()
                        .map(|v| match v.as_str() {
                            "modrinth" => Ok(ProviderKind::Modrinth),
                            "curseforge" => Ok(ProviderKind::CurseForge),
                            _ => bail!("Unknown search provider"),
                        })
                        .collect::<Result<_>>()?,
                )?,
            }
        }
        "url" => {
            fields(source, &["kind", "downloads"])?;
            SourceIntent::Url(NonEmpty::new(urls(required(source, "downloads")?)?)?)
        }
        "local" => {
            fields(source, &["kind", "path"])?;
            SourceIntent::Local(path(required(source, "path")?)?)
        }
        "local-files" => {
            fields(source, &["kind", "members"])?;
            SourceIntent::LocalFiles(
                object(required(source, "members")?)?
                    .iter()
                    .map(|(slot, value)| Ok((FileSlot::parse(slot)?, path(value)?)))
                    .collect::<Result<BTreeMap<_, _>>>()?,
            )
        }
        _ => bail!("Unknown dependency source kind"),
    };
    let selection = required(value, "selection")?;
    let selection = match text(required(selection, "mode")?)? {
        "automatic" => {
            fields(selection, &["mode"])?;
            DependencySelection::Automatic
        }
        "provider-pin" => {
            fields(selection, &["mode", "pin"])?;
            DependencySelection::ProviderPin(pin(required(selection, "pin")?)?)
        }
        "content-assertions" => {
            fields(selection, &["mode", "digests"])?;
            DependencySelection::ContentAssertions(digest_set(required(selection, "digests")?)?)
        }
        _ => bail!("Unknown source selection policy"),
    };
    let places = required(value, "placement")?;
    let placement = if places == "automatic" {
        PlacementIntent::Automatic
    } else if places.get("archive-root").is_some() {
        fields(places, &["archive-root"])?;
        PlacementIntent::ArchiveRoot(NonEmpty::new(
            array(required(places, "archive-root")?)?
                .iter()
                .map(placement)
                .collect::<Result<_>>()?,
        )?)
    } else if places.is_object() {
        fields(places, &["files"])?;
        PlacementIntent::ByFile(
            object(required(places, "files")?)?
                .iter()
                .map(|(slot, values)| {
                    Ok((
                        FileSlot::parse(slot)?,
                        NonEmpty::new(
                            array(values)?
                                .iter()
                                .map(placement)
                                .collect::<Result<_>>()?,
                        )?,
                    ))
                })
                .collect::<Result<_>>()?,
        )
    } else {
        PlacementIntent::Explicit(NonEmpty::new(
            array(places)?
                .iter()
                .map(placement)
                .collect::<Result<_>>()?,
        )?)
    };
    Ok(DependencyIntent {
        releases: value
            .get("releases")
            .filter(|value| !value.is_null())
            .map(release_policy)
            .transpose()?,
        source,
        kind: kind(required(value, "content")?)?,
        selection,
        placement,
        requirements: requirements(required(value, "environment")?)?,
    })
}
pub(super) fn encode(intent: &ProjectIntent) -> Value {
    let roots: BTreeMap<_, _> = intent.roots.iter().map(|(key, dep)| {
        let source = match &dep.source {
            SourceIntent::ProviderSelector(request) => json!({"kind":"provider-selector","request":provider_selector_value(request)}),
            SourceIntent::Provider(id) => json!({"kind":"provider","identity":provider_value(id)}),
            SourceIntent::Search { query, providers } => json!({"kind":"search","query":query,"providers":providers.as_slice().iter().map(|v| match v { ProviderKind::Modrinth => "modrinth", ProviderKind::CurseForge => "curseforge" }).collect::<Vec<_>>()}),
            SourceIntent::Url(values) => json!({"kind":"url","downloads":values.as_slice()}),
            SourceIntent::Local(path) => json!({"kind":"local","path":path.as_str()}),
            SourceIntent::LocalFiles(members) => json!({"kind":"local-files","members":members.iter().map(|(slot,path)|(slot.as_str(),path.as_str())).collect::<BTreeMap<_,_>>() }),
        };
        let selection = match &dep.selection { DependencySelection::Automatic => json!({"mode":"automatic"}), DependencySelection::ProviderPin(pin) => json!({"mode":"provider-pin","pin":pin_value(pin)}), DependencySelection::ContentAssertions(expected) => json!({"mode":"content-assertions","digests":digests(expected)}) };
        let placement = match &dep.placement { PlacementIntent::Automatic => json!("automatic"), PlacementIntent::ArchiveRoot(values) => json!({"archive-root":values.as_slice().iter().map(placement_value).collect::<Vec<_>>()}), PlacementIntent::Explicit(values) => json!(values.as_slice().iter().map(placement_value).collect::<Vec<_>>()), PlacementIntent::ByFile(files) => json!({"files": files.iter().map(|(slot, places)| (slot.as_str(), places.as_slice().iter().map(placement_value).collect::<Vec<_>>())).collect::<BTreeMap<_,_>>()}) };
        (key.as_str(), json!({"releases":dep.releases.map(ReleasePolicy::as_str),"source":source,"content":kind_name(dep.kind),"selection":selection,"placement":placement,"environment":requirements_value(&dep.requirements)}))
    }).collect();
    json!({"schema":INTENT_SCHEMA,"dependency-policy":intent.dependency_policy.as_str(),
        "pack":{"id":intent.metadata.id,"name":intent.metadata.name,"version":intent.metadata.version,"authors":intent.metadata.authors,"description":intent.metadata.description},
        "runtime":{"java":intent.runtime.java_major,"minecraft":intent.runtime.minecraft.as_str(),"acceptable-versions":intent.runtime.acceptable_versions.iter().map(GameVersion::as_str).collect::<Vec<_>>(),"loader":{"kind":loader_name(intent.runtime.loader),"version":intent.runtime.loader_version.as_ref().map(LoaderVersion::as_str)}},
        "dependencies":roots,
        "sources":{"exclude":intent.source_excludes},
        "layout":intent.layout.iter().map(|(k,v)| (kind_name(*k),v.as_str())).collect::<BTreeMap<_,_>>(),
        "installation-policies":intent.installation_policies.iter().map(|(path,policy)|(path.as_str(),match policy {empack_core::instance::FilePolicy::Managed=>"managed",empack_core::instance::FilePolicy::Seed=>"seed"})).collect::<BTreeMap<_,_>>(),
        "distribution":{"profiles":intent.distribution.profiles.iter().map(|(name, value)| (name, recipe::encode_profile(value))).collect::<BTreeMap<_,_>>(),"defaults":intent.distribution.defaults.as_slice()},
        "extensions":intent.extensions.iter().map(|(k,v)| (k,extension_value(v))).collect::<BTreeMap<_,_>>()})
}
