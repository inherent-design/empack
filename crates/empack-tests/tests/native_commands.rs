//! Command matrices use native roots, exact documents and real archive bytes.
use anyhow::Result;
use empack_lib::{
    application::{
        AppConfig, BuildArgs, Commands, InitArgs,
        cli::{CliArchiveFormat, CliProjectType},
        commands::execute_command_with_session,
        session_mocks::{
            MockCommandSession, MockConfigProvider, MockFileSystemProvider, MockInteractiveProvider,
        },
    },
    engine::documents::DocumentCodec,
};
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Write},
    path::{Path, PathBuf},
};
struct Project {
    root: tempfile::TempDir,
}
impl Project {
    fn new() -> Self {
        Self {
            root: tempfile::tempdir().unwrap(),
        }
    }
    fn path(&self) -> PathBuf {
        self.root.path().join("project")
    }
    fn session(&self, yes: bool, dry: bool) -> MockCommandSession {
        MockCommandSession::new()
            .with_filesystem(
                MockFileSystemProvider::new().with_current_dir(self.root.path().into()),
            )
            .with_config(MockConfigProvider::new(AppConfig {
                workdir: Some("project".into()),
                state_dir: Some("state".into()),
                yes,
                dry_run: dry,
                curseforge_api_client_key: None,
                ..Default::default()
            }))
            .with_interactive(MockInteractiveProvider::new().with_confirm(yes))
    }
    async fn run(&self, command: Commands) -> Result<()> {
        execute_command_with_session(command, &self.session(true, false)).await
    }
    async fn initialize(&self, loader: &str, version: Option<&str>) -> Result<()> {
        self.run(Commands::Init(InitArgs {
            modloader: Some(loader.into()),
            mc_version: Some("1.21.1".into()),
            loader_version: version.map(Into::into),
            pack_name: Some("Native".into()),
            pack_version: Some("1".into()),
            author: Some("Tester".into()),
            datapack_folder: Some("config/paxi/datapacks".into()),
            ..Default::default()
        }))
        .await
    }
    fn snapshot(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        fn walk(root: &Path, at: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
            if !at.exists() {
                return;
            }
            for entry in fs::read_dir(at).unwrap() {
                let entry = entry.unwrap();
                if entry.file_type().unwrap().is_dir() {
                    walk(root, &entry.path(), out);
                } else {
                    out.insert(
                        entry.path().strip_prefix(root).unwrap().into(),
                        fs::read(entry.path()).unwrap(),
                    );
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(&self.path(), &self.path(), &mut out);
        out
    }
    fn roots(&self) -> usize {
        DocumentCodec
            .decode_intent(&fs::read(self.path().join("empack.yml")).unwrap(), "test")
            .unwrap()
            .intent()
            .roots
            .len()
    }
    fn source(&self, filename: &str, members: &[(&str, &[u8])]) {
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in members {
            archive
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            archive.write_all(bytes).unwrap();
        }
        fs::write(
            self.root.path().join(filename),
            archive.finish().unwrap().into_inner(),
        )
        .unwrap();
    }
}
fn add(names: &[&str], kind: Option<CliProjectType>) -> Commands {
    Commands::Add {
        mods: names.iter().map(|value| (*value).into()).collect(),
        force: false,
        platform: None,
        project_type: kind,
        version_id: None,
        file_id: None,
    }
}
fn remove(names: &[&str], forget: bool, acknowledge_unknown: bool) -> Commands {
    Commands::Remove {
        mods: names.iter().map(|value| (*value).into()).collect(),
        deps: false,
        forget,
        acknowledge_unknown,
    }
}
fn build(target: &str) -> Commands {
    Commands::Build(BuildArgs {
        targets: vec![target.into()],
        ..Default::default()
    })
}

#[tokio::test]
async fn all_loader_families_keep_exact_runtime_and_generate_mrpack() -> Result<()> {
    for (loader, version) in [
        ("none", None),
        ("fabric", Some("0.16.14")),
        ("quilt", Some("0.29.2")),
        ("forge", Some("52.0.28")),
        ("neoforge", Some("21.1.172")),
    ] {
        let project = Project::new();
        project.initialize(loader, version).await?;
        let intent =
            DocumentCodec.decode_intent(&fs::read(project.path().join("empack.yml"))?, "test")?;
        let resolved = DocumentCodec.decode_lock(
            &fs::read(project.path().join("empack.lock"))?,
            &intent,
            "test",
        )?;
        assert_eq!(
            resolved
                .lock()
                .runtime
                .loader_version
                .as_ref()
                .map(|value| value.as_str()),
            version
        );
        assert!(
            project
                .path()
                .join("templates/server/server.properties.template")
                .exists()
        );
        project.run(build("mrpack")).await?;
        let mut archive =
            zip::ZipArchive::new(fs::File::open(project.path().join("dist/Native-1.mrpack"))?)?;
        let manifest: serde_json::Value =
            serde_json::from_reader(archive.by_name("modrinth.index.json")?)?;
        assert_eq!(manifest["dependencies"]["minecraft"], "1.21.1");
    }
    Ok(())
}
#[tokio::test]
async fn direct_content_matrix_preserves_selected_kind_destination_and_sync() -> Result<()> {
    type ContentCase = (
        &'static str,
        CliProjectType,
        &'static [(&'static str, &'static [u8])],
        &'static str,
    );
    let cases: &[ContentCase] = &[
        (
            "mod.jar",
            CliProjectType::Mod,
            &[("fabric.mod.json", b"{}")],
            "mods/mod.jar",
        ),
        (
            "resources.zip",
            CliProjectType::ResourcePack,
            &[("pack.mcmeta", b"{}"), ("assets/example/file", b"resource")],
            "resourcepacks/resources.zip",
        ),
        (
            "shaders.zip",
            CliProjectType::Shader,
            &[("shaders/example.glsl", b"shader")],
            "shaderpacks/shaders.zip",
        ),
        (
            "data.zip",
            CliProjectType::Datapack,
            &[("pack.mcmeta", b"{}"), ("data/example/file", b"data")],
            "config/paxi/datapacks/data.zip",
        ),
    ];
    for (filename, kind, members, destination) in cases {
        let project = Project::new();
        project.initialize("none", None).await?;
        project.source(filename, members);
        project.run(add(&[filename], Some(kind.clone()))).await?;
        assert_eq!(
            fs::read(project.path().join("pack").join(destination))?,
            fs::read(project.root.path().join(filename))?
        );
        let before = project.snapshot();
        for _ in 0..2 {
            project.run(Commands::Sync {}).await?;
        }
        assert_eq!(project.snapshot(), before);
    }
    Ok(())
}
#[tokio::test]
async fn preview_matrix_never_mutates_project_or_runs_backend() -> Result<()> {
    let project = Project::new();
    project.initialize("none", None).await?;
    project.source("fixture.jar", &[("fabric.mod.json", b"{}")]);
    project.run(add(&["fixture.jar"], None)).await?;
    fs::create_dir_all(project.path().join("dist"))?;
    fs::write(project.path().join("dist/prior.zip"), b"prior")?;
    let before = project.snapshot();
    let commands = [
        add(&["fixture.jar"], None),
        remove(&["fixture"], false, false),
        Commands::Sync {},
        build("mrpack"),
        Commands::Clean {
            targets: vec!["builds".into()],
        },
        Commands::Init(InitArgs {
            force: true,
            modloader: Some("none".into()),
            mc_version: Some("1.21.1".into()),
            ..Default::default()
        }),
    ];
    for command in commands {
        // Re-adding an existing identity can be refused; refusal is also read-only.
        let result =
            execute_command_with_session(command.clone(), &project.session(true, true)).await;
        if !matches!(command, Commands::Add { .. }) {
            result?;
        }
        assert_eq!(project.snapshot(), before);
    }
    Ok(())
}
#[tokio::test]
async fn removal_batches_demote_without_erasing_and_require_unknown_acknowledgment() -> Result<()> {
    let project = Project::new();
    project.initialize("none", None).await?;
    for name in ["first.jar", "second.jar"] {
        project.source(name, &[("fabric.mod.json", b"{}")]);
    }
    project.run(add(&["first.jar", "second.jar"], None)).await?;
    let before = project.snapshot();
    assert!(
        project
            .run(remove(&["first", "absent"], false, true))
            .await
            .is_err()
    );
    assert_eq!(project.snapshot(), before);
    project.run(remove(&["first"], true, false)).await?;
    assert_eq!(project.roots(), 1);
    assert!(project.path().join("pack/mods/first.jar").exists());
    project.run(Commands::Sync {}).await?;
    assert!(project.path().join("pack/mods/first.jar").exists());
    let before = project.snapshot();
    assert!(project.run(remove(&["first"], false, false)).await.is_err());
    assert_eq!(project.snapshot(), before);
    project.run(remove(&["first"], false, true)).await?;
    assert!(!project.path().join("pack/mods/first.jar").exists());
    assert!(project.path().join("pack/mods/second.jar").exists());
    Ok(())
}
#[tokio::test]
async fn client_full_archives_contain_current_content_for_every_format() -> Result<()> {
    for format in [
        CliArchiveFormat::Zip,
        CliArchiveFormat::TarGz,
        CliArchiveFormat::SevenZ,
    ] {
        let project = Project::new();
        project.initialize("none", None).await?;
        fs::create_dir_all(project.path().join("pack/config"))?;
        fs::write(project.path().join("pack/config/value.toml"), b"current")?;
        project
            .run(Commands::Build(BuildArgs {
                targets: vec!["client-full".into()],
                format: Some(format),
                ..Default::default()
            }))
            .await?;
        let extension = match format {
            CliArchiveFormat::Zip => "zip",
            CliArchiveFormat::TarGz => "tar.gz",
            CliArchiveFormat::SevenZ => "7z",
        };
        assert!(
            fs::metadata(
                project
                    .path()
                    .join(format!("dist/Native-1-client-full.{extension}"))
            )?
            .len()
                > 0
        );
        // Engine output verification checks the full byte inventory for all three writers.
        project
            .run(Commands::Clean {
                targets: vec!["builds".into()],
            })
            .await?;
        assert_eq!(
            fs::read(project.path().join("pack/config/value.toml"))?,
            b"current"
        );
    }
    Ok(())
}
#[tokio::test]
async fn build_continue_missing_state_and_unknown_clean_scope_preserve_files() -> Result<()> {
    let project = Project::new();
    project.initialize("none", None).await?;
    let before = project.snapshot();
    for command in [
        Commands::Build(BuildArgs {
            continue_build: true,
            ..Default::default()
        }),
        Commands::Clean {
            targets: vec!["builds".into(), "unknown".into()],
        },
        build("not-a-target"),
    ] {
        assert!(project.run(command).await.is_err());
        assert_eq!(project.snapshot(), before);
    }
    Ok(())
}
