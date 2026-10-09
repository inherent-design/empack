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
        ],
    )?;
    ensure!(
        required(value, "schema")?.as_u64() == Some(INTENT_SCHEMA),
        "Unsupported intent schema; expected {INTENT_SCHEMA}"
    );
    let pack = required(value, "pack")?;
    fields(pack, &["name", "version", "author", "description"])?;
    let runtime = required(value, "runtime")?;
    fields(runtime, &["minecraft", "acceptable-versions", "loader"])?;
    let load = required(runtime, "loader")?;
    fields(load, &["kind", "version"])?;
    let distribution = required(value, "distribution")?;
    fields(distribution, &["targets", "archive"])?;
    let intent = ProjectIntent {
        metadata: PackMetadata {
            name: text(required(pack, "name")?)?.into(),
            version: text(required(pack, "version")?)?.into(),
            author: optional_text(pack, "author")?,
            description: optional_text(pack, "description")?,
        },
        runtime: RuntimeIntent {
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
            targets: NonEmpty::new(
                array(required(distribution, "targets")?)?
                    .iter()
                    .map(|v| match text(v)? {
                        "mrpack" => Ok(BuildTarget::Mrpack),
                        "client" => Ok(BuildTarget::Client),
                        "server" => Ok(BuildTarget::Server),
                        "client-full" => Ok(BuildTarget::ClientFull),
                        "server-full" => Ok(BuildTarget::ServerFull),
                        _ => bail!("Unknown build target"),
                    })
                    .collect::<Result<_>>()?,
            )?,
            archive: match text(required(distribution, "archive")?)? {
                "zip" => DistributionArchive::Zip,
                "tar.gz" => DistributionArchive::TarGz,
                "7z" => DistributionArchive::SevenZip,
                _ => bail!("Unknown distribution archive"),
            },
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
        &["source", "content", "version", "placement", "environment"],
    )?;
    let source = required(value, "source")?;
    let source = match text(required(source, "kind")?)? {
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
    let version = required(value, "version")?;
    let version = match text(required(version, "mode")?)? {
        "follow-compatible" => {
            fields(version, &["mode"])?;
            VersionIntent::FollowCompatible
        }
        "exact" => {
            fields(version, &["mode", "pin"])?;
            VersionIntent::Exact(pin(required(version, "pin")?)?)
        }
        "content-pinned" => {
            fields(version, &["mode", "digests"])?;
            VersionIntent::ContentPinned(digest_set(required(version, "digests")?)?)
        }
        _ => bail!("Unknown version policy"),
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
        source,
        kind: kind(required(value, "content")?)?,
        version,
        placement,
        requirements: requirements(required(value, "environment")?)?,
    })
}
pub(super) fn encode(intent: &ProjectIntent) -> Value {
    let roots: BTreeMap<_, _> = intent.roots.iter().map(|(key, dep)| {
        let source = match &dep.source {
            SourceIntent::Provider(id) => json!({"kind":"provider","identity":provider_value(id)}),
            SourceIntent::Search { query, providers } => json!({"kind":"search","query":query,"providers":providers.as_slice().iter().map(|v| match v { ProviderKind::Modrinth => "modrinth", ProviderKind::CurseForge => "curseforge" }).collect::<Vec<_>>()}),
            SourceIntent::Url(values) => json!({"kind":"url","downloads":values.as_slice()}),
            SourceIntent::Local(path) => json!({"kind":"local","path":path.as_str()}),
            SourceIntent::LocalFiles(members) => json!({"kind":"local-files","members":members.iter().map(|(slot,path)|(slot.as_str(),path.as_str())).collect::<BTreeMap<_,_>>() }),
        };
        let version = match &dep.version { VersionIntent::FollowCompatible => json!({"mode":"follow-compatible"}), VersionIntent::Exact(pin) => json!({"mode":"exact","pin":pin_value(pin)}), VersionIntent::ContentPinned(expected) => json!({"mode":"content-pinned","digests":digests(expected)}) };
        let placement = match &dep.placement { PlacementIntent::Automatic => json!("automatic"), PlacementIntent::ArchiveRoot(values) => json!({"archive-root":values.as_slice().iter().map(placement_value).collect::<Vec<_>>()}), PlacementIntent::Explicit(values) => json!(values.as_slice().iter().map(placement_value).collect::<Vec<_>>()), PlacementIntent::ByFile(files) => json!({"files": files.iter().map(|(slot, places)| (slot.as_str(), places.as_slice().iter().map(placement_value).collect::<Vec<_>>())).collect::<BTreeMap<_,_>>()}) };
        (key.as_str(), json!({"source":source,"content":kind_name(dep.kind),"version":version,"placement":placement,"environment":requirements_value(&dep.requirements)}))
    }).collect();
    json!({"schema":INTENT_SCHEMA,
        "pack":{"name":intent.metadata.name,"version":intent.metadata.version,"author":intent.metadata.author,"description":intent.metadata.description},
        "runtime":{"minecraft":intent.runtime.minecraft.as_str(),"acceptable-versions":intent.runtime.acceptable_versions.iter().map(GameVersion::as_str).collect::<Vec<_>>(),"loader":{"kind":loader_name(intent.runtime.loader),"version":intent.runtime.loader_version.as_ref().map(LoaderVersion::as_str)}},
        "dependencies":roots,
        "layout":intent.layout.iter().map(|(k,v)| (kind_name(*k),v.as_str())).collect::<BTreeMap<_,_>>(),
        "distribution":{"targets":intent.distribution.targets.as_slice().iter().map(|v| match v { BuildTarget::Mrpack=>"mrpack",BuildTarget::Client=>"client",BuildTarget::Server=>"server",BuildTarget::ClientFull=>"client-full",BuildTarget::ServerFull=>"server-full" }).collect::<Vec<_>>(),"archive":match intent.distribution.archive {DistributionArchive::Zip=>"zip",DistributionArchive::TarGz=>"tar.gz",DistributionArchive::SevenZip=>"7z"}},
        "extensions":intent.extensions.iter().map(|(k,v)| (k,extension_value(v))).collect::<BTreeMap<_,_>>()})
}
