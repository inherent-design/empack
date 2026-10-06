use super::*;
use crate::engine::{
    content::{InitialObservation, verify_stream},
    documents::DocumentCodec,
    mrpack::tests::project,
    project::ProjectReader,
    publication::RecoveryReader,
    snapshot::SnapshotLimits,
};
use empack_core::{
    model::{ContentLayer, ResolvedProject},
    path::PathSyntax,
};
use std::{fs, io::Read, path::Path};
fn path(value: &str) -> PortableRelPath {
    PortableRelPath::parse(value, PathSyntax::ProjectContent).unwrap()
}
fn write_project(root: &Path, project: &ResolvedProject) {
    fs::write(
        root.join("empack.yml"),
        DocumentCodec.encode_intent(project.intent()).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("empack.lock"),
        DocumentCodec.encode_lock(project).unwrap(),
    )
    .unwrap();
}
fn put(root: &Path, name: &str, bytes: &[u8]) {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}
fn capture(root: &Path, host: &Path) -> WorkspaceSnapshot {
    ProjectReader::new(RecoveryReader::new(host.join("private")))
        .capture_build(
            root,
            &[path("result.zip")],
            SnapshotLimits::default(),
            &Cancellation::default(),
        )
        .unwrap()
}
fn acquired(project: &ResolvedProject, value: &[u8]) -> BuildAcquisitions {
    let mut external = BuildAcquisitions::default();
    for (key, dependency) in &project.lock().dependencies {
        for file in dependency.files.as_slice() {
            let content = verify_stream(
                &mut &*value,
                &ExpectedContent {
                    digests: None,
                    size: None,
                    accepted_observation: None,
                },
                100,
                SourceEvidencePolicy::Compatibility,
                InitialObservation::Accepted,
                &Cancellation::default(),
            )
            .unwrap();
            external.locked.insert(
                LockedFileKey {
                    dependency: key.clone(),
                    slot: file.slot.clone(),
                },
                AcquiredBuildFile {
                    content,
                    permissions: empack_core::files::FilePermissions {
                        readonly: false,
                        executable: false,
                    },
                },
            );
        }
    }
    external
}
fn bytes(file: &AcquiredBuildFile) -> Vec<u8> {
    let mut bytes = vec![];
    file.content.lease().open().read_to_end(&mut bytes).unwrap();
    bytes
}
#[test]
fn materialized_views_select_side_content_and_validate_every_included_file() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let project = project(false, false);
    write_project(root.path(), &project);
    put(root.path(), "pack/config/options", b"common");
    put(root.path(), "overrides/client/config/options", b"client");
    put(root.path(), "overrides/server/config/options", b"server");
    // Unlisted client-only download is not a server obligation, even when missing.
    put(root.path(), "pack/mods/extra.pw.toml", b"filename='extra.jar'\nside='client'\n[download]\nurl='https://example.com/extra.jar'\nhash-format='md5'\nhash='321c3cf486ed509164edec1e1981fec8'\n");
    let workspace = capture(root.path(), host.path());
    let cancel = Cancellation::default();
    let policy = OptionalPolicy::Preserve;
    let server = prepare_game_content(
        &workspace,
        &BuildAcquisitions::default(),
        BuildTarget::ServerFull,
        &policy,
        SourceEvidencePolicy::Compatibility,
        &cancel,
    )
    .unwrap();
    assert_eq!(server.files().len(), 1);
    assert_eq!(bytes(&server.files()[&path("config/options")]), b"server");
    let error = prepare_game_content(
        &workspace,
        &BuildAcquisitions::default(),
        BuildTarget::ClientFull,
        &policy,
        SourceEvidencePolicy::Compatibility,
        &cancel,
    )
    .err()
    .unwrap();
    let missing = error.downcast_ref::<MissingGameContent>().unwrap();
    assert_eq!(missing.files.len(), 2);
    assert_eq!(missing.observed, vec![path("mods/extra.pw.toml")]);
    put(root.path(), "pack/mods/extra.jar", b"payload");
    let workspace = capture(root.path(), host.path());
    let client = prepare_game_content(
        &workspace,
        &acquired(&project, b"payload"),
        BuildTarget::ClientFull,
        &policy,
        SourceEvidencePolicy::Compatibility,
        &cancel,
    )
    .unwrap();
    assert_eq!(client.files().len(), 5);
    assert_eq!(bytes(&client.files()[&path("config/options")]), b"client");
    assert_eq!(
        bytes(&client.files()[&path("resourcepacks/a.zip")]),
        b"payload"
    );
    assert_eq!(client.observed().len(), 1);
    // Identical bytes do not merge independent source assurance by content address.
    assert_eq!(
        client.files()[&path("resourcepacks/a.zip")]
            .content
            .lease()
            .id(),
        client.files()[&path("mods/extra.jar")].content.lease().id()
    );
    assert!(matches!(
        client.files()[&path("resourcepacks/a.zip")]
            .content
            .evidence(),
        empack_core::digest::IntegrityEvidence::ObservedOnly { .. }
    ));
    assert!(matches!(
        client.files()[&path("mods/extra.jar")].content.evidence(),
        empack_core::digest::IntegrityEvidence::MatchedExpected { .. }
    ));
    assert!(
        prepare_game_content(
            &workspace,
            &acquired(&project, b"wrong"),
            BuildTarget::ClientFull,
            &policy,
            SourceEvidencePolicy::Compatibility,
            &cancel
        )
        .is_err()
    );
    workspace
        .root()
        .revalidate(workspace.observations(), &cancel)
        .unwrap();
    assert!(!root.path().join("dist").exists());
    assert!(!host.path().join("private").exists());
}
#[test]
fn disabled_unacquired_optional_overlays_preserve_common_fallback() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let base = project(false, true);
    let mut lock = base.lock().clone();
    for dependency in lock.dependencies.values_mut() {
        let mut files = dependency.files.as_slice().to_vec();
        for file in &mut files {
            let mut placements = file.placements.as_slice().to_vec();
            for placement in &mut placements {
                placement.layer = ContentLayer::Client;
            }
            file.placements = empack_core::model::NonEmpty::new(placements).unwrap();
        }
        dependency.files = empack_core::model::NonEmpty::new(files).unwrap();
    }
    let project =
        ResolvedProject::validate(base.intent().clone(), lock, base.lock().intent_revision)
            .unwrap();
    write_project(root.path(), &project);
    put(root.path(), "pack/resourcepacks/a.zip", b"fallback");
    let workspace = capture(root.path(), host.path());
    let cancel = Cancellation::default();
    let policy = OptionalPolicy::Resolve {
        choices: BTreeMap::new(),
        use_defaults: true,
    };
    let result = prepare_game_content(
        &workspace,
        &BuildAcquisitions::default(),
        BuildTarget::ClientFull,
        &policy,
        SourceEvidencePolicy::Compatibility,
        &cancel,
    )
    .unwrap();
    assert_eq!(result.files().len(), 1);
    assert_eq!(
        bytes(&result.files()[&path("resourcepacks/a.zip")]),
        b"fallback"
    );
    assert!(!result.inventory().choices()[0].enabled);
    let policy = OptionalPolicy::Resolve {
        choices: BTreeMap::from([("extra".into(), true)]),
        use_defaults: false,
    };
    let missing = prepare_game_content(
        &workspace,
        &BuildAcquisitions::default(),
        BuildTarget::ClientFull,
        &policy,
        SourceEvidencePolicy::Compatibility,
        &cancel,
    )
    .err()
    .unwrap();
    assert_eq!(
        missing
            .downcast_ref::<MissingGameContent>()
            .unwrap()
            .files
            .len(),
        2
    );
    let result = prepare_game_content(
        &workspace,
        &acquired(&project, b"payload"),
        BuildTarget::ClientFull,
        &policy,
        SourceEvidencePolicy::Compatibility,
        &cancel,
    )
    .unwrap();
    assert_eq!(
        bytes(&result.files()[&path("resourcepacks/a.zip")]),
        b"payload"
    );
    assert!(result.inventory().choices()[0].enabled);
    assert_eq!(result.inventory().precedence().len(), 1);
}

#[test]
fn captured_bootstrap_projection_retains_references_and_declared_observed_evidence() {
    use crate::engine::packwiz::InstallerInteraction;
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let project = project(true, false);
    write_project(root.path(), &project);
    put(root.path(), "pack/config/value.bin", b"settings");
    put(
        root.path(),
        "pack/mods/extra.pw.toml",
        br#"name = "Extra"
filename = "extra.jar"
side = "client"
[download]
url = "https://example.com/extra.jar"
hash-format = "md5"
hash = "321c3cf486ed509164edec1e1981fec8"
"#,
    );
    let workspace = capture(root.path(), host.path());
    let cancel = Cancellation::default();
    let game = prepare_bootstrap_game_content(
        &workspace,
        &BuildAcquisitions::default(),
        BuildTarget::Client,
        &OptionalPolicy::Preserve,
        SourceEvidencePolicy::Compatibility,
        &cancel,
    )
    .unwrap();
    assert_eq!(game.inventory().entries().len(), 5);
    assert_eq!(game.files().len(), 1);
    assert_eq!(game.observed().len(), 1);
    assert_eq!(game.observed()[0].actual, None);
    assert_eq!(
        game.observed()[0].declared.algorithm(),
        empack_core::digest::DigestAlgorithm::Md5
    );
    let tree = game
        .packwiz(InstallerInteraction::Headless, &cancel)
        .unwrap();
    assert_eq!(tree.files().len(), 7);
    assert_eq!(bytes(&tree.files()[&path("config/value.bin")]), b"settings");
    assert!(!tree.files().contains_key(&path("mods/extra.jar")));
    assert!(
        prepare_game_content(
            &workspace,
            &BuildAcquisitions::default(),
            BuildTarget::ClientFull,
            &OptionalPolicy::Preserve,
            SourceEvidencePolicy::Compatibility,
            &cancel
        )
        .is_err()
    );
    assert!(
        prepare_bootstrap_game_content(
            &workspace,
            &BuildAcquisitions::default(),
            BuildTarget::Client,
            &OptionalPolicy::Preserve,
            SourceEvidencePolicy::StrongSourceRequired,
            &cancel
        )
        .is_err()
    );
    assert!(
        prepare_bootstrap_game_content(
            &workspace,
            &acquired(&project, b"wrong"),
            BuildTarget::Client,
            &OptionalPolicy::Preserve,
            SourceEvidencePolicy::Compatibility,
            &cancel
        )
        .is_err()
    );
}

#[test]
fn native_shared_override_is_preserved_and_selected_before_side_content() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    let project = project(false, false);
    write_project(root.path(), &project);
    put(root.path(), "pack/config/options", b"base");
    put(root.path(), "overrides/common/config/options", b"shared");
    put(root.path(), "overrides/client/config/options", b"client");
    let workspace = capture(root.path(), host.path());
    for (target, expected) in [
        (BuildTarget::ClientFull, b"client"),
        (BuildTarget::ServerFull, b"shared"),
    ] {
        let view = prepare_game_content(
            &workspace,
            &acquired(&project, b"payload"),
            target,
            &OptionalPolicy::Preserve,
            SourceEvidencePolicy::Compatibility,
            &Cancellation::default(),
        )
        .unwrap();
        assert_eq!(bytes(&view.files()[&path("config/options")]), expected);
    }
    assert_eq!(
        fs::read(root.path().join("pack/config/options")).unwrap(),
        b"base"
    );
    assert_eq!(
        fs::read(root.path().join("overrides/common/config/options")).unwrap(),
        b"shared"
    );
}
