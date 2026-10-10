use super::*;

fn source() -> Value {
    json!({"schema":2,"pack":{"name":"Pack [日本語]\n$(display data)","version":"alpha"},
        "runtime":{"minecraft":"1.20.1","loader":{"kind":"neoforge","version":"47.1.106"}},
        "distribution":{"targets":["mrpack","client","server","client-full","server-full"],"archive":"7z"},
        "layout":{"data-pack":"world/datapacks"},
        "dependencies":{"renderer alias":{"source":{"kind":"provider","identity":{"provider":"modrinth","project":"AANobbMI"}},"content":"mod","version":{"mode":"exact","pin":{"provider":"modrinth","id":"Version1"}},"placement":"automatic","environment":{"client":{"optional":"renderer","default-enabled":false,"description":"Rendering support"},"server":"unsupported"}}},
        "extensions":{"authoring":{"note":"retain this","values":[1,true,null]}}})
}
fn decoded() -> DecodedIntent {
    DocumentCodec
        .decode_intent(&serde_json::to_vec(&source()).unwrap(), "test.yml")
        .unwrap()
}
fn resolution(source: &DecodedIntent) -> ResolutionLock {
    let (key, dep) = source.intent().roots.first_key_value().unwrap();
    let SourceIntent::Provider(project) = &dep.source else {
        unreachable!()
    };
    let pin = ResolvedPin {
        project: project.clone(),
        selection: PinSelector::ModrinthVersion(ModrinthVersionId::parse("Version1").unwrap()),
    };
    let expected = DigestSet::parse([("md5", "321c3cf486ed509164edec1e1981fec8")]).unwrap();
    let files = [
        ("main", "mods/renderer.jar"),
        ("extra", "mods/renderer-helper.jar"),
    ]
    .into_iter()
    .map(|(slot, destination)| ResolvedFile {
        slot: FileSlot::parse(slot).unwrap(),
        acquisition: AcquisitionSpec::Provider {
            pin: pin.clone(),
            slot: FileSlot::parse(slot).unwrap(),
            alternatives: vec!["https://example.com/file.jar".into()],
        },
        expected: ExpectedContent {
            digests: Some(expected.clone()),
            size: Some(7),
            accepted_observation: None,
        },
        provenance: Provenance {
            source: "mrpack".into(),
            location: Some("files[0]".into()),
            declared_digests: Some(expected.clone()),
            conversions: vec![],
        },
        placements: NonEmpty::new(vec![Placement {
            destination: InstallDestination::parse(destination).unwrap(),
            layer: ContentLayer::Common,
            requirements: dep.requirements.clone(),
        }])
        .unwrap(),
    })
    .collect();
    ResolutionLock {
        acceptable_versions: source.intent().runtime.acceptable_versions.clone(),
        intent_revision: source.semantic_revision(),
        resolver: "test-resolver.v1".into(),
        dependencies: BTreeMap::from([(
            key.clone(),
            LockedDependency {
                title: "Sodium".into(),
                kind: ContentKind::Mod,
                identity: ResolvedIdentity::Provider(project.clone()),
                selected: Some(pin),
                files: NonEmpty::new(files).unwrap(),
            },
        )]),
        required_edges: BTreeMap::new(),
        coverage: BTreeMap::from([(key.clone(), Coverage::Unknown)]),
        runtime: RuntimeResolution {
            minecraft: source.intent.runtime.minecraft.clone(),
            loader: LoaderKind::NeoForge,
            loader_version: source.intent.runtime.loader_version.clone(),
        },
    }
}
fn validate(source: &DecodedIntent, lock: ResolutionLock) -> Result<ResolvedProject> {
    Ok(ResolvedProject::validate(
        source.intent().clone(),
        lock,
        source.semantic_revision(),
    )?)
}

#[test]
fn normalized_documents_round_trip_all_file_slots_and_weak_source_evidence() {
    let source = decoded();
    let reloaded = DocumentCodec
        .decode_intent(
            &DocumentCodec.encode_intent(source.intent()).unwrap(),
            "round-trip.yml",
        )
        .unwrap();
    assert_eq!(source.intent(), reloaded.intent());
    assert_eq!(source.semantic_revision(), reloaded.semantic_revision());
    let project = validate(&source, resolution(&source)).unwrap();
    let locked = DocumentCodec
        .decode_lock(
            &DocumentCodec.encode_lock(&project).unwrap(),
            &source,
            "empack.lock",
        )
        .unwrap();
    assert_eq!(project.lock(), locked.lock());
    let dep = locked.lock().dependencies.values().next().unwrap();
    assert_eq!(dep.files.as_slice().len(), 2);
    assert_eq!(
        dep.files.as_slice()[0]
            .expected
            .digests
            .as_ref()
            .unwrap()
            .strongest(),
        empack_core::digest::DigestAlgorithm::Md5
    );
}

#[test]
fn yaml_scalar_spellings_preserve_authored_strings_and_semantic_revision() {
    for text in [
        "null",
        "yes",
        "0xB",
        "1e3",
        "a#b",
        "\n",
        "\n\n",
        "\u{fffe}\u{ffff}",
    ] {
        let mut value = source();
        value["pack"]["description"] = json!(text);
        value["extensions"]["authoring"]["note"] = json!(text);
        // YAML permits these noncharacters only as escapes, even in JSON-shaped input.
        let authored = serde_json::to_string(&value)
            .unwrap()
            .replace('\u{fffe}', "\\uFFFE")
            .replace('\u{ffff}', "\\uFFFF");
        let original = DocumentCodec
            .decode_intent(authored.as_bytes(), "authored.json")
            .unwrap();
        let encoded = DocumentCodec.encode_intent(original.intent()).unwrap();
        let reloaded = DocumentCodec.decode_intent(&encoded, "empack.yml").unwrap();
        assert_eq!(original.intent(), reloaded.intent(), "{text:?}");
        assert_eq!(original.semantic_revision(), reloaded.semantic_revision());
        assert_eq!(
            DocumentCodec
                .replace_intent(&reloaded, original.intent())
                .unwrap()
                .edit,
            DocumentEdit::Unchanged
        );
    }
}

#[test]
fn manual_file_selection_must_match_its_owning_exact_provider_pin() {
    let source = decoded();
    for matching in [true, false] {
        let mut lock = resolution(&source);
        let dependency = lock.dependencies.values_mut().next().unwrap();
        let mut pin = dependency.selected.clone().unwrap();
        if !matching {
            pin.selection =
                PinSelector::ModrinthVersion(ModrinthVersionId::parse("Version2").unwrap());
        }
        let mut files = dependency.files.as_slice().to_vec();
        files[0].acquisition = AcquisitionSpec::Manual {
            pin: Some(pin),
            instructions: "Select the requested download".into(),
        };
        dependency.files = NonEmpty::new(files).unwrap();
        assert_eq!(lock.validate_structure().is_ok(), matching);
    }
}

#[test]
fn comment_edits_have_same_semantics_but_distinct_raw_revisions() {
    let bytes = serde_saphyr::to_string(&source()).unwrap();
    let a = DocumentCodec.decode_intent(bytes.as_bytes(), "a").unwrap();
    let b = DocumentCodec
        .decode_intent(format!("# user comment\n{bytes}").as_bytes(), "b")
        .unwrap();
    assert_eq!(a.semantic_revision(), b.semantic_revision());
    assert_ne!(a.raw_revision(), b.raw_revision());
    let noop = DocumentCodec.replace_intent(&b, b.intent()).unwrap();
    assert_eq!(noop.edit, DocumentEdit::Unchanged);
    assert_eq!(noop.bytes, b.original());
    let mut next = b.intent().clone();
    next.metadata.version = "next".into();
    let edit = DocumentCodec.replace_intent(&b, &next).unwrap();
    assert_eq!(edit.expected, b.raw_revision());
    assert_eq!(edit.edit, DocumentEdit::Reformatted);
    assert_eq!(
        DocumentCodec
            .decode_intent(&edit.bytes, "edit")
            .unwrap()
            .intent(),
        &next
    );
}

#[test]
fn invalid_explicit_intent_never_becomes_a_search_or_default() {
    let paths = [
        ("/schema", json!(3)),
        (
            "/dependencies/renderer alias/source/identity/project",
            json!("sodium"),
        ),
        (
            "/dependencies/renderer alias/version/pin/id",
            json!("latest"),
        ),
        ("/dependencies/renderer alias/source/kind", json!("unknown")),
        ("/distribution/archive", json!("rar")),
    ];
    for (path, invalid) in paths {
        let mut value = source();
        *value.pointer_mut(path).unwrap() = invalid;
        assert!(
            DocumentCodec
                .decode_intent(&serde_json::to_vec(&value).unwrap(), "invalid")
                .is_err(),
            "{path}"
        );
    }
    let mut value = source();
    value["runtime"]["surprise"] = json!(true);
    assert!(
        DocumentCodec
            .decode_intent(&serde_json::to_vec(&value).unwrap(), "unknown")
            .is_err()
    );
    let raw = serde_saphyr::to_string(&source()).unwrap();
    assert!(
        DocumentCodec
            .decode_intent(format!("schema: 2\n{raw}").as_bytes(), "duplicate")
            .is_err()
    );
    assert!(
        DocumentCodec
            .decode_intent(b"empack: {}", "legacy")
            .is_err()
    );
}

#[test]
fn locks_reject_stale_intent_missing_roots_and_cross_project_selections() {
    let source = decoded();
    let mut lock = resolution(&source);
    lock.intent_revision = SemanticRevision([1; 32]);
    assert!(validate(&source, lock).is_err());
    let mut lock = resolution(&source);
    lock.dependencies.clear();
    assert!(validate(&source, lock).is_err());
    let mut lock = resolution(&source);
    lock.dependencies
        .values_mut()
        .next()
        .unwrap()
        .selected
        .as_mut()
        .unwrap()
        .project = ProviderProjectId::Modrinth(ModrinthProjectId::parse("OtherPrj").unwrap());
    assert!(validate(&source, lock).is_err());
    let mut lock = resolution(&source);
    lock.dependencies.values_mut().next().unwrap().kind = ContentKind::ResourcePack;
    assert!(validate(&source, lock).is_err());
}

#[test]
fn duplicate_placements_fail_but_identical_bytes_at_distinct_paths_survive() {
    let source = decoded();
    let mut lock = resolution(&source);
    let dep = lock.dependencies.values_mut().next().unwrap();
    let mut files = dep.files.as_slice().to_vec();
    files[1].placements = files[0].placements.clone();
    dep.files = NonEmpty::new(files).unwrap();
    assert!(validate(&source, lock).is_err());
    assert_eq!(
        validate(&source, resolution(&source))
            .unwrap()
            .lock()
            .dependencies
            .values()
            .next()
            .unwrap()
            .files
            .as_slice()
            .len(),
        2
    );
}

#[test]
fn resolution_preserves_requirements_provenance_and_coverage() {
    let source = decoded();
    let mut lock = resolution(&source);
    lock.coverage.clear();
    assert!(validate(&source, lock).is_err());
    let mut lock = resolution(&source);
    let dep = lock.dependencies.values_mut().next().unwrap();
    let mut files = dep.files.as_slice().to_vec();
    let mut places = files[0].placements.as_slice().to_vec();
    places[0].requirements.client = Requirement::Required;
    files[0].placements = NonEmpty::new(places).unwrap();
    dep.files = NonEmpty::new(files).unwrap();
    assert!(validate(&source, lock).is_err());
    let mut lock = resolution(&source);
    let dep = lock.dependencies.values_mut().next().unwrap();
    let mut files = dep.files.as_slice().to_vec();
    files[0].expected.digests =
        Some(DigestSet::parse([("sha256", "a".repeat(64).as_str())]).unwrap());
    dep.files = NonEmpty::new(files).unwrap();
    assert!(validate(&source, lock).is_err());
}

#[test]
fn url_order_changes_semantics_and_credentials_cannot_enter_documents() {
    let mut value = source();
    let dep = &mut value["dependencies"]["renderer alias"];
    dep["source"] =
        json!({"kind":"url","downloads":["https://example.com/one","https://example.com/two"]});
    dep["version"] = json!({"mode":"content-pinned","digests":{"sha256":"a".repeat(64)}});
    let a = DocumentCodec
        .decode_intent(&serde_json::to_vec(&value).unwrap(), "a")
        .unwrap();
    value["dependencies"]["renderer alias"]["source"]["downloads"]
        .as_array_mut()
        .unwrap()
        .reverse();
    let b = DocumentCodec
        .decode_intent(&serde_json::to_vec(&value).unwrap(), "b")
        .unwrap();
    assert_ne!(a.semantic_revision(), b.semantic_revision());
    for url in [
        "https://user:secret@example.com/file",
        "https://example.com/file?token=secret",
        "https://example.com/file?access_token=secret",
        "https://example.com/file?ACCESS%5FTOKEN=secret",
        "https://example.com/file?refresh-token=secret",
        "https://example.com/file?client_secret=secret",
        "https://example.com/file?X-Goog-Credential=secret",
        "https://example.com/file?AWSAccessKeyId=secret",
    ] {
        value["dependencies"]["renderer alias"]["source"]["downloads"] = json!([url]);
        assert!(
            DocumentCodec
                .decode_intent(&serde_json::to_vec(&value).unwrap(), "secret")
                .is_err()
        );
    }
}

#[test]
fn requirements_remain_lossless_even_when_a_backend_projection_cannot_express_them() {
    let mut value = source();
    value["dependencies"]["renderer alias"]["environment"]["server"] = json!("required");
    let source = DocumentCodec
        .decode_intent(&serde_json::to_vec(&value).unwrap(), "mixed")
        .unwrap();
    let requirements = &source.intent.roots.values().next().unwrap().requirements;
    assert!(requirements.uniform().is_err());
    assert_eq!(
        source.intent(),
        DocumentCodec
            .decode_intent(
                &DocumentCodec.encode_intent(source.intent()).unwrap(),
                "roundtrip"
            )
            .unwrap()
            .intent()
    );
}

#[test]
fn lock_encoder_rejects_ephemeral_credentials_before_serialization() {
    let source = decoded();
    let mut lock = resolution(&source);
    let dependency = lock.dependencies.values_mut().next().unwrap();
    let mut files = dependency.files.as_slice().to_vec();
    if let AcquisitionSpec::Provider { alternatives, .. } = &mut files[0].acquisition {
        *alternatives = vec!["https://example.com/file?access_token=not-for-storage".into()];
    }
    dependency.files = NonEmpty::new(files).unwrap();
    let project = validate(&source, lock).unwrap();
    let error = DocumentCodec.encode_lock(&project).unwrap_err();
    assert!(!format!("{error:#}").contains("not-for-storage"));
}

#[test]
fn prior_lock_preserves_exact_selections_without_claiming_changed_intent_is_resolved() {
    let original = decoded();
    let project = validate(&original, resolution(&original)).unwrap();
    let bytes = DocumentCodec.encode_lock(&project).unwrap();
    let prior = DocumentCodec
        .decode_prior_lock(&bytes, "empack.lock")
        .unwrap();
    let mut changed = original.intent().clone();
    changed.metadata.version = "next".into();
    let changed = DocumentCodec
        .decode_intent(
            &DocumentCodec.encode_intent(&changed).unwrap(),
            "empack.yml",
        )
        .unwrap();
    assert!(prior.bind(&changed).is_err());
    assert_eq!(prior.lock().dependencies, project.lock().dependencies);
    assert!(prior.bind(&original).is_ok());
    let mut corrupt: Value = serde_saphyr::from_str(std::str::from_utf8(&bytes).unwrap()).unwrap();
    corrupt["resolver"] = json!("");
    assert!(
        DocumentCodec
            .decode_prior_lock(&serde_json::to_vec(&corrupt).unwrap(), "empack.lock")
            .is_err()
    );
}

#[test]
fn shared_override_placements_survive_both_wire_documents() {
    let mut source = decoded().intent().clone();
    let dep = source.roots.values_mut().next().unwrap();
    let placements = NonEmpty::new(vec![
        Placement {
            destination: InstallDestination::parse("mods/renderer.jar").unwrap(),
            layer: ContentLayer::CommonOverride,
            requirements: dep.requirements.clone(),
        },
        Placement {
            destination: InstallDestination::parse("mods/renderer-helper.jar").unwrap(),
            layer: ContentLayer::CommonOverride,
            requirements: dep.requirements.clone(),
        },
    ])
    .unwrap();
    dep.placement = PlacementIntent::ByFile(BTreeMap::from([
        (
            FileSlot::parse("main").unwrap(),
            NonEmpty::new(vec![placements.as_slice()[0].clone()]).unwrap(),
        ),
        (
            FileSlot::parse("extra").unwrap(),
            NonEmpty::new(vec![placements.as_slice()[1].clone()]).unwrap(),
        ),
    ]));
    let encoded = DocumentCodec.encode_intent(&source).unwrap();
    let decoded = DocumentCodec
        .decode_intent(&encoded, "override.yml")
        .unwrap();
    let mut lock = resolution(&decoded);
    let dep = lock.dependencies.values_mut().next().unwrap();
    let mut files = dep.files.as_slice().to_vec();
    for (file, placement) in files.iter_mut().zip(placements.as_slice()) {
        file.placements = NonEmpty::new(vec![placement.clone()]).unwrap();
    }
    dep.files = NonEmpty::new(files).unwrap();
    let project = validate(&decoded, lock).unwrap();
    let bytes = DocumentCodec.encode_lock(&project).unwrap();
    let locked = DocumentCodec
        .decode_lock(&bytes, &decoded, "override.lock")
        .unwrap();
    assert_eq!(locked.lock(), project.lock());
}

#[test]
fn provider_file_plan_uses_strict_placement_and_requirement_contracts() {
    let base = json!({"schema":1,"environment":{"client":"required","server":"unsupported"},"files":{
        "main.jar":[{"destination":"mods/main.jar","layer":"common","environment":{"client":"required","server":"unsupported"}}]
    }});
    let mut cases = vec![];
    let mut invalid = base.clone();
    invalid["files"]["main.jar"][0]["destination"] = json!("../outside.jar");
    cases.push(invalid);
    let mut invalid = base.clone();
    invalid["files"]["main.jar"][0]["environment"]["typo"] = json!(true);
    cases.push(invalid);
    let mut invalid = base.clone();
    invalid["files"]["main.jar"] = json!([]);
    cases.push(invalid);
    let mut invalid = base.clone();
    invalid["schema"] = json!(2);
    cases.push(invalid);
    let mut invalid = base.clone();
    invalid["files"]["main.jar"][0]["environment"]["client"] =
        json!({"optional":"x","default-enabled":"false"});
    cases.push(invalid);
    for invalid in cases {
        let error = DocumentCodec
            .decode_provider_files(&serde_json::to_vec(&invalid).unwrap(), "fixture")
            .err()
            .expect("accepted invalid file plan");
        assert!(error.downcast_ref::<InvalidDocument>().is_some());
    }
    assert!(
        DocumentCodec
            .decode_provider_files(&serde_json::to_vec(&base).unwrap(), "fixture")
            .is_ok()
    );
    assert!(
        DocumentCodec
            .decode_provider_files(b"schema: 1\nschema: 1\n", "duplicate")
            .is_err()
    );
}

#[test]
fn local_member_sources_have_explicit_nonempty_coverage_and_individual_evidence() {
    let mut value = source();
    let dependency = value["dependencies"]
        .as_object_mut()
        .unwrap()
        .values_mut()
        .next()
        .unwrap();
    dependency["source"] = json!({"kind":"local-files","members":{"level.dat":"pack/saves/world/level.dat","region":"pack/saves/world/region/r.0.0.mca"}});
    dependency["version"] = json!({"mode":"follow-compatible"});
    dependency["placement"] = json!({"files":{
        "level.dat":[{"destination":"saves/world/level.dat","layer":"common","environment":{"client":"required","server":"unsupported"}}],
        "region":[{"destination":"saves/world/region/r.0.0.mca","layer":"common","environment":{"client":"required","server":"unsupported"}}]
    }});
    let bytes = serde_json::to_vec(&value).unwrap();
    let decoded = DocumentCodec.decode_intent(&bytes, "members").unwrap();
    let roundtrip = DocumentCodec
        .decode_intent(
            &DocumentCodec.encode_intent(decoded.intent()).unwrap(),
            "members",
        )
        .unwrap();
    assert_eq!(decoded.intent(), roundtrip.intent());
    for mode in ["empty", "escape", "automatic", "pin"] {
        let mut invalid = value.clone();
        let dependency = invalid["dependencies"]
            .as_object_mut()
            .unwrap()
            .values_mut()
            .next()
            .unwrap();
        match mode {
            "empty" => dependency["source"]["members"] = json!({}),
            "escape" => dependency["source"]["members"]["level.dat"] = json!("../outside"),
            "automatic" => dependency["placement"] = json!("automatic"),
            "pin" => {
                dependency["version"] = json!({"mode":"content-pinned","digests":{"md5":"321c3cf486ed509164edec1e1981fec8"}})
            }
            _ => unreachable!(),
        }
        assert!(
            DocumentCodec
                .decode_intent(&serde_json::to_vec(&invalid).unwrap(), mode)
                .is_err(),
            "{mode}"
        );
    }
}

#[test]
fn named_file_placements_preserve_role_assignment_not_only_the_destination_union() {
    let original = decoded();
    let mut lock = resolution(&original);
    let mut intent = original.intent().clone();
    let files = lock.dependencies.values().next().unwrap().files.as_slice();
    intent.roots.values_mut().next().unwrap().placement = PlacementIntent::ByFile(
        files
            .iter()
            .map(|file| (file.slot.clone(), file.placements.clone()))
            .collect(),
    );
    let encoded = DocumentCodec.encode_intent(&intent).unwrap();
    let source = DocumentCodec
        .decode_intent(&encoded, "named roles")
        .unwrap();
    assert_eq!(source.intent(), &intent);
    lock.intent_revision = source.semantic_revision();
    let project = validate(&source, lock.clone()).unwrap();
    assert_eq!(
        DocumentCodec
            .decode_lock(
                &DocumentCodec.encode_lock(&project).unwrap(),
                &source,
                "lock"
            )
            .unwrap()
            .lock(),
        &lock
    );
    let dependency = lock.dependencies.values_mut().next().unwrap();
    let mut swapped = dependency.files.as_slice().to_vec();
    let first = swapped[0].placements.clone();
    swapped[0].placements = swapped[1].placements.clone();
    swapped[1].placements = first;
    dependency.files = NonEmpty::new(swapped).unwrap();
    assert!(
        validate(&source, lock).is_err(),
        "same destinations cannot authorize different member assignments"
    );
    for invalid in [
        json!({"files":{}}),
        json!({"files":{"main":[]}}),
        json!({"files":{}, "unexpected":true}),
    ] {
        let mut wire: Value =
            serde_saphyr::from_str(std::str::from_utf8(&encoded).unwrap()).unwrap();
        wire["dependencies"]["renderer alias"]["placement"] = invalid;
        assert!(
            DocumentCodec
                .decode_intent(&serde_json::to_vec(&wire).unwrap(), "invalid roles")
                .is_err()
        );
    }
}

#[test]
fn provider_world_documents_separate_archive_assertions_members_and_destination_roots() {
    let original = decoded();
    let mut intent = original.intent().clone();
    let key = intent.roots.keys().next().unwrap().clone();
    let root = intent.roots.get_mut(&key).unwrap();
    root.kind = ContentKind::World;
    let placement = Placement {
        destination: InstallDestination::parse("saves/My World").unwrap(),
        layer: ContentLayer::Common,
        requirements: root.requirements.clone(),
    };
    root.placement = PlacementIntent::ArchiveRoot(NonEmpty::new(vec![placement.clone()]).unwrap());
    let source = DocumentCodec
        .decode_intent(&DocumentCodec.encode_intent(&intent).unwrap(), "world")
        .unwrap();
    let mut lock = resolution(&source);
    let dependency = lock.dependencies.get_mut(&key).unwrap();
    dependency.kind = ContentKind::World;
    let archive = ProviderArchiveSource {
        pin: dependency.selected.clone().unwrap(),
        slot: FileSlot::parse("world.zip").unwrap(),
        expected: dependency.files.as_slice()[0].expected.clone(),
        alternatives: vec!["https://example.com/world.zip".into()],
    };
    let mut level = dependency.files.as_slice()[0].clone();
    level.slot = FileSlot::parse("level.dat").unwrap();
    level.acquisition = AcquisitionSpec::ProviderArchiveMember {
        archive,
        member: PortableRelPath::parse("World/level.dat", PathSyntax::ArchiveMember).unwrap(),
    };
    level.expected = ExpectedContent {
        digests: None,
        size: Some(7),
        accepted_observation: Some(ContentId::from_sha256([5; 32])),
    };
    level.provenance.declared_digests = None;
    let mut placed = placement;
    placed.destination = InstallDestination::parse("saves/My World/level.dat").unwrap();
    level.placements = NonEmpty::new(vec![placed]).unwrap();
    dependency.files = NonEmpty::new(vec![level]).unwrap();
    let resolved = validate(&source, lock.clone()).unwrap();
    let bytes = DocumentCodec.encode_lock(&resolved).unwrap();
    let roundtrip = DocumentCodec
        .decode_lock(&bytes, &source, "world.lock")
        .unwrap();
    assert_eq!(roundtrip.lock(), resolved.lock());
    assert_eq!(roundtrip.intent(), resolved.intent());
    let value: Value = serde_saphyr::from_slice(&bytes).unwrap();
    let file = &value["dependencies"][key.as_str()]["files"][0];
    assert!(file["expected"]["digests"].is_null());
    assert_eq!(
        file["acquisition"]["archive"]["expected"]["digests"]["md5"],
        "321c3cf486ed509164edec1e1981fec8"
    );
    for change in [
        "member-digest",
        "lost-archive-assertion",
        "foreign-pin",
        "wrong-root",
        "empty-level",
        "no-level",
        "declared-member-digest",
    ] {
        let mut changed = lock.clone();
        let dependency = changed.dependencies.get_mut(&key).unwrap();
        let mut file = dependency.files.as_slice()[0].clone();
        match change {
            "member-digest" => {
                file.expected.digests =
                    Some(DigestSet::new(vec![ExpectedDigest::Sha256([5; 32])]).unwrap())
            }
            "lost-archive-assertion" => {
                if let AcquisitionSpec::ProviderArchiveMember { archive, .. } =
                    &mut file.acquisition
                {
                    archive.expected.digests = None;
                }
            }
            "foreign-pin" => {
                if let AcquisitionSpec::ProviderArchiveMember { archive, .. } =
                    &mut file.acquisition
                {
                    archive.pin.selection =
                        PinSelector::ModrinthVersion(ModrinthVersionId::parse("Another1").unwrap());
                }
            }
            "wrong-root" => {
                let mut place = file.placements.as_slice()[0].clone();
                place.destination =
                    InstallDestination::parse("saves/Other World/level.dat").unwrap();
                file.placements = NonEmpty::new(vec![place]).unwrap();
            }
            "empty-level" => file.expected.size = Some(0),
            "no-level" => file.slot = FileSlot::parse("other.dat").unwrap(),
            "declared-member-digest" => {
                file.provenance.declared_digests =
                    Some(DigestSet::new(vec![ExpectedDigest::Sha256([5; 32])]).unwrap())
            }
            _ => unreachable!(),
        }
        dependency.files = NonEmpty::new(vec![file]).unwrap();
        assert!(validate(&source, changed).is_err(), "{change}");
    }
}

#[test]
fn encoding_admission_scales_with_variable_intent_and_lock_data() {
    let source = decoded();
    let small = validate(&source, resolution(&source)).unwrap();
    let baseline = DocumentCodec.encoding_memory(&small).unwrap();
    assert!(baseline < 8 << 20);
    let mut expanded = self::source();
    expanded["pack"]["description"] = json!("description\n\"".repeat(2048));
    expanded["extensions"]["large"] = json!(["extension".repeat(4096)]);
    let expanded = DocumentCodec
        .decode_intent(&serde_json::to_vec(&expanded).unwrap(), "expanded")
        .unwrap();
    let mut lock = resolution(&expanded);
    let root = lock.dependencies.values_mut().next().unwrap();
    let mut files = root.files.clone().into_vec();
    files[0]
        .provenance
        .conversions
        .push("conversion".repeat(4096));
    root.files = NonEmpty::new(files).unwrap();
    let large = validate(&expanded, lock).unwrap();
    let estimate = DocumentCodec.encoding_memory(&large).unwrap();
    assert!(estimate > baseline + (1 << 20));
    let encoded = DocumentCodec.encode_intent(large.intent()).unwrap().len()
        + DocumentCodec.encode_lock(&large).unwrap().len();
    assert!(estimate > encoded as u64 * 8);
}

#[test]
fn native_publication_and_source_rules_are_strict_author_intent() {
    let project = crate::engine::mrpack::tests::project(false, false);
    let mut intent = project.intent().clone();
    intent.source_excludes = vec!["private/**".into(), "!private/shared.txt".into()];
    intent.distribution.native = Some(NativeDistributionIntent {
        pack_id: "stable.pack".into(),
        java_major: 17,
        delivery: empack_core::distribution::Delivery::References,
        policies: std::collections::BTreeMap::from([(
            PortableRelPath::parse("config/options.txt", PathSyntax::ProjectContent).unwrap(),
            empack_core::instance::FilePolicy::Seed,
        )]),
    });
    let codec = DocumentCodec;
    let encoded = codec.encode_intent(&intent).unwrap();
    let decoded = codec.decode_intent(&encoded, "native").unwrap();
    assert_eq!(decoded.intent(), &intent);
    let old = codec
        .decode_intent(&codec.encode_intent(project.intent()).unwrap(), "old")
        .unwrap();
    assert_ne!(decoded.semantic_revision(), old.semantic_revision());
    intent.source_excludes.push("bad\npattern".into());
    assert!(codec.encode_intent(&intent).is_err());
    let mut value: serde_json::Value = serde_saphyr::from_slice(&encoded).unwrap();
    value["distribution"]["native"]["extra"] = serde_json::json!(true);
    assert!(
        codec
            .decode_intent(&serde_json::to_vec(&value).unwrap(), "unknown")
            .is_err()
    );
}
