//! Document-only selection edits over a fixed, offline provider resolution.
use empack_core::{
    digest::{DigestSet, ExpectedDigest},
    identity::{ModrinthProjectId, ModrinthVersionId, PinSelector, ProviderProjectId},
    model::*,
    path::{InstallDestination, PathSyntax, PortableRelPath},
    requirements::{Requirement, Requirements},
};
use empack_lib::engine::documents::DocumentCodec;
use empack_tests::harness::process::{TestProject, offline_command, read_project};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::Path};

#[path = "support/files.rs"]
mod files;

const AUTHOR: &str = r#"# Keep this author's selection notes.
schema: 5
pack:
  id: selection.fixture
  name: &title Selection fixture
  description: *title
  version: '1'
  authors: [Fixture]
runtime: {minecraft: '1.21.1', loader: vanilla, java: 21}
dependency-policy: {releases: stable-only}
dependencies:
  renderer-alias:
    source: modrinth:Project1 # Keep the canonical source and logical alias.
    type: resource-pack
    releases: any
  local:
    path: vendor/local.zip
    type: resource-pack
build: {default: [], profiles: {}}
"#;
const BINDINGS: &str = r#"# Keep the custom destination independent of provider filenames.
schema: 1
bindings:
  renderer-alias:
    placement:
      - destination: resourcepacks/custom-name.zip
        layer: common
        environment: {client: required, server: required}
"#;

fn write(root: &Path, name: &str, bytes: &[u8]) {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

// The lock is component setup, not evidence of provider resolution. The author
// inputs and expected file inventory are literal and independent of the writer.
fn fixture(root: &Path) -> ResolvedProject {
    let source = DocumentCodec
        .decode_authoring(
            AUTHOR.as_bytes(),
            Some(BINDINGS.as_bytes()),
            "selection fixture",
        )
        .unwrap();
    let provider = ProviderProjectId::Modrinth(ModrinthProjectId::parse("Project1").unwrap());
    let selected = ResolvedPin {
        project: provider.clone(),
        selection: PinSelector::ModrinthVersion(ModrinthVersionId::parse("Version1").unwrap()),
    };
    let mut dependencies = BTreeMap::new();
    let mut coverage = BTreeMap::new();
    for (name, destination, bytes) in [
        (
            "renderer-alias",
            "resourcepacks/custom-name.zip",
            b"retained provider bytes".as_slice(),
        ),
        (
            "local",
            "resourcepacks/local.zip",
            b"retained local bytes".as_slice(),
        ),
    ] {
        let key = DependencyKey::parse(name).unwrap();
        let is_provider = name == "renderer-alias";
        let digests =
            DigestSet::new(vec![ExpectedDigest::Sha256(Sha256::digest(bytes).into())]).unwrap();
        dependencies.insert(
            key.clone(),
            LockedDependency {
                source_selector: source.intent().roots[&key].source.selector().cloned(),
                title: name.into(),
                kind: ContentKind::ResourcePack,
                identity: if is_provider {
                    ResolvedIdentity::Provider(provider.clone())
                } else {
                    ResolvedIdentity::Local(key.clone())
                },
                selected: is_provider.then(|| selected.clone()),
                incompatibilities: vec![],
                files: NonEmpty::new(vec![ResolvedFile {
                    slot: FileSlot::parse("primary").unwrap(),
                    acquisition: if is_provider {
                        AcquisitionSpec::Provider {
                            pin: selected.clone(),
                            slot: FileSlot::parse("original.zip").unwrap(),
                            alternatives: vec!["https://example.invalid/original.zip".into()],
                        }
                    } else {
                        AcquisitionSpec::Local(
                            PortableRelPath::parse("vendor/local.zip", PathSyntax::ProjectContent)
                                .unwrap(),
                        )
                    },
                    expected: ExpectedContent {
                        digests: Some(digests.clone()),
                        size: Some(bytes.len() as u64),
                        accepted_observation: None,
                    },
                    provenance: Provenance {
                        source: "fixed selection fixture".into(),
                        location: None,
                        declared_digests: Some(digests),
                        conversions: vec![],
                    },
                    placements: NonEmpty::new(vec![Placement {
                        destination: InstallDestination::parse(destination).unwrap(),
                        layer: ContentLayer::Common,
                        requirements: Requirements {
                            client: Requirement::Required,
                            server: Requirement::Required,
                        },
                    }])
                    .unwrap(),
                }])
                .unwrap(),
            },
        );
        coverage.insert(key, Coverage::CompleteForSelection);
        write(root, &format!("pack/{destination}"), bytes);
        if !is_provider {
            write(root, "vendor/local.zip", bytes);
        }
    }
    let lock = ResolutionLock {
        acceptable_versions: vec![],
        intent_revision: source.semantic_revision(),
        resolver: "fixed-selection-fixture".into(),
        dependencies,
        required_edges: BTreeMap::new(),
        coverage,
        runtime: RuntimeResolution {
            minecraft: GameVersion::parse("1.21.1").unwrap(),
            loader: LoaderKind::Vanilla,
            loader_version: None,
        },
    };
    let project =
        ResolvedProject::validate(source.intent().clone(), lock, source.semantic_revision())
            .unwrap();
    write(root, "empack.yml", AUTHOR.as_bytes());
    write(root, "empack.bindings.yml", BINDINGS.as_bytes());
    write(
        root,
        "empack.lock",
        &DocumentCodec.encode_lock(&project).unwrap(),
    );
    write(root, "templates/note.txt", b"author template");
    write(root, "dist/prior.zip", b"previous artifact");
    project
}

#[test]
fn journey_cli_pin_unpin_retains_selection_and_refuses_grouped_invalid_edits() {
    let project = TestProject::new();
    let root = project.dir();
    let host = tempfile::tempdir().unwrap();
    let command = || {
        let mut command = offline_command(root);
        command
            .env("EMPACK_STATE_DIR", host.path().join("state"))
            .env("EMPACK_CACHE_DIR", host.path().join("cache"))
            .env("TMPDIR", host.path())
            .env("TMP", host.path())
            .env("TEMP", host.path());
        command
    };
    let original = fixture(root);
    let key = DependencyKey::parse("renderer-alias").unwrap();
    let original_files = files::snapshot(root);
    for action in ["pin", "unpin"] {
        for (invalid, diagnostic) in [
            ("missing", "explicit dependency"),
            ("renderer-alias", "Repeated pin key"),
            ("local", "resolved provider source"),
        ] {
            let before = files::snapshot(root);
            command()
                .args(["--yes", action, "renderer-alias", invalid])
                .assert()
                .failure()
                .stderr(predicates::str::contains(diagnostic));
            assert_eq!(
                files::snapshot(root),
                before,
                "{action} {invalid} published partial changes"
            );
        }
        let before = files::snapshot(root);
        command()
            .args(["--dry-run", action, "renderer-alias"])
            .assert()
            .success();
        assert_eq!(files::snapshot(root), before);
        command()
            .args(["--yes", action, "renderer-alias"])
            .assert()
            .success();
        let current = read_project(root);
        let expected = if action == "pin" {
            DependencySelection::ProviderPin(
                original.lock().dependencies[&key]
                    .selected
                    .as_ref()
                    .unwrap()
                    .selection
                    .clone(),
            )
        } else {
            DependencySelection::Automatic
        };
        assert_eq!(current.intent().roots[&key].selection, expected);
        assert_eq!(current.intent().roots[&key].releases, None);
        assert_eq!(
            current.intent().dependency_policy,
            ReleasePolicy::StableOnly
        );
        let mut expected_lock = original.lock().clone();
        expected_lock.intent_revision = current.lock().intent_revision;
        assert_eq!(
            current.lock(),
            &expected_lock,
            "pin/unpin changed the retained resolution"
        );
        let author = fs::read_to_string(root.join("empack.yml")).unwrap();
        let stripped = author
            .lines()
            .filter(|line| !line.trim_start().starts_with("pin:"))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        assert_eq!(stripped, AUTHOR.replace("    releases: any\n", ""));
        let mut expected_files = original_files.clone();
        expected_files.insert("empack.yml".into(), author.into_bytes());
        expected_files.insert(
            "empack.lock".into(),
            fs::read(root.join("empack.lock")).unwrap(),
        );
        assert_eq!(files::snapshot(root), expected_files);
        let after = files::snapshot(root);
        for _ in 0..2 {
            command().args(["--yes", "sync"]).assert().success();
            assert_eq!(
                files::snapshot(root),
                after,
                "sync changed an exact retained selection"
            );
        }
    }
}
