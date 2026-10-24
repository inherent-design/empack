//! Controlled host journey with literal authoring and independently expected bytes.
use super::*;
use crate::application::engine_host::tests::snapshot;
use sha2::{Digest, Sha256, Sha512};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
};

const AUTHOR: &str = r#"# Keep the exact pin and local authoring during migration.
schema: 5
pack: {id: migration.fixture, name: Migration Fixture, version: '1.0', authors: [Tester]}
runtime:
  minecraft: '1.21.1'
  loader:
    kind: fabric
    pin: '0.16.14'
  java: 21
dependencies:
  root: modrinth:Root0001
  pinned: {source: 'modrinth:Pinned01', pin: PinVer01}
  client: {source: 'modrinth:Client01', side: client}
  settings: {path: vendor/settings.toml, type: config}
build:
  default: [native-client, native-server]
  profiles:
    native-client: {target: empack, delivery: bundled, environment: client}
    native-server: {target: empack, delivery: bundled, environment: server}
"#;
const BINDINGS: &str = r#"# The primary role outlives the provider filename.
schema: 1
bindings:
  root:
    placement:
      files:
        primary:
          - destination: mods/stable-root.jar
            layer: common
            environment: {client: required, server: required}
  settings:
    placement:
      - destination: config/settings.toml
        layer: common
        environment: {client: required, server: required}
"#;
const SETTINGS: &[u8] = b"# Local policy, not provider content.\nenabled = true\n";

fn jar(version: &str) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    zip.start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(
        json!({"schemaVersion":1,"id":"fixture","version":version})
            .to_string()
            .as_bytes(),
    )
    .unwrap();
    zip.finish().unwrap().into_inner()
}

#[derive(Clone, Copy)]
enum UpdateConflict {
    Incoming,
    Retained,
}

async fn populated_migration(retained_compatible: bool, conflict: Option<UpdateConflict>) {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path()).await;
    let project_dir = root.path().join("project");
    fs::create_dir(project_dir.join("vendor")).unwrap();
    fs::write(project_dir.join("vendor/settings.toml"), SETTINGS).unwrap();
    fs::write(project_dir.join("empack.yml"), AUTHOR).unwrap();
    fs::write(project_dir.join("empack.bindings.yml"), BINDINGS).unwrap();
    let mut server = mockito::Server::new_async().await;
    runtime(&mut server).await;
    server.mock("GET", "/fabric/1.21.1")
        .with_body(json!([{"loader":{"version":"0.16.14","stable":true},"intermediary":{"version":"1.21.1"}}]).to_string())
        .create_async().await;
    let required = json!({"project_id":"Library1","version_id":null,"dependency_type":"required"});
    let records = [
        (
            "Root0001",
            "RootOld1",
            vec!["1.21.1"],
            vec![required.clone()],
        ),
        (
            "Root0001",
            "RootMid1",
            vec!["1.21.1"],
            vec![required.clone()],
        ),
        ("Root0001", "RootNew1", vec!["1.21.2"], vec![]),
        (
            "Library1",
            "LibOld01",
            if retained_compatible {
                vec!["1.21.1", "1.21.2"]
            } else {
                vec!["1.21.1"]
            },
            vec![],
        ),
        ("Pinned01", "PinVer01", vec!["1.21.1", "1.21.2"], vec![]),
        // A newer compatible selection must not replace the authored exact pin.
        ("Pinned01", "PinVer02", vec!["1.21.1", "1.21.2"], vec![]),
        ("Client01", "ClientV1", vec!["1.21.1", "1.21.2"], vec![]),
    ];
    let mut catalogs = BTreeMap::<&str, Vec<serde_json::Value>>::new();
    let mut payloads = BTreeMap::new();
    for (id, version, games, mut dependencies) in records {
        match (conflict, version) {
            (Some(UpdateConflict::Incoming), "RootMid1") => dependencies.push(
                json!({"project_id":"Pinned01","dependency_type":"incompatible"}),
            ),
            (Some(UpdateConflict::Retained), "ClientV1") => dependencies.push(
                json!({"project_id":"Root0001","version_id":"RootMid1","dependency_type":"incompatible"}),
            ),
            _ => {}
        }
        let bytes = jar(version);
        let value = json!({"id":version,"project_id":id,"game_versions":games,"loaders":["fabric"],
            "files":[{"filename":format!("{version}.jar"),"primary":true,"size":bytes.len(),
                "hashes":{"sha512":empack_core::digest::ExpectedDigest::Sha512(Sha512::digest(&bytes).into()).hex()},"url":format!("{}/files/{version}.jar",server.url())}],
            "dependencies":dependencies,"date_published":if matches!(version, "PinVer02" | "RootMid1") {"2026-02-01T00:00:00Z"} else {"2026-01-01T00:00:00Z"},
            "status":"listed","version_type":"release"});
        server
            .mock("GET", format!("/files/{version}.jar").as_str())
            .with_body(bytes.clone())
            .create_async()
            .await;
        server
            .mock("GET", format!("/version/{version}").as_str())
            .with_body(value.to_string())
            .create_async()
            .await;
        catalogs.entry(id).or_default().push(value);
        payloads.insert(version, bytes);
    }
    let updated_root_catalog = catalogs["Root0001"].clone();
    let mut initial_root_catalog = None;
    for (id, versions) in catalogs {
        server.mock("GET", format!("/project/{id}").as_str())
            .with_body(json!({"id":id,"slug":id.to_lowercase(),"title":id,"project_type":"mod","loaders":["fabric"]}).to_string())
            .create_async().await;
        let initial: Vec<_> = versions
            .into_iter()
            .filter(|version| version["id"] != "RootMid1")
            .collect();
        let catalog = server
            .mock("GET", format!("/project/{id}/version").as_str())
            .match_query(mockito::Matcher::Any)
            .with_body(json!(initial).to_string())
            .create_async()
            .await;
        if id == "Root0001" {
            initial_root_catalog = Some(catalog);
        }
    }
    synchronization::synchronize_with_services(
        &session(root.path(), false),
        true,
        services(&server),
        RuntimeCatalog::for_loopback_tests(&server.url()),
    )
    .await
    .unwrap();
    let initial = project(root.path());
    assert_eq!(initial.intent().roots.len(), 4);
    assert_eq!(initial.lock().dependencies.len(), 5);
    assert_eq!(
        fs::read(project_dir.join("pack/mods/stable-root.jar")).unwrap(),
        payloads["RootOld1"]
    );
    initial_root_catalog.unwrap().remove_async().await;
    server
        .mock("GET", "/project/Root0001/version")
        .match_query(mockito::Matcher::Any)
        .with_body(json!(updated_root_catalog).to_string())
        .create_async()
        .await;
    let before_update = snapshot(&project_dir);
    for dry in [true, false] {
        let (host, messages) = session(root.path(), dry).record_status_messages();
        let result = super::super::super::update::update_with_services(
            &host,
            vec!["root".into(), "pinned".into()],
            services(&server),
        )
        .await;
        let messages = messages.lock().unwrap();
        if conflict.is_some() {
            let error = format!("{:#}", result.unwrap_err());
            assert!(error.contains("incompatib"), "{error}");
            assert_eq!(snapshot(&project_dir), before_update);
            assert!(
                !messages
                    .iter()
                    .any(|message| message.starts_with("Checked "))
            );
            continue;
        }
        result.unwrap();
        for expected in [
            "Check root as root: selection changed",
            "Check pinned as pinned: selection retained",
        ] {
            assert!(
                messages.iter().any(|message| message == expected),
                "{messages:?}"
            );
        }
        let receipt = "Checked 2 selected dependencies: 1 selections changed, 1 retained; preserved authored intent";
        assert_eq!(
            messages
                .iter()
                .filter(|message| message.as_str() == receipt)
                .count(),
            usize::from(!dry)
        );
        if dry {
            assert_eq!(snapshot(&project_dir), before_update);
        }
    }
    if conflict.is_some() {
        return;
    }
    synchronization::synchronize_with_services(
        &session(root.path(), false),
        true,
        services(&server),
        RuntimeCatalog::for_loopback_tests(&server.url()),
    )
    .await
    .unwrap();
    let old = project(root.path());
    assert_eq!(old.intent(), initial.intent());
    assert_eq!(old.lock().runtime, initial.lock().runtime);
    assert_eq!(old.lock().required_edges, initial.lock().required_edges);
    assert_eq!(
        old.lock().dependencies.len(),
        initial.lock().dependencies.len()
    );
    for (key, dependency) in &initial.lock().dependencies {
        if key.as_str() != "root" {
            assert_eq!(
                &old.lock().dependencies[key],
                dependency,
                "retained {key:?}"
            );
        }
    }
    assert_eq!(
        fs::read(project_dir.join("empack.yml")).unwrap(),
        AUTHOR.as_bytes()
    );
    assert_eq!(
        fs::read(project_dir.join("empack.bindings.yml")).unwrap(),
        BINDINGS.as_bytes()
    );
    assert_eq!(
        fs::read(project_dir.join("vendor/settings.toml")).unwrap(),
        SETTINGS
    );
    assert_eq!(
        fs::read(project_dir.join("pack/mods/stable-root.jar")).unwrap(),
        payloads["RootMid1"]
    );
    assert!(!project_dir.join("pack/mods/RootMid1.jar").exists());
    let before = snapshot(&project_dir);
    let mut plans = Vec::new();
    // Preview, dry-run and execution independently resolve the same frozen inputs.
    for (plan, dry) in [(true, false), (false, true), (false, false)] {
        let (host, messages) = session(root.path(), dry).record_messages();
        let mut options = args(plan);
        options.json = true;
        let result = synchronization::synchronize_mode(
            &host,
            true,
            services(&server),
            RuntimeCatalog::for_loopback_tests(&server.url()),
            Some(MigrationOptions::parse(options).unwrap()),
        )
        .await;
        if !retained_compatible {
            let error = format!("{:#}", result.unwrap_err());
            assert!(
                error.contains("library1") && error.contains("incompatible"),
                "{error}"
            );
            assert_eq!(snapshot(&project_dir), before);
            continue;
        }
        result.unwrap();
        let reports: Vec<serde_json::Value> = messages
            .lock()
            .unwrap()
            .iter()
            .filter_map(|message| serde_json::from_str(message).ok())
            .filter(|value: &serde_json::Value| value.get("proposed_runtime").is_some())
            .collect();
        assert_eq!(
            reports.len(),
            1,
            "one displayed migration report per invocation"
        );
        plans.push(reports[0].clone());
        if plan || dry {
            assert_eq!(snapshot(&project_dir), before);
        }
    }
    if !retained_compatible {
        return;
    }
    assert_eq!(plans[0], plans[1]);
    assert_eq!(plans[0], plans[2]);
    let report = &plans[0];
    assert_eq!(report["current_runtime"]["minecraft"], "1.21.1");
    assert_eq!(report["proposed_runtime"]["minecraft"], "1.21.2");
    let rows = report["dependencies"].as_array().unwrap();
    assert_eq!(rows.len(), 5);
    let row = |key: &str| rows.iter().find(|r| r["key"] == key).unwrap();
    assert_eq!(row("root")["before"]["selection"]["selection"], "RootMid1");
    assert_eq!(row("root")["after"]["selection"]["selection"], "RootNew1");
    assert_eq!(
        row("root")["after"]["placements"][0]["destination"],
        "mods/stable-root.jar"
    );
    assert_eq!(row("root")["after"]["placements"][0]["file"], "primary");
    let announced_file = &row("root")["after"]["files"][0];
    assert_eq!(
        announced_file["expected"]["size"],
        payloads["RootNew1"].len()
    );
    assert_eq!(
        announced_file["expected"]["digests"]["sha512"],
        empack_core::digest::ExpectedDigest::Sha512(Sha512::digest(&payloads["RootNew1"]).into())
            .hex()
    );
    assert_eq!(
        row("library1")["requirement"],
        "requirement-unknown-retained"
    );
    assert_eq!(
        row("library1")["after"]["selection"]["selection"],
        "LibOld01"
    );
    assert_eq!(row("pinned")["after"]["selection"]["selection"], "PinVer01");
    assert_eq!(
        row("client")["after"]["placements"][0]["server"]["state"],
        "unsupported"
    );
    assert_eq!(row("settings")["change"], "retained");
    // The local source has no provider dependency declaration. Losing the known
    // root edge therefore cannot establish complete dependency coverage.
    assert_eq!(
        row("library1")["before"]["reason_chain"],
        json!(["root", "library1"])
    );
    assert!(row("library1")["after"]["reason_chain"].is_null());
    assert_eq!(row("settings")["manual_compatibility"], true);

    let next = project(root.path());
    assert_eq!(next.intent().roots, old.intent().roots);
    assert_eq!(next.lock().runtime.minecraft.as_str(), "1.21.2");
    for (key, version) in [
        ("root", "RootNew1"),
        ("library1", "LibOld01"),
        ("pinned", "PinVer01"),
        ("client", "ClientV1"),
    ] {
        let entry = &next.lock().dependencies[&DependencyKey::parse(key).unwrap()];
        assert_eq!(
            entry.selected.as_ref().unwrap().selection,
            entry
                .selected
                .as_ref()
                .unwrap()
                .project
                .parse_pin(version)
                .unwrap()
        );
    }
    // Literal destinations and bytes are the oracle, not a second call to the planner.
    let expected = BTreeMap::from([
        ("config/settings.toml", SETTINGS.to_vec()),
        ("mods/stable-root.jar", payloads["RootNew1"].clone()),
        ("mods/LibOld01.jar", payloads["LibOld01"].clone()),
        ("mods/PinVer01.jar", payloads["PinVer01"].clone()),
        ("mods/ClientV1.jar", payloads["ClientV1"].clone()),
    ]);
    let actual = snapshot(&project_dir.join("pack"));
    assert_eq!(
        actual,
        expected
            .clone()
            .into_iter()
            .map(|(path, bytes)| (std::path::PathBuf::from(path), bytes))
            .collect()
    );
    assert_eq!(
        fs::read(project_dir.join("pack/mods/ClientV1.jar")).unwrap(),
        payloads["ClientV1"]
    );
    assert!(
        !project_dir
            .join("overrides/server/mods/ClientV1.jar")
            .exists()
    );
    assert_eq!(
        fs::read(project_dir.join("vendor/settings.toml")).unwrap(),
        SETTINGS
    );
    assert_eq!(
        fs::read_to_string(project_dir.join("empack.bindings.yml")).unwrap(),
        BINDINGS
    );
    assert!(
        fs::read_to_string(project_dir.join("empack.yml"))
            .unwrap()
            .starts_with("# Keep the exact pin")
    );
    crate::application::engine_host::build::build(
        &session(root.path(), false),
        &crate::application::BuildArgs::default(),
        crate::application::engine_host::build::BuildDecisions::default(),
        crate::engine::build::BuildAcquisitions::default(),
    )
    .await
    .unwrap();
    let archives: Vec<_> = fs::read_dir(project_dir.join("dist"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(archives.len(), 2);
    for side in ["client", "server"] {
        let suffix = format!("empack-bundled-{side}.empack");
        let path = archives
            .iter()
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .ends_with(&suffix)
            })
            .unwrap();
        let mut archive = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
        let mut descriptor = Vec::new();
        archive
            .by_name("release.json")
            .unwrap()
            .read_to_end(&mut descriptor)
            .unwrap();
        let release: serde_json::Value = serde_json::from_slice(&descriptor).unwrap();
        let files = release["files"].as_array().unwrap();
        let side_expected: BTreeMap<_, _> = expected
            .iter()
            .filter(|(name, _)| side == "client" || **name != "mods/ClientV1.jar")
            .map(|(name, bytes)| (*name, bytes))
            .collect();
        assert_eq!(
            files.len(),
            side_expected.len(),
            "no duplicate destinations"
        );
        let destinations: std::collections::BTreeSet<_> = files
            .iter()
            .map(|file| file["destination"].as_str().unwrap())
            .collect();
        assert_eq!(
            destinations,
            side_expected.keys().copied().collect(),
            "{side} destinations"
        );
        for file in files {
            let bytes = side_expected[file["destination"].as_str().unwrap()];
            let digest =
                empack_core::digest::ExpectedDigest::Sha256(Sha256::digest(bytes).into()).hex();
            assert_eq!(file["sha256"], digest);
            assert_eq!(file["bytes"], bytes.len());
            let mut actual = Vec::new();
            archive
                .by_name(&format!("assets/{digest}"))
                .unwrap()
                .read_to_end(&mut actual)
                .unwrap();
            assert_eq!(&actual, bytes);
        }
    }
    let committed = snapshot(&project_dir);
    for _ in 0..2 {
        synchronization::synchronize_with_services(
            &session(root.path(), false),
            true,
            services(&server),
            RuntimeCatalog::for_loopback_tests(&server.url()),
        )
        .await
        .unwrap();
        assert_eq!(snapshot(&project_dir), committed);
    }
}

#[tokio::test]
async fn host_populated_migration_matches_displayed_plan_and_materialized_inventory() {
    populated_migration(true, None).await;
}
#[tokio::test]
async fn host_populated_migration_refuses_incompatible_retained_library_without_edits() {
    populated_migration(false, None).await;
}

#[tokio::test]
async fn host_selected_update_refuses_incoming_and_retained_incompatibilities_without_edits() {
    for conflict in [UpdateConflict::Incoming, UpdateConflict::Retained] {
        populated_migration(true, Some(conflict)).await;
    }
}
