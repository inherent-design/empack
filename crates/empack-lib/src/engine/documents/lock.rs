use super::*;

fn resolved_pin(value: &Value) -> Result<ResolvedPin> {
    fields(value, &["project", "selection"])?;
    let pin = ResolvedPin {
        project: provider(required(value, "project")?)?,
        selection: pin(required(value, "selection")?)?,
    };
    pin.validate()?;
    Ok(pin)
}
fn resolved_pin_value(value: &ResolvedPin) -> Value {
    json!({"project":provider_value(&value.project),"selection":pin_value(&value.selection)})
}
fn expected(value: &Value) -> Result<ExpectedContent> {
    fields(value, &["digests", "size", "accepted-observation"])?;
    Ok(ExpectedContent {
        digests: value
            .get("digests")
            .filter(|v| !v.is_null())
            .map(digest_set)
            .transpose()?,
        size: value
            .get("size")
            .filter(|v| !v.is_null())
            .map(|v| v.as_u64().context("Expected unsigned byte length"))
            .transpose()?,
        accepted_observation: value
            .get("accepted-observation")
            .filter(|v| !v.is_null())
            .map(|v| hash32(v).map(ContentId::from_sha256))
            .transpose()?,
    })
}
fn acquisition(value: &Value) -> Result<AcquisitionSpec> {
    Ok(match text(required(value, "kind")?)? {
        "provider" => {
            fields(value, &["kind", "pin", "slot", "downloads"])?;
            AcquisitionSpec::Provider {
                pin: resolved_pin(required(value, "pin")?)?,
                slot: FileSlot::parse(text(required(value, "slot")?)?)?,
                alternatives: urls(required(value, "downloads")?)?,
            }
        }
        "url" => {
            fields(value, &["kind", "downloads"])?;
            AcquisitionSpec::Url(NonEmpty::new(urls(required(value, "downloads")?)?)?)
        }
        "local" => {
            fields(value, &["kind", "path"])?;
            AcquisitionSpec::Local(path(required(value, "path")?)?)
        }
        "embedded" => {
            fields(value, &["kind", "archive", "member"])?;
            AcquisitionSpec::Embedded {
                archive: path(required(value, "archive")?)?,
                member: path(required(value, "member")?)?,
            }
        }
        "manual" => {
            fields(value, &["kind", "pin", "instructions"])?;
            AcquisitionSpec::Manual {
                pin: value
                    .get("pin")
                    .filter(|v| !v.is_null())
                    .map(resolved_pin)
                    .transpose()?,
                instructions: label(required(value, "instructions")?)?.into(),
            }
        }
        _ => bail!("Unknown acquisition kind"),
    })
}
fn acquisition_value(value: &AcquisitionSpec) -> Value {
    match value {
        AcquisitionSpec::Provider {
            pin,
            slot,
            alternatives,
        } => {
            json!({"kind":"provider","pin":resolved_pin_value(pin),"slot":slot.as_str(),"downloads":alternatives})
        }
        AcquisitionSpec::Url(values) => json!({"kind":"url","downloads":values.as_slice()}),
        AcquisitionSpec::Local(path) => json!({"kind":"local","path":path.as_str()}),
        AcquisitionSpec::Embedded { archive, member } => {
            json!({"kind":"embedded","archive":archive.as_str(),"member":member.as_str()})
        }
        AcquisitionSpec::Manual { pin, instructions } => {
            json!({"kind":"manual","pin":pin.as_ref().map(resolved_pin_value),"instructions":instructions})
        }
    }
}
fn dependency(value: &Value) -> Result<LockedDependency> {
    fields(
        value,
        &["title", "content", "identity", "selection", "files"],
    )?;
    let identity = required(value, "identity")?;
    let identity = match text(required(identity, "kind")?)? {
        "provider" => {
            fields(identity, &["kind", "project"])?;
            ResolvedIdentity::Provider(provider(required(identity, "project")?)?)
        }
        "url" => {
            fields(identity, &["kind", "key"])?;
            ResolvedIdentity::Url(DependencyKey::parse(text(required(identity, "key")?)?)?)
        }
        "local" => {
            fields(identity, &["kind", "key"])?;
            ResolvedIdentity::Local(DependencyKey::parse(text(required(identity, "key")?)?)?)
        }
        _ => bail!("Unknown resolved identity"),
    };
    Ok(LockedDependency {
        title: label(required(value, "title")?)?.into(),
        kind: kind(required(value, "content")?)?,
        identity,
        selected: value
            .get("selection")
            .filter(|v| !v.is_null())
            .map(resolved_pin)
            .transpose()?,
        files: NonEmpty::new(
            array(required(value, "files")?)?
                .iter()
                .map(|file| {
                    fields(
                        file,
                        &[
                            "slot",
                            "acquisition",
                            "expected",
                            "placements",
                            "provenance",
                        ],
                    )?;
                    let provenance = required(file, "provenance")?;
                    fields(
                        provenance,
                        &["source", "location", "declared-digests", "conversions"],
                    )?;
                    Ok(ResolvedFile {
                        provenance: Provenance {
                            source: label(required(provenance, "source")?)?.into(),
                            location: optional_text(provenance, "location")?,
                            declared_digests: provenance
                                .get("declared-digests")
                                .filter(|v| !v.is_null())
                                .map(digest_set)
                                .transpose()?,
                            conversions: string_list(required(provenance, "conversions")?)?,
                        },
                        slot: FileSlot::parse(text(required(file, "slot")?)?)?,
                        acquisition: acquisition(required(file, "acquisition")?)?,
                        expected: expected(required(file, "expected")?)?,
                        placements: NonEmpty::new(
                            array(required(file, "placements")?)?
                                .iter()
                                .map(placement)
                                .collect::<Result<_>>()?,
                        )?,
                    })
                })
                .collect::<Result<_>>()?,
        )?,
    })
}
pub(super) fn decode(value: &Value) -> Result<ResolutionLock> {
    fields(
        value,
        &[
            "schema",
            "intent-revision",
            "acceptable-versions",
            "resolver",
            "dependencies",
            "required-edges",
            "coverage",
            "runtime",
        ],
    )?;
    ensure!(
        required(value, "schema")?.as_u64() == Some(LOCK_SCHEMA),
        "Unsupported lock schema; expected {LOCK_SCHEMA}"
    );
    let runtime = required(value, "runtime")?;
    fields(runtime, &["minecraft", "loader", "loader-version"])?;
    Ok(ResolutionLock {
        acceptable_versions: string_list(required(value, "acceptable-versions")?)?
            .iter()
            .map(|value| GameVersion::parse(value))
            .collect::<std::result::Result<_, _>>()?,
        intent_revision: SemanticRevision(hash32(required(value, "intent-revision")?)?),
        resolver: label(required(value, "resolver")?)?.into(),
        dependencies: object(required(value, "dependencies")?)?
            .iter()
            .map(|(key, value)| {
                Ok((
                    DependencyKey::parse(key)?,
                    dependency(value).with_context(|| format!("Locked dependency '{key}'"))?,
                ))
            })
            .collect::<Result<_>>()?,
        required_edges: object(required(value, "required-edges")?)?
            .iter()
            .map(|(key, edges)| {
                let list = string_list(edges)?;
                let set = list
                    .iter()
                    .map(|v| DependencyKey::parse(v))
                    .collect::<std::result::Result<BTreeSet<_>, _>>()?;
                ensure!(list.len() == set.len(), "Duplicate dependency edge");
                Ok((DependencyKey::parse(key)?, set))
            })
            .collect::<Result<_>>()?,
        coverage: object(required(value, "coverage")?)?
            .iter()
            .map(|(key, value)| {
                Ok((
                    DependencyKey::parse(key)?,
                    match text(value)? {
                        "complete" => Coverage::CompleteForSelection,
                        "partial" => Coverage::Partial,
                        "unknown" => Coverage::Unknown,
                        _ => bail!("Unknown dependency coverage"),
                    },
                ))
            })
            .collect::<Result<_>>()?,
        runtime: RuntimeResolution {
            minecraft: GameVersion::parse(text(required(runtime, "minecraft")?)?)?,
            loader: loader(required(runtime, "loader")?)?,
            loader_version: optional_text(runtime, "loader-version")?
                .map(|v| LoaderVersion::parse(&v))
                .transpose()?,
        },
    })
}
pub(super) fn encode(lock: &ResolutionLock) -> Value {
    let dependencies: BTreeMap<_,_> = lock.dependencies.iter().map(|(key,dep)| {
        let identity = match &dep.identity { ResolvedIdentity::Provider(project)=>json!({"kind":"provider","project":provider_value(project)}), ResolvedIdentity::Url(key)=>json!({"kind":"url","key":key.as_str()}), ResolvedIdentity::Local(key)=>json!({"kind":"local","key":key.as_str()}) };
        let files: Vec<_> = dep.files.as_slice().iter().map(|file| json!({"slot":file.slot.as_str(),"provenance":{"source":file.provenance.source,"location":file.provenance.location,"declared-digests":file.provenance.declared_digests.as_ref().map(digests),"conversions":file.provenance.conversions},"acquisition":acquisition_value(&file.acquisition),"expected":{"digests":file.expected.digests.as_ref().map(digests),"size":file.expected.size,"accepted-observation":file.expected.accepted_observation.as_ref().map(|id|hex32(*id.bytes()))},"placements":file.placements.as_slice().iter().map(placement_value).collect::<Vec<_>>()})).collect();
        (key.as_str(),json!({"title":dep.title,"content":kind_name(dep.kind),"identity":identity,"selection":dep.selected.as_ref().map(resolved_pin_value),"files":files}))
    }).collect();
    json!({"schema":LOCK_SCHEMA,"acceptable-versions":lock.acceptable_versions.iter().map(GameVersion::as_str).collect::<Vec<_>>(),"intent-revision":hex32(lock.intent_revision.0),"resolver":lock.resolver,"dependencies":dependencies,
        "required-edges":lock.required_edges.iter().map(|(key,values)|(key.as_str(),values.iter().map(DependencyKey::as_str).collect::<Vec<_>>())).collect::<BTreeMap<_,_>>(),
        "coverage":lock.coverage.iter().map(|(key,value)|(key.as_str(),match value {Coverage::CompleteForSelection=>"complete",Coverage::Partial=>"partial",Coverage::Unknown=>"unknown"})).collect::<BTreeMap<_,_>>(),
        "runtime":{"minecraft":lock.runtime.minecraft.as_str(),"loader":loader_name(lock.runtime.loader),"loader-version":lock.runtime.loader_version.as_ref().map(LoaderVersion::as_str)}})
}
