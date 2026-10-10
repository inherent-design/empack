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
    fields(distribution, &["recipes", "archive", "native"])?;
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
    let native = distribution
        .get("native")
        .filter(|v| !v.is_null())
        .map(|native| -> Result<NativeDistributionIntent> {
            fields(native, &["pack-id", "java-major", "delivery", "policies"])?;
            let policies = native
                .get("policies")
                .map(|v| {
                    object(v)?
                        .iter()
                        .map(|(k, v)| {
                            let policy = match text(v)? {
                                "managed" => empack_core::instance::FilePolicy::Managed,
                                "seed" => empack_core::instance::FilePolicy::Seed,
                                _ => bail!("Unknown native file policy"),
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
            Ok(NativeDistributionIntent {
                pack_id: text(required(native, "pack-id")?)?.into(),
                java_major: u16::try_from(
                    required(native, "java-major")?
                        .as_u64()
                        .context("Java major must be an integer")?,
                )?,
                delivery: match text(required(native, "delivery")?)? {
                    "references" => empack_core::distribution::Delivery::References,
                    "bundled" => empack_core::distribution::Delivery::Bundled,
                    _ => bail!("Unknown native delivery policy"),
                },
                policies,
            })
        })
        .transpose()?;
    let intent = ProjectIntent {
        source_excludes,
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
            native,
            recipes: NonEmpty::new(
                array(required(distribution, "recipes")?)?
                    .iter()
                    .map(recipe::decode)
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
        "sources":{"exclude":intent.source_excludes},
        "layout":intent.layout.iter().map(|(k,v)| (kind_name(*k),v.as_str())).collect::<BTreeMap<_,_>>(),
        "distribution":{"native":intent.distribution.native.as_ref().map(|native| json!({"pack-id":native.pack_id,"java-major":native.java_major,"delivery":match native.delivery {empack_core::distribution::Delivery::References=>"references",empack_core::distribution::Delivery::Bundled=>"bundled"},"policies":native.policies.iter().map(|(path,policy)|(path.as_str(),match policy {empack_core::instance::FilePolicy::Managed=>"managed",empack_core::instance::FilePolicy::Seed=>"seed"})).collect::<BTreeMap<_,_>>() })),"recipes":intent.distribution.recipes.as_slice().iter().map(recipe::encode).collect::<Vec<_>>(),"archive":match intent.distribution.archive {DistributionArchive::Zip=>"zip",DistributionArchive::TarGz=>"tar.gz",DistributionArchive::SevenZip=>"7z"}},
        "extensions":intent.extensions.iter().map(|(k,v)| (k,extension_value(v))).collect::<BTreeMap<_,_>>()})
}
