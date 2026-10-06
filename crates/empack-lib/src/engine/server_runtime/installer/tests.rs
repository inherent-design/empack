use super::*;
use crate::engine::content::verify_stream;
use empack_core::model::{GameVersion, LoaderVersion};
use serde_json::json;
use std::io::{Cursor, Write};
fn runtime(game: &str, loader: LoaderKind, version: &str) -> RuntimeResolution {
    RuntimeResolution {
        minecraft: GameVersion::parse(game).unwrap(),
        loader,
        loader_version: Some(LoaderVersion::parse(version).unwrap()),
    }
}
fn content(profile: &Value, version: &Value, extras: &[(&str, &[u8])]) -> AcquiredContent {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [
        ("install_profile.json", serde_json::to_vec(profile).unwrap()),
        ("version.json", serde_json::to_vec(version).unwrap()),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&bytes).unwrap();
    }
    for (name, bytes) in extras {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    let bytes = zip.finish().unwrap().into_inner();
    verify_stream(
        &mut bytes.as_slice(),
        &ExpectedContent {
            digests: None,
            size: None,
            accepted_observation: None,
        },
        2 << 20,
        SourceEvidencePolicy::Compatibility,
        InitialObservation::Accepted,
        &Cancellation::default(),
    )
    .unwrap()
}
fn fixture() -> (Value, Value) {
    let library = json!({"name":"group:artifact:1:all@zip","downloads":{"artifact":{"path":"group/artifact/1/artifact-1-all.zip","url":"https://example.com/library.zip","size":10,"sha1":"00".repeat(20)}}});
    (
        json!({"spec":1,"minecraft":"1.20.1","version":"1.20.1-forge-47.4.0","json":"/version.json","libraries":[library.clone()],"data":{"OUTPUT":{"server":"[group:generated:1]"},"HASH":{"server":format!("'{}'", "ab".repeat(20))}},"processors":[{"outputs":{"{OUTPUT}":"{HASH}"}}]}),
        json!({"id":"1.20.1-forge-47.4.0","inheritsFrom":"1.20.1","libraries":[library]}),
    )
}
fn parse(profile: &Value, version: &Value) -> Result<InstallerContract> {
    let runtime = runtime("1.20.1", LoaderKind::Forge, "47.4.0");
    InstallerContract::parse(
        runtime.clone(),
        InstallerIdentity::for_runtime(&runtime)?,
        &content(
            profile,
            version,
            &[
                ("data/unix_args.txt", b"main"),
                ("data/win_args.txt", b"main"),
            ],
        ),
        ArchiveLimits::default(),
        &Cancellation::default(),
    )
}
#[test]
fn exact_installer_coordinates_preserve_historical_artifact_families() {
    for (game, loader, version, coordinate) in [
        (
            "1.20.1",
            LoaderKind::Forge,
            "47.4.0",
            "net.minecraftforge:forge:1.20.1-47.4.0",
        ),
        (
            "1.7.10",
            LoaderKind::Forge,
            "10.13.4.1614",
            "net.minecraftforge:forge:1.7.10-10.13.4.1614-1.7.10",
        ),
        (
            "1.7.10",
            LoaderKind::Forge,
            "10.13.4.1614-1.7.10",
            "net.minecraftforge:forge:1.7.10-10.13.4.1614-1.7.10",
        ),
        (
            "1.7.10",
            LoaderKind::Forge,
            "10.13.2.1291",
            "net.minecraftforge:forge:1.7.10-10.13.2.1291",
        ),
        (
            "1.20.1",
            LoaderKind::NeoForge,
            "47.1.106",
            "net.neoforged:forge:1.20.1-47.1.106",
        ),
        (
            "1.21.1",
            LoaderKind::NeoForge,
            "21.1.209",
            "net.neoforged:neoforge:21.1.209",
        ),
    ] {
        let identity = InstallerIdentity::for_runtime(&runtime(game, loader, version)).unwrap();
        assert_eq!(identity.coordinate, coordinate);
        assert!(identity.url.ends_with("-installer.jar"));
    }
    assert!(
        InstallerIdentity::for_runtime(&runtime("1.20.1", LoaderKind::Fabric, "0.16.0")).is_err()
    );
    for coordinate in [
        "group:../outside:1",
        "group:artifact:1@../../zip",
        "group::1",
        "group:artifact:1:extra:more",
    ] {
        assert!(maven_path(coordinate).is_err());
    }
}
#[test]
fn profile_binds_runtime_libraries_and_generated_output_evidence() {
    let (profile, version) = fixture();
    let plan = parse(&profile, &version).unwrap();
    assert_eq!(plan.libraries.len(), 1);
    assert_eq!(
        plan.libraries[0].path.as_str(),
        "libraries/group/artifact/1/artifact-1-all.zip"
    );
    assert_eq!(plan.generated.len(), 1);
    assert!(
        plan.generated
            .contains_key(&path("libraries/group/generated/1/generated-1.jar").unwrap())
    );
    assert!(matches!(plan.layout, InstallerLayout::Arguments { .. }));
    for field in ["minecraft", "version", "json"] {
        let mut wrong = profile.clone();
        wrong[field] = "wrong".into();
        assert!(parse(&wrong, &version).is_err());
    }
    let mut wrong = version.clone();
    wrong["inheritsFrom"] = "1.19".into();
    assert!(parse(&profile, &wrong).is_err());
    let mut wrong = version.clone();
    wrong["libraries"][0]["downloads"]["artifact"]["sha1"] = "ff".repeat(20).into();
    assert!(parse(&profile, &wrong).is_err());
    let mut wrong = profile.clone();
    wrong["libraries"][0]["downloads"]["artifact"]["path"] = "../outside".into();
    assert!(parse(&wrong, &version).is_err());
    let mut wrong = profile.clone();
    wrong["data"]["OUTPUT"]["server"] = "{ROOT}/../outside".into();
    assert!(parse(&wrong, &version).is_err());
    let mut wrong = profile.clone();
    wrong["serverJarPath"] = "{UNKNOWN}/server.jar".into();
    assert!(parse(&wrong, &version).is_err());
}
#[test]
fn legacy_profile_requires_exact_embedded_executable_without_inventing_hashes() {
    let runtime = runtime("1.7.10", LoaderKind::Forge, "10.13.4.1614");
    let identity = InstallerIdentity::for_runtime(&runtime).unwrap();
    let mut profile = json!({"install":{"minecraft":"1.7.10","path":identity.coordinate,"filePath":"forge-universal.jar"},"versionInfo":{"inheritsFrom":"1.7.10","libraries":[{"name":"group:artifact:1","serverreq":true}]}});
    let parse = |profile: &Value, extras: &[(&str, &[u8])]| {
        InstallerContract::parse(
            runtime.clone(),
            identity.clone(),
            &content(profile, &Value::Null, extras),
            ArchiveLimits::default(),
            &Cancellation::default(),
        )
    };
    assert!(parse(&profile, &[]).is_err());
    let plan = parse(&profile, &[("forge-universal.jar", b"fixture")]).unwrap();
    assert!(plan.libraries[0].expected.digests.is_none());
    assert!(matches!(plan.layout, InstallerLayout::ExecutableJar { .. }));
    assert_eq!(plan.minecraft_path.as_str(), "minecraft_server.1.7.10.jar");
    profile["install"]["filePath"] = "../outside.jar".into();
    assert!(parse(&profile, &[]).is_err());
}
