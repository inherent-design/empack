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

#[test]
fn installed_runtime_rejects_missing_and_changed_outputs_before_proof() {
    use crate::engine::{snapshot::SnapshotLimits, staging::MutableStage};
    let runtime = runtime("1.12.2", LoaderKind::Forge, "14.23.5.2860");
    let identity = InstallerIdentity::for_runtime(&runtime).unwrap();
    let mut jar = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [
        (
            "META-INF/MANIFEST.MF",
            b"Manifest-Version: 1.0\r\nMain-Class: test.Launcher\r\n\r\n".as_slice(),
        ),
        (
            "test/Launcher.class",
            &[0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 52],
        ),
    ] {
        jar.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        jar.write_all(bytes).unwrap();
    }
    let executable = jar.finish().unwrap().into_inner();
    let digest = ExpectedDigest::Sha1(sha1::Sha1::digest(&executable).into()).hex();
    let relative = maven_path(&identity.coordinate).unwrap();
    let profile = json!({"spec":0,"minecraft":"1.12.2","version":"1.12.2-forge-14.23.5.2860","json":"/version.json","path":identity.coordinate,"libraries":[{"name":identity.coordinate,"downloads":{"artifact":{"path":relative.as_str(),"url":"","sha1":digest,"size":executable.len()}}}]});
    let version = json!({"id":"1.12.2-forge-14.23.5.2860","inheritsFrom":"1.12.2","libraries":[profile["libraries"][0].clone(),{"name":"fixture:dependency:1","downloads":{"artifact":{"path":"fixture/dependency/1/dependency-1.jar","url":"https://example.com/dependency.jar","sha1":digest,"size":executable.len()}}}]});
    let installer = content(
        &profile,
        &version,
        &[(&format!("maven/{}", relative.as_str()), &executable)],
    );
    for failure in [
        "none",
        "missing library",
        "changed library",
        "changed executable",
        "missing generated",
        "changed vanilla",
        "alternative match",
        "wrong alternatives",
    ] {
        let mut contract = InstallerContract::parse(
            runtime.clone(),
            identity.clone(),
            &installer,
            ArchiveLimits::default(),
            &Cancellation::default(),
        )
        .unwrap();
        if failure == "missing generated" {
            contract.generated.insert(
                path("libraries/generated.jar").unwrap(),
                ExpectedContent {
                    digests: None,
                    size: None,
                    accepted_observation: None,
                },
            );
        }
        if matches!(failure, "alternative match" | "wrong alternatives") {
            let library = contract
                .libraries
                .iter_mut()
                .find(|library| library.coordinate == "fixture:dependency:1")
                .unwrap();
            library.expected.digests = None;
            library.acceptable_sha1 = vec![ExpectedDigest::Sha1([0; 20])];
            if failure == "alternative match" {
                library
                    .acceptable_sha1
                    .push(ExpectedDigest::parse("sha1", &digest).unwrap());
            }
        }
        let mut vanilla = crate::engine::server_runtime::tests::prepared_fixture();
        vanilla.runtime.minecraft = runtime.minecraft.clone();
        let base = &vanilla.files[&path("server.jar").unwrap()].content;
        let mut stage = MutableStage::empty().unwrap();
        stage
            .write(
                &contract.minecraft_path,
                &mut base.lease().open(),
                base.lease().len(),
                &Cancellation::default(),
            )
            .unwrap();
        if failure == "changed vanilla" {
            stage
                .write(
                    &contract.minecraft_path,
                    &mut b"wrong".as_slice(),
                    5,
                    &Cancellation::default(),
                )
                .unwrap();
        }
        let InstallerLayout::ExecutableJar { destination, .. } = &contract.layout else {
            panic!("wrong fixture layout")
        };
        let bytes = if failure == "changed executable" {
            b"wrong".as_slice()
        } else {
            executable.as_slice()
        };
        stage
            .write(
                destination,
                &mut &*bytes,
                bytes.len() as u64,
                &Cancellation::default(),
            )
            .unwrap();
        if failure != "missing library" {
            let bytes = if failure == "changed library" {
                b"wrong".as_slice()
            } else {
                executable.as_slice()
            };
            stage
                .write(
                    &contract
                        .libraries
                        .iter()
                        .find(|library| library.coordinate == "fixture:dependency:1")
                        .unwrap()
                        .path,
                    &mut &*bytes,
                    bytes.len() as u64,
                    &Cancellation::default(),
                )
                .unwrap();
        }
        let plan = InstallerServerPlan {
            contract,
            installer: installer.clone(),
            expected: ExpectedContent {
                digests: Some(installer.observed_digests().clone()),
                size: Some(installer.lease().len()),
                accepted_observation: None,
            },
            checksum_document: installer.lease().id(),
        };
        let result = plan.verify_outputs(
            vanilla,
            stage
                .freeze(SnapshotLimits::default(), &Cancellation::default())
                .unwrap(),
            ArchiveLimits::default(),
            &Cancellation::default(),
        );
        if matches!(failure, "none" | "alternative match") {
            let prepared = result.unwrap();
            assert_eq!(prepared.runtime().loader, LoaderKind::Forge);
            assert_eq!(prepared.launcher_main_class(), "test.Launcher");
            assert!(matches!(prepared.launch(), ServerLaunch::Jar(_)));
        } else {
            assert!(result.is_err(), "accepted {failure}");
        }
    }
}

#[test]
fn historical_checksum_alternatives_remain_an_explicit_disjunction() {
    let value = json!({"name":"group:artifact:1","checksums":["ab".repeat(20),"cd".repeat(20)]});
    let library = parse_library(&value).unwrap();
    assert!(library.expected.digests.is_none());
    assert_eq!(library.acceptable_sha1.len(), 2);
    for checksums in [json!([]), json!(["wrong"]), json!("not an array")] {
        let mut wrong = value.clone();
        wrong["checksums"] = checksums;
        assert!(parse_library(&wrong).is_err());
    }
}

#[tokio::test]
async fn remote_installer_inputs_verify_before_tools_and_retain_no_failed_subset() {
    use crate::engine::{
        resources::ResourceGovernor,
        runtime::{OperationOutcome, OperationRuntime},
        snapshot::SnapshotLimits,
    };
    let mut server = mockito::Server::new_async().await;
    let response = server
        .mock("GET", "/library")
        .with_body("payload")
        .expect(8)
        .create_async()
        .await;
    for case in [
        "valid",
        "wrong digest",
        "alternative",
        "wrong alternative",
        "size limit",
    ] {
        let (profile, version) = fixture();
        let installer = content(&profile, &version, &[]);
        let mut contract = parse(&profile, &version).unwrap();
        let library = &mut contract.libraries[0];
        library.download = Some(format!("{}/library", server.url()));
        library.expected.size = Some(7);
        library.expected.digests = Some(
            DigestSet::new(vec![ExpectedDigest::Sha256(
                Sha256::digest(b"payload").into(),
            )])
            .unwrap(),
        );
        let mut first = library.clone();
        first.path = path("libraries/first.jar").unwrap();
        if case == "wrong digest" {
            library.expected.digests =
                Some(DigestSet::new(vec![ExpectedDigest::Sha256([0; 32])]).unwrap());
        }
        if case.contains("alternative") {
            library.expected.digests = None;
            library.acceptable_sha1 = vec![ExpectedDigest::Sha1([0; 20])];
            if case == "alternative" {
                library
                    .acceptable_sha1
                    .push(ExpectedDigest::Sha1(sha1::Sha1::digest(b"payload").into()));
            }
        }
        contract.libraries.insert(0, first);
        let plan = InstallerServerPlan {
            contract,
            expected: ExpectedContent {
                digests: None,
                size: None,
                accepted_observation: None,
            },
            checksum_document: installer.lease().id(),
            installer,
        };
        let governor = ResourceGovernor::new(ResourceRequest {
            jobs: 1,
            memory_bytes: 1 << 20,
            scratch_bytes: 1024,
            open_files: 10,
        });
        let owner = OperationRuntime::new(governor.clone(), 1);
        let mut handle = owner
            .start(move |mut scope| async move {
                Ok(plan
                    .acquire_libraries(
                        &HttpAcquisition::for_loopback_tests(),
                        &mut scope,
                        TransferLimits::default(),
                        SourceEvidencePolicy::Compatibility,
                        SnapshotLimits {
                            total_bytes: if case == "size limit" { 6 } else { 16 },
                            ..SnapshotLimits::default()
                        },
                    )
                    .await)
            })
            .unwrap();
        let outcome = handle.wait().await;
        owner.release_completed(handle.id());
        owner.shutdown().await;
        match &*outcome {
            OperationOutcome::Completed(result) => {
                assert_eq!(
                    result.is_ok(),
                    matches!(case, "valid" | "alternative"),
                    "{case}"
                );
                if let Ok(files) = result {
                    assert_eq!(files.len(), 2);
                }
            }
            _ => panic!("unexpected operation failure"),
        }
        drop(outcome);
        drop(handle);
        assert_eq!(governor.status().reserved, ResourceRequest::default());
    }
    response.assert_async().await;
}
