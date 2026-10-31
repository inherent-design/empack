//! Captured provider records exercise export interpretation without live catalogs.
//! The seven-byte payload is a protocol fixture, not runnable provider content.
use crate::files::snapshot;
use empack_core::{
    digest::DigestSet,
    model::{
        AcquisitionSpec, DependencyKey, FileSlot, NonEmpty, Placement, PlacementIntent,
        ResolvedProject,
    },
    path::InstallDestination,
    requirements::{ChoiceKey, OptionalChoice, Requirement, Requirements},
};
use empack_lib::engine::documents::DocumentCodec;
use empack_tests::{
    fixtures::restricted,
    harness::{
        archive::zip_members,
        process::{TestProject, assert_dist_artifact_suffix, offline_command},
    },
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs};

const NAME: &str = "provider-exports";
const URL: &str = "https://example.invalid/manual.zip";
const SHA1: &str = "f07e5a815613c5abeddc4b682247a4c42d8a95df";
const SHA512: &str = "70b33ce9c9047e30f917e7ea13e42f7767008c3f4f9c9baf49e4390fc625549e9625eee39b94545074e8a1824cf3f238463b11bc03d97348e0fc2999ca1fff7f";

struct Fixture {
    project: TestProject,
    host: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let fixture = Self {
            project: restricted::fixture(NAME),
            host: tempfile::tempdir().unwrap(),
        };
        for (name, bytes) in [
            ("pack/common.txt", b"common bytes".as_slice()),
            ("pack/config/options.txt", b"common configuration"),
            (
                "overrides/client/config/options.txt",
                b"client configuration",
            ),
            ("overrides/server/server-only.txt", b"server bytes"),
            ("dist/prior.zip", b"prior artifact"),
        ] {
            let path = fixture.project.dir().join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
        fixture.capture_record(false, "manual.zip");
        fixture
    }

    /// Component setup changes only this fresh fixture's captured DTOs. Real
    /// commands below receive no endpoint override or production fault control.
    fn capture_record(&self, optional: bool, filename: &str) {
        let root = self.project.dir();
        let document = empack_lib::testing::read_authoring(&root.join("empack.yml")).unwrap();
        let resolved = DocumentCodec
            .decode_lock(
                &fs::read(root.join("empack.lock")).unwrap(),
                &document,
                "fixture",
            )
            .unwrap();
        let mut intent = resolved.intent().clone();
        let mut lock = resolved.lock().clone();
        let key = DependencyKey::parse("manual-assets").unwrap();
        let requirements = Requirements {
            client: if optional {
                Requirement::Optional(OptionalChoice {
                    key: ChoiceKey::parse("art").unwrap(),
                    default_enabled: false,
                    description: Some("Optional artwork".into()),
                })
            } else {
                Requirement::Required
            },
            server: Requirement::Unsupported,
        };
        let placements = NonEmpty::new(vec![Placement {
            destination: InstallDestination::parse("resourcepacks/manual.zip").unwrap(),
            layer: empack_core::model::ContentLayer::Common,
            requirements: requirements.clone(),
        }])
        .unwrap();
        let dependency = intent.roots.get_mut(&key).unwrap();
        dependency.requirements = requirements;
        dependency.placement = PlacementIntent::Explicit(placements.clone());
        let dependency = lock.dependencies.get_mut(&key).unwrap();
        let pin = dependency.selected.clone().unwrap();
        let mut file = dependency.files.as_slice()[0].clone();
        file.acquisition = AcquisitionSpec::Provider {
            pin,
            slot: FileSlot::parse(filename).unwrap(),
            alternatives: vec![URL.into()],
        };
        let digests = DigestSet::parse([("sha1", SHA1), ("sha512", SHA512)]).unwrap();
        file.expected.digests = Some(digests.clone());
        file.provenance.declared_digests = Some(digests);
        file.placements = placements;
        dependency.files = NonEmpty::new(vec![file]).unwrap();
        let bytes = DocumentCodec.encode_intent(&intent).unwrap();
        let revision = DocumentCodec
            .decode_intent(&bytes, "fixture")
            .unwrap()
            .semantic_revision();
        lock.intent_revision = revision;
        let resolved = ResolvedProject::validate(intent, lock, revision).unwrap();
        fs::write(root.join("empack.yml"), bytes).unwrap();
        fs::write(
            root.join("empack.lock"),
            DocumentCodec.encode_lock(&resolved).unwrap(),
        )
        .unwrap();
    }

    fn command(&self, consumer: &str, extra: &[&str]) -> assert_cmd::Command {
        let mut command = offline_command(self.project.dir());
        command
            .arg("--state-dir")
            .arg(self.host.path().join("state"))
            .arg("--cache-dir")
            .arg(self.host.path().join("cache"))
            .args([
                "--yes",
                "build",
                "--ad-hoc",
                consumer,
                "--delivery",
                "references",
            ])
            .args(extra);
        command
    }

    fn check(&self, consumer: &str, choice: Option<bool>) -> std::path::PathBuf {
        let root = self.project.dir();
        let suffix = if consumer == "modrinth" {
            ".mrpack"
        } else {
            "-curseforge.zip"
        };
        let artifact = assert_dist_artifact_suffix(root, suffix);
        let mut members = zip_members(&artifact);
        let enabled = choice != Some(false);
        let optional = choice.is_none();
        if consumer == "modrinth" {
            let actual: Value =
                serde_json::from_slice(&members.remove("modrinth.index.json").unwrap()).unwrap();
            assert_eq!(
                actual,
                json!({
                    "formatVersion":1, "game":"minecraft", "versionId":"1.0.0", "name":NAME,
                    "dependencies":{"minecraft":"1.21.1","fabric-loader":"0.15.0"},
                    "files":if enabled { json!([{
                        "path":"resourcepacks/manual.zip", "downloads":[URL], "fileSize":7,
                        "hashes":{"sha1":SHA1,"sha512":SHA512},
                        "env":{"client":if optional {"optional"} else {"required"},"server":"unsupported"}
                    }]) } else { json!([]) }
                })
            );
            assert_eq!(
                members,
                BTreeMap::from([
                    ("overrides/common.txt".into(), b"common bytes".to_vec()),
                    (
                        "overrides/config/options.txt".into(),
                        b"common configuration".to_vec()
                    ),
                    (
                        "client-overrides/config/options.txt".into(),
                        b"client configuration".to_vec()
                    ),
                    (
                        "server-overrides/server-only.txt".into(),
                        b"server bytes".to_vec()
                    ),
                ])
            );
        } else {
            let actual: Value =
                serde_json::from_slice(&members.remove("manifest.json").unwrap()).unwrap();
            assert_eq!(
                actual,
                json!({
                    "manifestType":"minecraftModpack", "manifestVersion":1,
                    "name":NAME, "version":"1.0.0", "author":"Workflow Test",
                    "minecraft":{"version":"1.21.1", "modLoaders":[{"id":"fabric-0.15.0","primary":true}]},
                    "files":if enabled { json!([{"projectID":123,"fileID":456,"required":!optional}]) } else {json!([])},
                    "overrides":"overrides"
                })
            );
            assert_eq!(
                members,
                BTreeMap::from([
                    ("overrides/common.txt".into(), b"common bytes".to_vec()),
                    (
                        "overrides/config/options.txt".into(),
                        b"client configuration".to_vec()
                    ),
                ])
            );
        }
        artifact
    }

    fn build(&self, consumer: &str, extra: &[&str], choice: Option<bool>) {
        let root = self.project.dir();
        let mut before = snapshot(root);
        self.command(consumer, extra).assert().success();
        let artifact = self.check(consumer, choice);
        before.insert(
            artifact.strip_prefix(root).unwrap().to_owned(),
            fs::read(&artifact).unwrap(),
        );
        assert_eq!(
            snapshot(root),
            before,
            "build changed author inputs or prior outputs"
        );
    }

    fn build_mixed(&self, enabled: bool) {
        let root = self.project.dir();
        let mut before = snapshot(root);
        self.command(
            "modrinth",
            &[
                "curseforge",
                "--optional",
                if enabled { "art=true" } else { "art=false" },
                "--allow-optional-metadata-loss",
            ],
        )
        .assert()
        .success();
        for (consumer, choice) in [("modrinth", None), ("curseforge", Some(enabled))] {
            let artifact = self.check(consumer, choice);
            before.insert(
                artifact.strip_prefix(root).unwrap().to_owned(),
                fs::read(&artifact).unwrap(),
            );
        }
        assert_eq!(snapshot(root), before);
    }
}

#[test]
fn cli_provider_reference_exports_preserve_manifests_choices_and_refusal_effects() {
    let fixture = Fixture::new();
    for consumer in ["modrinth", "curseforge"] {
        fixture.build(consumer, &[], Some(true));
    }
    fixture.capture_record(true, "manual.zip");
    for consumer in ["modrinth", "curseforge"] {
        let before = snapshot(fixture.project.dir());
        fixture
            .command(consumer, &[])
            .assert()
            .failure()
            .stderr(predicates::str::contains("metadata"));
        assert_eq!(snapshot(fixture.project.dir()), before);
        fixture.build(consumer, &["--allow-optional-metadata-loss"], None);
        if consumer == "curseforge" {
            fixture.build(consumer, &["--optional", "art=true"], Some(true));
            fixture.build(consumer, &["--optional-defaults"], Some(false));
            fixture.build(consumer, &["--optional", "art=false"], Some(false));
            let before = snapshot(fixture.project.dir());
            fixture
                .command(consumer, &["--optional", "typo=true"])
                .assert()
                .failure()
                .stderr(predicates::str::contains("typo"));
            assert_eq!(snapshot(fixture.project.dir()), before);
        } else {
            for flags in [
                vec!["--optional", "art=true", "--allow-optional-metadata-loss"],
                vec!["--optional-defaults", "--allow-optional-metadata-loss"],
                vec!["--optional", "art=false"],
            ] {
                for explain in [false, true] {
                    let before = snapshot(fixture.project.dir());
                    let host = snapshot(fixture.host.path());
                    let mut command = fixture.command(consumer, &flags);
                    if explain {
                        command.args(["--explain", "--json"]);
                    }
                    command
                        .assert()
                        .failure()
                        .stderr(predicates::str::contains("preserve optional participation"));
                    assert_eq!(snapshot(fixture.project.dir()), before);
                    assert_eq!(snapshot(fixture.host.path()), host);
                }
            }
        }
    }
    fixture.build_mixed(true);
    fixture.build_mixed(false);
    let before = snapshot(fixture.project.dir());
    let host = snapshot(fixture.host.path());
    let report = fixture
        .command(
            "modrinth",
            &["prism", "--optional", "art=true", "--explain", "--json"],
        )
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: Value = serde_json::from_slice(&report).unwrap();
    let resolve = json!({"mode":"resolve","choices":{"art":true},"use_defaults":false});
    assert_eq!(report["optional_policy"], resolve);
    let profiles = report["profiles"].as_array().unwrap();
    assert_eq!(profiles.len(), 2);
    for profile in profiles {
        assert_eq!(
            profile["optional_policy"],
            if profile["recipe"]["consumer"] == "modrinth" {
                json!({"mode":"preserve"})
            } else {
                assert_eq!(profile["recipe"]["consumer"], "prism");
                resolve.clone()
            }
        );
    }
    assert_eq!(snapshot(fixture.project.dir()), before);
    assert_eq!(snapshot(fixture.host.path()), host);
    fixture.capture_record(false, "original.zip");
    let before = snapshot(fixture.project.dir());
    fixture
        .command("curseforge", &[])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "cannot preserve a renamed or relocated provider file",
        ));
    assert_eq!(snapshot(fixture.project.dir()), before);
    fixture
        .command("modrinth", &["curseforge"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "cannot preserve a renamed or relocated provider file",
        ));
    assert_eq!(
        snapshot(fixture.project.dir()),
        before,
        "a late consumer refusal must preserve both outputs"
    );
    // Generic mrpack references can retain the authored destination explicitly.
    fixture.build("modrinth", &[], Some(true));
}
