//! Native command-host regression; controlled profile saves occur at real approval boundaries.
//! Release codecs construct test inputs; this does not claim launcher or authoring acceptance.
use anyhow::{Result, bail};
use clap::Parser;
use empack_lib::{
    application::{
        AppConfig, Commands,
        cli::Cli,
        commands::execute_command_with_session,
        process_runtime::Cancellation,
        session::{
            ConfigProvider, InteractiveProvider, InvocationProvider, LiveProcessProvider,
            ProcessProvider, Session,
        },
        session_mocks::{MockCommandSession, MockConfigProvider},
    },
    display::DisplayProvider,
    engine::release::*,
    terminal::TerminalCapabilities,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

#[test]
fn runtime_fixture() {
    let root = std::env::current_dir().unwrap();
    if !root.join("prism-repreparation-runtime").exists() {
        return;
    }
    let mut count = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("runtime-invocations"))
        .unwrap();
    writeln!(count, "started").unwrap();
    if root.join("runtime-failure").exists() {
        std::process::exit(7);
    }
}

struct Fixture {
    _home: tempfile::TempDir,
    root: PathBuf,
    state: PathBuf,
    cache: PathBuf,
    release: PathBuf,
    digest: String,
}
impl Fixture {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("instance");
        fs::create_dir(&root).unwrap();
        let assets = home.path().join("assets");
        fs::create_dir(&assets).unwrap();
        let files = [
            (
                "owned",
                "owned.txt",
                b"publisher".as_slice(),
                FilePolicy::Managed,
            ),
            (
                "options",
                "options.txt",
                b"defaults".as_slice(),
                FilePolicy::Seed,
            ),
        ]
        .into_iter()
        .map(|(key, destination, bytes, policy)| {
            fs::write(assets.join(key), bytes).unwrap();
            ReleaseFile {
                key: key.into(),
                destination: destination.into(),
                layer: ReleaseLayer::Common,
                policy,
                client: Participation::Required,
                server: Participation::Unsupported,
                sha256: Sha256::digest(bytes)
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect(),
                bytes: bytes.len() as u64,
                readonly: false,
                executable: false,
                assertions: vec![],
                asset: None,
                source: ReleaseSource::Asset {
                    path: format!("assets/{key}"),
                },
                world_root: None,
            }
        })
        .collect();
        let release = DecodedRelease::encode(ReleaseDocument {
            require_subscription: false,
            server_launch: None,
            schema: RELEASE_SCHEMA,
            pack: "prism-repreparation".into(),
            version: "1".into(),
            minimum_engine: ">=0.8.0-beta".into(),
            runtime: ReleaseRuntime {
                minecraft: "1.21.1".into(),
                loader: ReleaseLoader::Vanilla,
                java: ReleaseJava::for_game("1.21.1", 21).unwrap(),
            },
            choices: vec![],
            files,
        })
        .unwrap();
        let digest = release.id().to_owned();
        let path = home.path().join("release.json");
        fs::write(&path, release.bytes()).unwrap();
        Self {
            state: home.path().join("state"),
            cache: home.path().join("cache"),
            root,
            release: path,
            digest,
            _home: home,
        }
    }
    fn session(&self, yes: bool) -> MockCommandSession {
        MockCommandSession::new().with_config(MockConfigProvider::new(AppConfig {
            workdir: Some(self.root.clone()),
            state_dir: Some(self.state.clone()),
            cache_dir: Some(self.cache.clone()),
            yes,
            ..Default::default()
        }))
    }
    fn command(&self, action: &str) -> Commands {
        let mut args = vec!["empack".into(), "instance".into(), action.into()];
        if action == "launch" {
            args.extend([
                "--".into(),
                std::env::current_exe().unwrap().into_os_string(),
                "--exact".into(),
                "runtime_fixture".into(),
                "--nocapture".into(),
            ]);
        } else {
            args.extend([
                self.release.clone().into_os_string(),
                "--sha256".into(),
                self.digest.clone().into(),
                "--layout".into(),
                "prism".into(),
            ]);
        }
        Cli::try_parse_from(args).unwrap().command.unwrap()
    }
    async fn install(&self) {
        execute_command_with_session(self.command("install"), &self.session(true))
            .await
            .unwrap();
        let game = self.root.join(".minecraft");
        fs::write(game.join("prism-repreparation-runtime"), b"fixture").unwrap();
        fs::write(game.join("options.txt"), b"player options").unwrap();
    }
}

struct SaveAtApproval {
    root: PathBuf,
    case: &'static str,
    calls: AtomicUsize,
    cancellation: Cancellation,
}
impl InteractiveProvider for SaveAtApproval {
    fn can_choose(&self) -> bool {
        true
    }
    fn confirm(&self, _: &str, _: bool) -> Result<bool> {
        let attempt = self.calls.fetch_add(1, Ordering::SeqCst);
        if attempt > 0 && self.case != "repeated" {
            return Ok(true);
        }
        let path = self.root.join("mmc-pack.json");
        let mut profile: Value = serde_json::from_slice(&fs::read(&path)?)?;
        match self.case {
            "cache" => profile["components"][0]["cachedName"] = json!("Launcher label"),
            "same" => {}
            "game" => profile["components"][0]["version"] = json!("1.21.2"),
            "loader" => profile["components"]
                .as_array_mut()
                .unwrap()
                .push(json!({"uid":"net.fabricmc.fabric-loader","version":"0.16.14"})),
            "malformed" => {
                fs::write(path, b"{")?;
                return Ok(true);
            }
            "selection" => {
                let record = self.root.join(".empack/instance.json");
                let mut value: Value = serde_json::from_slice(&fs::read(&record)?)?;
                value["pack"] = json!("other-selection");
                fs::write(record, serde_json::to_vec(&value)?)?;
            }
            "trust" => {
                fs::write(self.root.join(".empack/subscription.json"), b"{}")?;
            }
            "managed" => {
                fs::write(self.root.join(".minecraft/owned.txt"), b"external change")?;
            }
            "runtime-failure" => {
                fs::write(self.root.join(".minecraft/runtime-failure"), b"exit 7")?;
            }
            "resolved" | "cancel" | "repeated" => {
                if attempt == 0 {
                    profile["components"].as_array_mut().unwrap().insert(
                        0,
                        json!({"uid":"org.lwjgl3","version":"3.3.3","dependencyOnly":true}),
                    );
                } else {
                    profile["components"][0]["version"] = json!("3.3.4");
                }
                if self.case == "cancel" {
                    self.cancellation.cancel();
                }
            }
            other => bail!("Unexpected fixture {other}"),
        }
        let temporary = path.with_extension("save");
        fs::write(&temporary, serde_json::to_vec_pretty(&profile)?)?;
        fs::rename(temporary, path)?;
        Ok(true)
    }
    fn text_input(&self, _: &str, _: String) -> Result<String> {
        bail!("Unexpected text prompt")
    }
    fn select(&self, _: &str, _: &[&str]) -> Result<usize> {
        bail!("Unexpected choice")
    }
    fn fuzzy_select(&self, _: &str, _: &[String]) -> Result<Option<usize>> {
        bail!("Unexpected choice")
    }
}
struct Driver {
    session: MockCommandSession,
    prompt: SaveAtApproval,
}
impl Session for Driver {
    fn display(&self) -> &dyn DisplayProvider {
        self.session.display()
    }
    fn invocation(&self) -> &dyn InvocationProvider {
        self.session.invocation()
    }
    fn process(&self) -> &dyn ProcessProvider {
        self.session.process()
    }
    fn config(&self) -> &dyn ConfigProvider {
        self.session.config()
    }
    fn terminal(&self) -> &TerminalCapabilities {
        self.session.terminal()
    }
    fn interactive(&self) -> &dyn InteractiveProvider {
        &self.prompt
    }
}

async fn matrix(action: &str) {
    for case in [
        "resolved",
        "repeated",
        "cache",
        "same",
        "game",
        "loader",
        "malformed",
        "selection",
        "trust",
        "managed",
        "cancel",
        "runtime-failure",
    ] {
        if action != "launch" && case == "runtime-failure" {
            continue;
        }
        let fixture = Fixture::new();
        fixture.install().await;
        let original = fs::read(fixture.root.join(".empack/instance.json")).unwrap();
        let cancellation = Cancellation::default();
        let mut session = fixture.session(false);
        session.process_provider =
            LiveProcessProvider::new().with_cancellation(cancellation.clone());
        let driver = Driver {
            session,
            prompt: SaveAtApproval {
                root: fixture.root.clone(),
                case,
                calls: AtomicUsize::new(0),
                cancellation,
            },
        };
        let result = execute_command_with_session(fixture.command(action), &driver).await;
        let success = matches!(case, "resolved" | "cache" | "same");
        assert_eq!(result.is_ok(), success, "{action}/{case}: {result:?}");
        let calls = driver.prompt.calls.load(Ordering::SeqCst);
        assert_eq!(
            calls,
            if matches!(case, "resolved" | "repeated") {
                2
            } else {
                1
            },
            "{action}/{case}"
        );
        let game = fixture.root.join(".minecraft");
        let starts = fs::read_to_string(game.join("runtime-invocations"))
            .unwrap_or_default()
            .lines()
            .count();
        assert_eq!(
            starts,
            usize::from(action == "launch" && (success || case == "runtime-failure")),
            "{action}/{case}"
        );
        assert_eq!(
            fs::read(game.join("options.txt")).unwrap(),
            b"player options"
        );
        if case != "selection" {
            assert_eq!(
                fs::read(fixture.root.join(".empack/instance.json")).unwrap(),
                original
            );
        }
        if case == "repeated" {
            let error = format!("{:#}", result.unwrap_err());
            assert!(
                error.starts_with("Launch settings kept changing."),
                "{error}"
            );
        } else if case == "cancel" {
            assert!(
                result
                    .unwrap_err()
                    .is::<empack_lib::application::process_runtime::Interrupted>()
            );
        } else if case == "runtime-failure" {
            assert!(format!("{:#}", result.unwrap_err()).contains("exit status"));
        }
        // A subsequent ordinary launch verifies that a rejected/retired attempt left no false runtime lease.
        if matches!(case, "resolved" | "cache" | "same" | "repeated" | "cancel") {
            execute_command_with_session(fixture.command("launch"), &fixture.session(true))
                .await
                .unwrap();
            let final_starts = fs::read_to_string(game.join("runtime-invocations"))
                .unwrap()
                .lines()
                .count();
            assert_eq!(final_starts, starts + 1, "{action}/{case}");
        }
    }
}

#[tokio::test]
async fn prism_launch_reprepares_once_without_replaying_runtime_or_changing_selection() {
    matrix("launch").await;
}

#[tokio::test]
async fn prism_ensure_reprepares_once_without_replaying_publication() {
    matrix("ensure").await;
}

#[tokio::test]
async fn cli_prism_first_save_reprepares_and_starts_runtime_once() {
    use empack_tests::harness::{process::empack_cmd, terminal};
    use expectrl::{Expect, Regex};
    use std::time::Duration;
    let fixture = Fixture::new();
    fixture.install().await;
    let mut command = empack_cmd(&fixture.root);
    command
        .arg("--state-dir")
        .arg(&fixture.state)
        .arg("--cache-dir")
        .arg(&fixture.cache)
        .args(["instance", "launch", "--"])
        .arg(std::env::current_exe().unwrap())
        .args(["--exact", "runtime_fixture", "--nocapture"]);
    let mut session =
        expectrl::session::log(terminal::spawn_sized(command, 100, 30), std::io::stderr()).unwrap();
    session.set_expect_timeout(Some(Duration::from_secs(20)));
    session
        .expect(Regex("Apply this instance launch plan"))
        .unwrap();
    let save = SaveAtApproval {
        root: fixture.root.clone(),
        case: "resolved",
        calls: AtomicUsize::new(0),
        cancellation: Cancellation::default(),
    };
    save.confirm("controlled save after preparation", false)
        .unwrap();
    // Confirmation consumes one key; a newline would decline the next prompt.
    session.send("y").unwrap();
    let recovery = session
        .expect(Regex("Launch settings changed; preparing again[.]"))
        .unwrap();
    assert!(!String::from_utf8_lossy(recovery.before()).contains("Error:"));
    session
        .expect(Regex("Apply this instance launch plan"))
        .unwrap();
    session.send("y").unwrap();
    session
        .expect(Regex("Runtime completed for release"))
        .unwrap();
    #[cfg(not(windows))]
    session.expect(expectrl::Eof).unwrap();
    #[cfg(windows)]
    assert_eq!(session.get_process().wait(Some(20_000)).unwrap(), 0);
    assert_eq!(
        fs::read_to_string(fixture.root.join(".minecraft/runtime-invocations"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    assert_eq!(
        fs::read(fixture.root.join(".minecraft/options.txt")).unwrap(),
        b"player options"
    );
}
