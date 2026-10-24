//! One host journey retains companion roles across add, sync, update and adoption.
use super::*;
use empack_core::inventory::OptionalPolicy;
use sha2::Sha256;
use std::io::{Read, Write};

fn archive(member: &str, bytes: &[u8]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    zip.start_file(member, zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(bytes).unwrap();
    zip.finish().unwrap().into_inner()
}

fn members(path: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut archive = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut members = BTreeMap::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        assert!(
            members
                .insert(entry.name().unwrap().into_owned(), bytes)
                .is_none()
        );
    }
    members
}

fn assert_member_names(
    actual: &BTreeMap<String, Vec<u8>>,
    files: impl IntoIterator<Item = String>,
) {
    let mut expected = std::collections::BTreeSet::new();
    for name in files {
        for (index, _) in name.match_indices('/') {
            let directory = &name[..=index];
            assert!(
                actual[directory].is_empty(),
                "directory payload: {directory}"
            );
            expected.insert(directory.to_owned());
        }
        expected.insert(name);
    }
    assert_eq!(
        actual
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>(),
        expected
    );
}

async fn check_outputs(root: &Path, payloads: &BTreeMap<&str, Vec<u8>>) {
    use crate::application::engine_host::build::{BuildDecisions, build};
    use crate::engine::build::BuildAcquisitions;
    let project = root.join("project");
    let decisions = |choices, use_defaults| BuildDecisions {
        optional: OptionalPolicy::Resolve {
            choices,
            use_defaults,
        },
        ..Default::default()
    };
    for enabled in [false, true] {
        build(
            &session(root, false),
            &crate::application::BuildArgs::default(),
            decisions(BTreeMap::from([("extra-art".into(), enabled)]), false),
            BuildAcquisitions::default(),
        )
        .await
        .unwrap();
        let archives: Vec<_> = fs::read_dir(project.join("dist"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(archives.len(), 3);
        for side in ["client", "server"] {
            let suffix = format!("empack-bundled-{side}.empack");
            let path = archives
                .iter()
                .find(|path| path.to_str().unwrap().ends_with(&suffix))
                .unwrap();
            let actual = members(path);
            let release: serde_json::Value =
                serde_json::from_slice(&actual["release.json"]).unwrap();
            let expected = if side == "client" {
                BTreeMap::from([
                    ("mods/renamed.jar", &payloads["renderer-new.jar"]),
                    ("resourcepacks/required.zip", &payloads["assets.zip"]),
                    ("resourcepacks/extra-renamed.zip", &payloads["extras.zip"]),
                ])
            } else {
                BTreeMap::from([
                    ("mods/renamed.jar", &payloads["renderer-new.jar"]),
                    ("mods/server-helper.jar", &payloads["server-helper.jar"]),
                ])
            };
            let expected_choices = if side == "client" {
                json!([{"key":"extra-art","alternatives":["disabled","enabled"],"default":"disabled","description":"Extra artwork"}])
            } else {
                json!([])
            };
            assert_eq!(release["choices"], expected_choices);
            let files = release["files"].as_array().unwrap();
            assert_eq!(files.len(), expected.len(), "{side}: {release}");
            let destinations: std::collections::BTreeSet<_> = files
                .iter()
                .map(|file| file["destination"].as_str().unwrap())
                .collect();
            assert_eq!(destinations, expected.keys().copied().collect());
            let mut expected_members =
                std::collections::BTreeSet::from(["release.json".to_owned()]);
            for file in files {
                let destination = file["destination"].as_str().unwrap();
                let bytes = expected[destination];
                assert_eq!(
                    file[if side == "client" { "server" } else { "client" }],
                    json!({"kind":"unsupported"})
                );
                let participation = if destination == "resourcepacks/extra-renamed.zip" {
                    json!({"kind":"choice","key":"extra-art","value":"enabled"})
                } else {
                    json!({"kind":"required"})
                };
                assert_eq!(file[side], participation);
                let digest =
                    empack_core::digest::ExpectedDigest::Sha256(Sha256::digest(bytes).into()).hex();
                assert_eq!(file["sha256"], digest);
                assert_eq!(file["bytes"], bytes.len());
                let name = format!("assets/{digest}");
                assert_eq!(&actual[&name], bytes);
                expected_members.insert(name);
            }
            assert_member_names(&actual, expected_members);
        }
        let prism = archives
            .iter()
            .find(|path| path.extension().is_some_and(|ext| ext == "zip"))
            .unwrap();
        let actual = members(prism);
        let mut game = BTreeMap::from([
            (".minecraft/mods/renamed.jar", &payloads["renderer-new.jar"]),
            (
                ".minecraft/resourcepacks/required.zip",
                &payloads["assets.zip"],
            ),
        ]);
        if enabled {
            game.insert(
                ".minecraft/resourcepacks/extra-renamed.zip",
                &payloads["extras.zip"],
            );
        }
        let expected_names: std::collections::BTreeSet<_> = game
            .keys()
            .copied()
            .chain(["instance.cfg", "mmc-pack.json"])
            .collect();
        assert_member_names(&actual, expected_names.into_iter().map(str::to_owned));
        for (name, bytes) in game {
            assert_eq!(&actual[name], bytes);
        }
    }
    // Unknown or missing decisions cannot replace the last verified artifact batch.
    for choices in [BTreeMap::new(), BTreeMap::from([("typo".into(), true)])] {
        let before = crate::application::engine_host::tests::snapshot(&project);
        let error = build(
            &session(root, false),
            &crate::application::BuildArgs::default(),
            decisions(choices, false),
            BuildAcquisitions::default(),
        )
        .await
        .unwrap_err();
        assert!(
            format!("{error:#}").to_lowercase().contains("optional"),
            "{error:#}"
        );
        assert_eq!(
            crate::application::engine_host::tests::snapshot(&project),
            before
        );
    }
}

#[tokio::test]
async fn host_provider_roles_survive_update_adoption_and_side_projection() {
    let mut server = mockito::Server::new_async().await;
    server.mock("GET", "/project/renderer")
        .with_body(json!({"id":"Root0001","slug":"renderer","title":"Renderer","project_type":"mod","loaders":["fabric"]}).to_string()).create_async().await;
    server.mock("GET", "/project/Root0001")
        .with_body(json!({"id":"Root0001","slug":"renderer","title":"Renderer","project_type":"mod","loaders":["fabric"]}).to_string()).create_async().await;
    let payloads = BTreeMap::from([
        (
            "renderer.jar",
            archive(
                "fabric.mod.json",
                br#"{"schemaVersion":1,"id":"fixture","version":"1"}"#,
            ),
        ),
        (
            "renderer-new.jar",
            archive(
                "fabric.mod.json",
                br#"{"schemaVersion":1,"id":"fixture","version":"2"}"#,
            ),
        ),
        (
            "assets.zip",
            archive(
                "pack.mcmeta",
                br#"{"pack":{"pack_format":34,"description":"Required assets"}}"#,
            ),
        ),
        (
            "extras.zip",
            archive(
                "pack.mcmeta",
                br#"{"pack":{"pack_format":34,"description":"Optional assets"}}"#,
            ),
        ),
        (
            "server-helper.jar",
            archive(
                "fabric.mod.json",
                br#"{"schemaVersion":1,"id":"server_fixture","version":"1"}"#,
            ),
        ),
    ]);
    let file = |name: &str, primary: bool, role: Option<&str>| {
        let bytes = &payloads[name];
        json!({
            "filename":name,"primary":primary,"size":bytes.len(),"file_type":role,
            "hashes":{"sha512":empack_core::digest::ExpectedDigest::Sha512(Sha512::digest(bytes).into()).hex()},
            "url":format!("https://example.invalid/{name}")
        })
    };
    let version = |id: &str, primary: &str| {
        json!({
            "id":id,"project_id":"Root0001","game_versions":["1.21.1"],"loaders":["fabric"],
            "files":[file(primary,true,None),file("assets.zip",false,Some("required-resource-pack")),file("extras.zip",false,Some("optional-resource-pack")),file("server-helper.jar",false,None)],
            "dependencies":[],"date_published":if id=="RootVer2" {"2026-02-01T00:00:00Z"} else {"2026-01-01T00:00:00Z"},"status":"listed","version_type":"release"
        })
    };
    let first = version("RootVer1", "renderer.jar");
    let second = version("RootVer2", "renderer-new.jar");
    for value in [&first, &second] {
        server
            .mock(
                "GET",
                format!("/version/{}", value["id"].as_str().unwrap()).as_str(),
            )
            .with_body(value.to_string())
            .create_async()
            .await;
    }
    for (name, bytes) in &payloads {
        server
            .mock("GET", format!("/{name}").as_str())
            .with_body(bytes.clone())
            .create_async()
            .await;
    }
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let environment = json!({"client":"required","server":"required"});
    let placement = |path: &str, environment: serde_json::Value| {
        json!([{
            "destination":path,"layer":"common","environment":environment
        }])
    };
    let mut plan = json!({"schema":1,"environment":environment,"files":{
        "primary":placement("mods/renamed.jar",environment.clone()),
        "assets.zip":placement("resourcepacks/required.zip",json!({"client":"required","server":"unsupported"})),
        "server-helper.jar":placement("mods/server-helper.jar",json!({"client":"unsupported","server":"required"})),
        "extras.zip":placement("resourcepacks/extra.zip",json!({"client":{"optional":"extra-art","default-enabled":false,"description":"Extra artwork"},"server":"unsupported"}))
    }});
    let selected = || {
        let mut options = options("renderer");
        options.file_plan = Some("files.yml".into());
        options
    };
    // Missing required companions fail the whole request before publication.
    let companion = plan["files"]
        .as_object_mut()
        .unwrap()
        .remove("assets.zip")
        .unwrap();
    fs::write(root.path().join("files.yml"), plan.to_string()).unwrap();
    let before = crate::application::engine_host::tests::snapshot(root.path());
    let error = add_with_catalog(
        &session(root.path(), false),
        selected(),
        ProviderCatalog::for_loopback_tests(&server.url(), None),
    )
    .await
    .unwrap_err();
    assert!(format!("{error:#}").contains("required companion"));
    assert_eq!(
        crate::application::engine_host::tests::snapshot(root.path()),
        before
    );
    plan["files"]["assets.zip"] = companion;
    fs::write(root.path().join("files.yml"), plan.to_string()).unwrap();
    let before = crate::application::engine_host::tests::snapshot(root.path());
    add_with_catalog(
        &session(root.path(), true),
        selected(),
        ProviderCatalog::for_loopback_tests(&server.url(), None),
    )
    .await
    .unwrap();
    assert_eq!(
        crate::application::engine_host::tests::snapshot(root.path()),
        before
    );
    add_with_catalog(
        &session(root.path(), false),
        selected(),
        ProviderCatalog::for_loopback_tests(&server.url(), None),
    )
    .await
    .unwrap();
    let resolved = project(root.path());
    let record = resolved.lock().dependencies.values().next().unwrap();
    assert_eq!(record.files.as_slice().len(), 4);
    let extra = record
        .files
        .as_slice()
        .iter()
        .find(|file| file.slot.as_str() == "extras.zip")
        .unwrap();
    let placement = &extra.placements.as_slice()[0];
    assert_eq!(
        placement.destination.relative().as_str(),
        "resourcepacks/extra.zip"
    );
    assert_eq!(placement.requirements.server, Requirement::Unsupported);
    let Requirement::Optional(choice) = &placement.requirements.client else {
        panic!("lost optional requirement")
    };
    assert_eq!(choice.key.as_str(), "extra-art");
    assert!(!choice.default_enabled);
    for _ in 0..2 {
        let before = crate::application::engine_host::tests::snapshot(&root.path().join("project"));
        synchronize(&session(root.path(), false), false)
            .await
            .unwrap();
        assert_eq!(
            crate::application::engine_host::tests::snapshot(&root.path().join("project")),
            before
        );
    }
    // Authored per-file decisions use the same resolver during a fresh synchronization.
    let mut intent = project(root.path()).intent().clone();
    let key = intent.roots.keys().next().unwrap().clone();
    let empack_core::model::PlacementIntent::ByFile(roles) =
        &mut intent.roots.get_mut(&key).unwrap().placement
    else {
        panic!("lost file decisions")
    };
    let assets = roles
        .remove(&empack_core::model::FileSlot::parse("assets.zip").unwrap())
        .unwrap();
    let write = |intent: &empack_core::model::ProjectIntent| {
        crate::testing::write_authoring(&root.path().join("project/empack.yml"), intent).unwrap()
    };
    write(&intent);
    let base = server.url();
    let services = || dependencies::AdditionServices {
        catalog: ProviderCatalog::for_loopback_tests(&base, None),
        transport: HttpAcquisition::for_loopback_tests()
            .with_test_origin("https://example.invalid", &base),
        files: DirectFileLimits::default(),
    };
    let before = crate::application::engine_host::tests::snapshot(root.path());
    let error = synchronization::synchronize_with_services(
        &session(root.path(), false),
        false,
        services(),
        crate::engine::runtime_catalog::RuntimeCatalog::for_loopback_tests(&server.url()),
    )
    .await
    .unwrap_err();
    assert!(format!("{error:#}").contains("required companion"));
    assert_eq!(
        crate::application::engine_host::tests::snapshot(root.path()),
        before
    );
    let empack_core::model::PlacementIntent::ByFile(roles) =
        &mut intent.roots.get_mut(&key).unwrap().placement
    else {
        unreachable!()
    };
    roles.insert(
        empack_core::model::FileSlot::parse("assets.zip").unwrap(),
        assets,
    );
    let extra = roles
        .get_mut(&empack_core::model::FileSlot::parse("extras.zip").unwrap())
        .unwrap();
    let mut placement = extra.as_slice()[0].clone();
    placement.destination =
        empack_core::path::InstallDestination::parse("resourcepacks/extra-renamed.zip").unwrap();
    *extra = NonEmpty::new(vec![placement.clone()]).unwrap();
    write(&intent);
    let before = crate::application::engine_host::tests::snapshot(root.path());
    for dry in [true, false] {
        synchronization::synchronize_with_services(
            &session(root.path(), dry),
            false,
            services(),
            crate::engine::runtime_catalog::RuntimeCatalog::for_loopback_tests(&server.url()),
        )
        .await
        .unwrap();
        if dry {
            assert_eq!(
                crate::application::engine_host::tests::snapshot(root.path()),
                before
            );
        }
    }
    let resolved = project(root.path());
    assert_eq!(resolved.intent(), &intent);
    let extra = resolved.lock().dependencies[&key]
        .files
        .as_slice()
        .iter()
        .find(|file| file.slot.as_str() == "extras.zip")
        .unwrap();
    assert_eq!(extra.placements.as_slice(), &[placement]);
    let stable = crate::application::engine_host::tests::snapshot(&root.path().join("project"));
    for _ in 0..2 {
        synchronize(&session(root.path(), false), false)
            .await
            .unwrap();
    }
    assert_eq!(
        crate::application::engine_host::tests::snapshot(&root.path().join("project")),
        stable
    );
    let mut value: serde_json::Value = serde_saphyr::from_str(
        &fs::read_to_string(root.path().join("project/empack.yml")).unwrap(),
    )
    .unwrap();
    value["dependencies"][key.as_str()]
        .as_object_mut()
        .unwrap()
        .remove("pin");
    value["build"] = json!({"default":["native-client","native-server","client"],"profiles":{
        "native-client":{"target":"empack","delivery":"bundled","environment":"client"},
        "native-server":{"target":"empack","delivery":"bundled","environment":"server"},
        "client":{"target":"prism","delivery":"bundled","environment":"client"}
    }});
    fs::write(
        root.path().join("project/empack.yml"),
        serde_saphyr::to_string(&value).unwrap(),
    )
    .unwrap();
    let old_catalog = server
        .mock("GET", "/project/Root0001/version")
        .match_query(mockito::Matcher::Any)
        .with_body(json!([first.clone()]).to_string())
        .create_async()
        .await;
    // Materialize the retained selection before offering the changed primary filename.
    synchronization::synchronize_with_services(
        &session(root.path(), false),
        true,
        services(),
        crate::engine::runtime_catalog::RuntimeCatalog::for_loopback_tests(&server.url()),
    )
    .await
    .unwrap();
    old_catalog.remove_async().await;
    server
        .mock("GET", "/project/Root0001/version")
        .match_query(mockito::Matcher::Any)
        .with_body(json!([second.clone(), first]).to_string())
        .create_async()
        .await;
    let before = crate::application::engine_host::tests::snapshot(&root.path().join("project"));
    update::update_with_services(
        &session(root.path(), true),
        vec![key.as_str().into()],
        services(),
    )
    .await
    .unwrap();
    assert_eq!(
        crate::application::engine_host::tests::snapshot(&root.path().join("project")),
        before
    );
    let authored = fs::read(root.path().join("project/empack.yml")).unwrap();
    let bindings = fs::read(root.path().join("project/empack.bindings.yml")).unwrap();
    update::update_with_services(
        &session(root.path(), false),
        vec![key.as_str().into()],
        services(),
    )
    .await
    .unwrap();
    synchronization::synchronize_with_services(
        &session(root.path(), false),
        true,
        services(),
        crate::engine::runtime_catalog::RuntimeCatalog::for_loopback_tests(&server.url()),
    )
    .await
    .unwrap();
    let updated = project(root.path());
    let dependency = &updated.lock().dependencies[&key];
    assert_eq!(
        dependency.selected.as_ref().unwrap().selection,
        empack_core::identity::PinSelector::ModrinthVersion(
            empack_core::identity::ModrinthVersionId::parse("RootVer2").unwrap()
        )
    );
    assert_eq!(dependency.files.as_slice().len(), 4);
    assert_eq!(
        fs::read(root.path().join("project/empack.yml")).unwrap(),
        authored
    );
    assert_eq!(
        fs::read(root.path().join("project/empack.bindings.yml")).unwrap(),
        bindings
    );
    let primary = dependency
        .files
        .as_slice()
        .iter()
        .find(|file| file.slot.as_str() == "primary")
        .unwrap();
    assert_eq!(
        primary.placements.as_slice()[0]
            .destination
            .relative()
            .as_str(),
        "mods/renamed.jar"
    );
    assert_eq!(
        fs::read(root.path().join("project/pack/mods/renamed.jar")).unwrap(),
        payloads["renderer-new.jar"]
    );
    assert!(
        !root
            .path()
            .join("project/pack/mods/renderer-new.jar")
            .exists()
    );
    for name in [
        "renderer-new.jar",
        "assets.zip",
        "extras.zip",
        "server-helper.jar",
    ] {
        let hash =
            empack_core::digest::ExpectedDigest::Sha512(Sha512::digest(&payloads[name]).into())
                .hex();
        server
            .mock("GET", format!("/version_file/{hash}").as_str())
            .match_query(mockito::Matcher::Any)
            .with_body(second.to_string())
            .create_async()
            .await;
    }
    let before = crate::application::engine_host::tests::snapshot(&root.path().join("project"));
    update::adopt_with_services(
        &session(root.path(), false),
        vec![key.as_str().into()],
        services(),
    )
    .await
    .unwrap();
    assert_eq!(
        crate::application::engine_host::tests::snapshot(&root.path().join("project")),
        before
    );
    for _ in 0..2 {
        synchronization::synchronize_with_services(
            &session(root.path(), false),
            true,
            services(),
            crate::engine::runtime_catalog::RuntimeCatalog::for_loopback_tests(&server.url()),
        )
        .await
        .unwrap();
        assert_eq!(
            crate::application::engine_host::tests::snapshot(&root.path().join("project")),
            before
        );
    }
    check_outputs(root.path(), &payloads).await;
}
