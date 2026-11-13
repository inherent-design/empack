//! Host-selected runtimes and local Java invocation through the ordinary CLI, not JVM startup.
use empack_lib::EmpackExitCode;
use empack_tests::harness::{
    process::{TestProject, empack_bin, file_sha256, offline_command, offline_empack_cmd},
    publication::Driver,
    workspace::{Snapshot, snapshot},
};
use predicates::str::contains;
use serde_json::{Value, json};
use std::{ffi::OsStr, fs, path::PathBuf, process::Command, time::Duration};

const EXIT_SEVEN: &str = if cfg!(windows) {
    "Instance runtime exited with exit code: 7"
} else {
    "Instance runtime exited with exit status: 7"
};

struct Server {
    instance: TestProject,
    fixtures: tempfile::TempDir,
    program: PathBuf,
    record: Vec<u8>,
    game: Snapshot,
    input: Snapshot,
}

impl Server {
    fn new() -> Self {
        Self::with_java_directory("server Java Ω ")
    }

    fn with_java_directory(prefix: &str) -> Self {
        let instance = TestProject::new();
        let fixtures = tempfile::Builder::new().prefix(prefix).tempdir().unwrap();
        fs::create_dir(fixtures.path().join("jvm-temp")).unwrap();
        let export = fixtures.path().join("export");
        fs::create_dir_all(export.join("assets")).unwrap();
        // Literal inventory and arbitrary bytes exercise ownership/invocation only.
        let jar = b"not a runnable JAR: invocation fixture\n";
        fs::write(export.join("assets/server.jar"), jar).unwrap();
        let release = serde_json::to_vec(&json!({
            "schema":2,"pack":"verification.java","version":"1","minimum_engine":">=0.8.0-beta",
            "require_subscription":false,
            "runtime":{"minecraft":"1.21.1","loader":{"kind":"vanilla"},"java":{"preferred":21,"minimum":21,"supported":[21],"policy":"empack-java-1"}},
            "server_launch":{"kind":"jar","path":"server.jar"},"choices":[],
            "files":[{"key":"server","destination":"server.jar","layer":"common","policy":"managed",
                "client":{"kind":"unsupported"},"server":{"kind":"required"},
                "sha256":file_sha256(&export.join("assets/server.jar")).unwrap(),"bytes":jar.len(),
                "readonly":false,"executable":false,"assertions":[],
                "source":{"kind":"asset","path":"assets/server.jar"}}]
        }))
        .unwrap();
        fs::write(export.join("release.json"), &release).unwrap();
        offline_command(instance.dir())
            .args(["--yes", "instance", "install"])
            .arg(export.join("release.json"))
            .args([
                "--sha256",
                &file_sha256(&export.join("release.json")).unwrap(),
                "--side",
                "server",
            ])
            .assert()
            .success();
        fs::write(instance.dir().join("game/player.txt"), b"player content\n").unwrap();
        fs::write(instance.dir().join("game/eula.txt"), b"eula=false\n").unwrap();
        let tools = fixtures.path().join("selected tools/bin");
        fs::create_dir_all(&tools).unwrap();
        let program = tools.join(if cfg!(windows) { "java.exe" } else { "java" });
        // Cargo supplies this compiled executable; no target-directory/PATH guessing.
        let compiled = PathBuf::from(env!("CARGO_BIN_EXE_java-fixture"));
        assert!(compiled.is_absolute() && compiled.is_file());
        // Retain one opened input's bytes: another test graph may replace Cargo's
        // shared output path while these independent test processes run.
        let bytes = fs::read(&compiled).unwrap();
        fs::write(&program, &bytes).unwrap();
        fs::set_permissions(&program, fs::metadata(&compiled).unwrap().permissions()).unwrap();
        assert_eq!(fs::read(&program).unwrap(), bytes);
        eprintln!(
            "Cargo-built Java invocation fixture SHA256={}",
            file_sha256(&program).unwrap()
        );
        fs::create_dir(fixtures.path().join("observations")).unwrap();
        let record = fs::read(instance.dir().join(".empack/instance.json")).unwrap();
        let game = snapshot(&instance.dir().join("game"));
        let input = snapshot(&export);
        let fixture = Self {
            instance,
            fixtures,
            program,
            record,
            game,
            input,
        };
        fixture.configure("", "java.specification.version = 21\n", 0, 0);
        fixture
    }

    fn configure(&self, stdout: &str, stderr: &str, probe_status: i32, runtime_status: i32) {
        fs::write(self.program.parent().unwrap().join("java-fixture.json"), serde_json::to_vec(&json!({
            "stdout":stdout,"stderr":stderr,"probe_status":probe_status,"runtime_status":runtime_status,
            "observations":self.fixtures.path().join("observations"),
            "bootstrap_java":self.instance.dir().join(".empack-tools/empack-bootstrap.jar").is_file().then(empack_tests::harness::process::java_bin),
            // The real JAR consumes an ordinary distribution-relative path,
            // not Rust's Windows verbatim spelling from qualification capture.
            "engine":PathBuf::from(".empack-tools").join(if cfg!(windows) { "empack.exe" } else { "empack" }),
            "development_engine":empack_tests::harness::process::expected_cli_version() == "0.0.0-dev",
        })).unwrap()).unwrap();
        for name in ["probe.json", "runtime.json"] {
            let path = self.fixtures.path().join("observations").join(name);
            if path.exists() {
                fs::remove_file(path).unwrap();
            }
        }
    }

    fn launch(&self, options: &[&str], program: &OsStr) -> assert_cmd::Command {
        let mut command = offline_command(self.instance.dir());
        command
            .args(options)
            .args(["instance", "launch", "--server", "--"])
            .arg(program)
            .args(["nogui", "literal argument", "é"])
            .env("PATH", self.program.parent().unwrap())
            .env(
                "JAVA_HOME",
                self.fixtures.path().join("unselected Java home"),
            );
        command
    }

    fn invocation(&self, name: &str, arguments: &[&str]) {
        let observed: Value = serde_json::from_slice(
            &fs::read(self.fixtures.path().join("observations").join(name)).unwrap(),
        )
        .unwrap();
        assert_eq!(
            PathBuf::from(observed["program"].as_str().unwrap())
                .canonicalize()
                .unwrap(),
            self.program.canonicalize().unwrap()
        );
        assert_eq!(
            PathBuf::from(observed["cwd"].as_str().unwrap())
                .canonicalize()
                .unwrap(),
            self.instance.dir().join("game").canonicalize().unwrap()
        );
        assert_eq!(observed["arguments"], json!(arguments));
    }

    fn not_started(&self) {
        assert!(
            !self
                .fixtures
                .path()
                .join("observations/runtime.json")
                .exists()
        );
    }

    fn preserved(&self) {
        assert_eq!(snapshot(&self.instance.dir().join("game")), self.game);
        assert_eq!(snapshot(&self.fixtures.path().join("export")), self.input);
        assert_eq!(
            fs::read(self.instance.dir().join(".empack/instance.json")).unwrap(),
            self.record
        );
    }

    fn successful(&self, program: &OsStr) {
        let tools = snapshot(self.program.parent().unwrap());
        self.launch(&["--yes"], program)
            .assert()
            .success()
            .stdout(contains("Runtime completed for release"));
        for (name, arguments) in [
            ("probe.json", vec!["-XshowSettings:properties", "-version"]),
            (
                "runtime.json",
                vec!["-jar", "server.jar", "nogui", "literal argument", "é"],
            ),
        ] {
            self.invocation(name, &arguments);
        }
        assert_eq!(snapshot(self.program.parent().unwrap()), tools);
        self.preserved();
    }

    fn generate_scripts(&self, driver: &Driver, managed: bool) {
        let root = self.instance.dir();
        let export = self.fixtures.path().join("export");
        let bootstrap = root.join(".empack-consumer");
        if managed {
            let tools = root.join(".empack-tools");
            fs::create_dir_all(&tools).unwrap();
            fs::copy(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../bootstrap/target/empack-bootstrap.jar"),
                tools.join("empack-bootstrap.jar"),
            )
            .unwrap();
            // Provision the exact admitted product through the same local-tool
            // location used by unpublished-candidate consumer checks. Keep the
            // descriptor and full-instance preservation assertions unchanged.
            let selected = empack_bin();
            let local = tools.join(if cfg!(windows) {
                "empack.exe"
            } else {
                "empack"
            });
            let digest = file_sha256(&selected).unwrap();
            fs::copy(&selected, &local).unwrap();
            fs::set_permissions(&local, fs::metadata(&selected).unwrap().permissions()).unwrap();
            assert_eq!(file_sha256(&local).unwrap(), digest);
        }
        fs::create_dir_all(bootstrap.join("assets")).unwrap();
        for name in ["release.json", "assets/server.jar"] {
            fs::copy(export.join(name), bootstrap.join(name)).unwrap();
        }
        let marker = root.join("server-script-fixture.json");
        fs::write(
            &marker,
            serde_json::to_vec(&json!({
                "managed":managed,
                "release_sha256":file_sha256(&export.join("release.json")).unwrap(),
            }))
            .unwrap(),
        )
        .unwrap();
        driver
            .command(root)
            .args([
                "--exact",
                "engine::build::server::tests::generated_server_entrypoint_fixture",
                "--nocapture",
            ])
            .assert()
            .success();
        fs::remove_file(marker).unwrap();
    }

    fn script(&self, gui: bool, home: bool) -> assert_cmd::Command {
        let root = self.instance.dir();
        let mut command = if cfg!(windows) {
            let mut command =
                Command::new(std::env::var_os("COMSPEC").unwrap_or_else(|| "cmd.exe".into()));
            command.args(["/D", "/C", if gui { "start_gui.bat" } else { "start.bat" }]);
            command
        } else {
            let mut command = Command::new("bash");
            command.arg(root.join(if gui { "start_gui.sh" } else { "start.sh" }));
            command
        };
        command
            .env_clear()
            .current_dir(root)
            .args(["literal argument", "é"]);
        for (key, value) in offline_empack_cmd(root).get_envs() {
            if let Some(value) = value {
                command.env(key, value);
            }
        }
        // Actual JVM scratch (including Windows hsperfdata) is host-owned,
        // outside the instance whose complete preservation this case asserts.
        empack_tests::harness::process::command_temp_dir(
            &mut command,
            &self.fixtures.path().join("jvm-temp"),
        );
        let binary = empack_bin();
        let paths = std::env::join_paths(
            [
                self.program.parent().unwrap().to_owned(),
                binary.parent().unwrap().to_owned(),
            ]
            .into_iter()
            .chain(std::env::split_paths(
                &std::env::var_os("PATH").unwrap_or_default(),
            )),
        )
        .unwrap();
        command.env("PATH", paths).env("EMPACK_NO_PAUSE", "1");
        // This fixture requires Unicode *external JVM* arguments. Keep the
        // harness's ordinary C-locale CLI coverage separate from that contract.
        #[cfg(target_os = "linux")]
        command.env("LANG", "C.UTF-8").env("LC_ALL", "C.UTF-8");
        #[cfg(target_os = "macos")]
        command
            .env("LANG", "en_US.UTF-8")
            .env("LC_ALL", "en_US.UTF-8");
        if home {
            command.env(
                "JAVA_HOME",
                self.program.parent().unwrap().parent().unwrap(),
            );
        } else {
            command.env_remove("JAVA_HOME");
        }
        let mut command = assert_cmd::Command::from_std(command);
        command.timeout(Duration::from_secs(30));
        command
    }
}

#[test]
#[cfg(unix)]
fn cli_runtime_preserves_native_program_and_non_utf8_arguments() {
    use std::{
        ffi::OsString,
        os::unix::ffi::{OsStrExt, OsStringExt},
    };

    let server = Server::new();
    let invalid_name = OsString::from_vec(b"selected runtime \xff".to_vec());
    // The macOS fixture volume rejects invalid filename bytes with EILSEQ.
    // Native argv still admits those bytes; Linux also exercises the raw path.
    let name = if cfg!(target_os = "linux") {
        invalid_name.clone()
    } else {
        OsString::from("selected runtime Ω")
    };
    let program = server.program.parent().unwrap().join(&name);
    fs::copy(&server.program, &program).unwrap();
    fs::set_permissions(
        &program,
        fs::metadata(&server.program).unwrap().permissions(),
    )
    .unwrap();
    let digest = file_sha256(&program).unwrap();
    assert_eq!(digest, file_sha256(&server.program).unwrap());
    let tools = snapshot(server.program.parent().unwrap());
    let instance = snapshot(server.instance.dir());

    // Bare-name PATH lookup deliberately requires UTF-8; an absolute native
    // path is the supported route. Refusal must not run either fixture mode.
    offline_command(server.instance.dir())
        .args(["--yes", "instance", "launch", "--"])
        .arg(&invalid_name)
        .env("PATH", server.program.parent().unwrap())
        .assert()
        .failure()
        .stderr(contains(
            "Runtime program name is not UTF-8; use an absolute path",
        ));
    assert_eq!(snapshot(server.instance.dir()), instance);
    assert_eq!(snapshot(server.program.parent().unwrap()), tools);
    assert!(
        !server
            .fixtures
            .path()
            .join("observations/probe.json")
            .exists()
    );
    server.not_started();

    let arguments = [
        OsString::from_vec(vec![0xff, b'a', 0xfe]),
        OsString::new(),
        OsString::from("space and Ω"),
        OsString::from("--looks-like-an-option"),
        OsString::from("line one\nline two"),
        OsString::from("$(touch must-not-run); `touch must-not-run`"),
        OsString::from("literal'quote\"value\\"),
    ];
    offline_command(server.instance.dir())
        .args(["--yes", "instance", "launch", "--"])
        .arg(&program)
        .args(&arguments)
        .assert()
        .success()
        .stdout(contains("Runtime completed for release"));
    let observed: Value = serde_json::from_slice(
        &fs::read(server.fixtures.path().join("observations/runtime.json")).unwrap(),
    )
    .unwrap();
    let native_path = |name: &str| {
        std::path::PathBuf::from(OsString::from_vec(
            serde_json::from_value::<Vec<u8>>(observed["native"][name].clone()).unwrap(),
        ))
    };
    let observed_program = native_path("program");
    // current_exe may retain /var or report /private/var on macOS. Verify the
    // selected file independently while preserving the native filename bytes.
    assert_eq!(
        observed_program.canonicalize().unwrap(),
        program.canonicalize().unwrap()
    );
    assert_eq!(observed_program.file_name(), program.file_name());
    assert_eq!(observed["program"], json!(observed_program.to_str()));
    assert_eq!(
        native_path("cwd").canonicalize().unwrap(),
        server.instance.dir().join("game").canonicalize().unwrap()
    );
    assert!(observed["arguments"].is_null());
    assert_eq!(observed["native"]["encoding"], "unix-bytes");
    assert_eq!(
        observed["native"]["arguments"],
        json!(
            arguments
                .iter()
                .map(|value| value.as_bytes())
                .collect::<Vec<_>>()
        )
    );
    assert!(
        !server
            .fixtures
            .path()
            .join("observations/probe.json")
            .exists()
    );
    assert_eq!(snapshot(server.program.parent().unwrap()), tools);
    assert_eq!(file_sha256(&program).unwrap(), digest);
    server.preserved();
    // A second public launch establishes ordinary ownership retirement too.
    server.successful(server.program.as_os_str());
}

#[test]
fn cli_server_java_selection_requires_approval_and_preserves_installation() {
    let server = Server::new();
    let before = snapshot(server.instance.dir());
    let tools = snapshot(server.fixtures.path());
    server
        .launch(&["--dry-run", "--yes"], server.program.as_os_str())
        .assert()
        .success()
        .stdout(contains("Dry run complete - no changes applied"));
    server
        .launch(&[], server.program.as_os_str())
        .assert()
        .failure()
        .stderr(contains("Noninteractive execution requires --yes"));
    server
        .launch(&["--yes"], OsStr::new("java"))
        .env("PATH", "")
        .assert()
        .failure()
        .stderr(contains("Runtime executable was not found on PATH"));
    assert_eq!(snapshot(server.instance.dir()), before);
    assert_eq!(snapshot(server.fixtures.path()), tools);
    server.not_started();
    for program in [server.program.as_os_str(), OsStr::new("java")] {
        server.configure("", "java.specification.version = 21\n", 0, 0);
        server.successful(program);
    }
    // JAVA_HOME is a generated-script choice; the CLI honors its explicit local program.
    server.configure("java.specification.version = 21\n", "", 0, 0);
    server.successful(server.program.as_os_str());
    server.configure("java.specification.version = 25\n", "", 0, 0);
    server
        .launch(&["--yes"], server.program.as_os_str())
        .assert()
        .code(EmpackExitCode::General.as_i32())
        .stderr(contains("release admits [21]"));
    server.not_started();
    server.preserved();
}

#[test]
fn cli_server_java_probe_failures_retire_before_followup_launch() {
    let server = Server::new();
    for (stdout, stderr, status, diagnostic) in [
        (
            "",
            "java.specification.version = 1.8\n",
            0,
            "release requires Java 21 or newer",
        ),
        (
            "",
            "not a Java version\n",
            0,
            "did not report a Java specification version",
        ),
        (
            "java.specification.version = 21\n",
            "java.specification.version = 17\n",
            0,
            "ambiguous specification versions",
        ),
        (
            "",
            "java.specification.version = 21.0.1\n",
            0,
            "Invalid Java specification version",
        ),
        (
            "",
            "fixture Java probe failure\nfixture.private.property = fixture-private-jvm-value\n",
            9,
            "Selected Java failed its version check; run that executable's version command separately for its output",
        ),
    ] {
        server.configure(stdout, stderr, status, 0);
        let tools = snapshot(server.program.parent().unwrap());
        let output = server
            .launch(&["--yes"], server.program.as_os_str())
            .assert()
            .code(EmpackExitCode::General.as_i32())
            .stderr(contains(diagnostic))
            .get_output()
            .clone();
        if status != 0 {
            for stream in [&output.stdout, &output.stderr] {
                let text = String::from_utf8_lossy(stream);
                assert!(!text.contains("fixture Java probe failure"));
                assert!(!text.contains("fixture-private-jvm-value"));
            }
        }
        server.invocation("probe.json", &["-XshowSettings:properties", "-version"]);
        server.not_started();
        assert_eq!(snapshot(server.program.parent().unwrap()), tools);
        server.preserved();
        server.configure("", "java.specification.version = 21\n", 0, 0);
        server.successful(server.program.as_os_str());
    }
    server.configure("", "java.specification.version = 21\n", 0, 7);
    server
        .launch(&["--yes"], server.program.as_os_str())
        .assert()
        .code(EmpackExitCode::General.as_i32())
        .stderr(contains(EXIT_SEVEN));
    server.invocation(
        "runtime.json",
        &["-jar", "server.jar", "nogui", "literal argument", "é"],
    );
    server.preserved();
    server.configure("", "java.specification.version = 21\n", 0, 0);
    server.successful(server.program.as_os_str());
    server.configure("", "java.specification.version = 21\n", 0, 130);
    let output = server
        .launch(&["--yes"], server.program.as_os_str())
        .assert()
        .code(EmpackExitCode::Interrupted.as_i32())
        .stderr(contains("Runtime interrupted"))
        .get_output()
        .clone();
    assert!(!String::from_utf8_lossy(&output.stderr).contains("Error:"));
    server.preserved();
    server.configure("", "java.specification.version = 21\n", 0, 0);
    server.successful(server.program.as_os_str());
}

#[test]
fn cli_generated_server_scripts_preserve_java_selection_and_classified_status() {
    const CHILD: &str = "engine::build::server::tests::generated_server_entrypoint_fixture";
    let driver = Driver::native_fixture(CHILD);
    // Windows launcher argv uses the system ANSI encoding before Java Main.
    // Keep this external JVM path within that documented scope. Direct native
    // launch cases retain the Unicode Java directory through Server::new().
    let server = Server::with_java_directory(if cfg!(windows) {
        "server Java fixture "
    } else {
        "server Java Ω "
    });
    server.generate_scripts(&driver, true);
    let bootstrap = snapshot(&server.instance.dir().join(".empack-consumer"));
    let scripts = |name: &str| fs::read(server.instance.dir().join(name)).unwrap();
    let names = [
        "start.sh",
        "start_gui.sh",
        "install_pack.sh",
        "start.bat",
        "start_gui.bat",
        "install_pack.bat",
    ];
    let generated = names.map(scripts);
    // The generated command selects exact descriptor bytes, even when an
    // installation already exists. Neither entrypoint may proceed to Java
    // after a well-formed but differently hashed descriptor is substituted.
    let descriptor = server.instance.dir().join(".empack-consumer/release.json");
    let original_descriptor = fs::read(&descriptor).unwrap();
    let mut changed_descriptor = original_descriptor.clone();
    changed_descriptor.push(b'\n');
    fs::write(&descriptor, changed_descriptor).unwrap();
    let changed_instance = snapshot(server.instance.dir());
    for gui in [false, true] {
        server.configure("", "java.specification.version = 21\n", 0, 0);
        let tools = snapshot(server.program.parent().unwrap());
        server
            .script(gui, true)
            .assert()
            .failure()
            .stderr(contains("Selected release payload identity mismatch"));
        assert!(
            !server
                .fixtures
                .path()
                .join("observations/probe.json")
                .exists()
        );
        server.not_started();
        assert_eq!(snapshot(server.instance.dir()), changed_instance);
        assert_eq!(snapshot(server.program.parent().unwrap()), tools);
        server.preserved();
    }
    fs::write(descriptor, original_descriptor).unwrap();
    for gui in [false, true] {
        for home in [false, true] {
            server.configure("", "java.specification.version = 21\n", 0, 0);
            let tools = snapshot(server.program.parent().unwrap());
            server.script(gui, home).assert().success();
            let mut expected = vec!["-jar", "server.jar"];
            if !gui {
                expected.push("nogui");
            }
            expected.extend(["literal argument", "é"]);
            server.invocation("runtime.json", &expected);
            server.invocation("probe.json", &["-XshowSettings:properties", "-version"]);
            assert_eq!(snapshot(server.program.parent().unwrap()), tools);
            server.preserved();
        }
    }
    // Preparation must reject drift before either the probe or runtime is invoked.
    let jar = server.instance.dir().join("game/server.jar");
    let original = fs::read(&jar).unwrap();
    fs::write(&jar, b"local server edit").unwrap();
    let edited = snapshot(&server.instance.dir().join("game"));
    server.configure("", "java.specification.version = 21\n", 0, 0);
    server
        .script(false, true)
        .assert()
        .failure()
        .stderr(contains("Instance has unresolved file conflicts"));
    assert_eq!(snapshot(&server.instance.dir().join("game")), edited);
    assert!(
        !server
            .fixtures
            .path()
            .join("observations/probe.json")
            .exists()
    );
    server.not_started();
    fs::write(&jar, original).unwrap();
    for (version, runtime, diagnostic) in [("1.8", 0, "requires Java 21"), ("21", 7, EXIT_SEVEN)] {
        server.configure(
            "",
            &format!("java.specification.version = {version}\n"),
            0,
            runtime,
        );
        server
            .script(false, true)
            .assert()
            .code(EmpackExitCode::General.as_i32())
            .stderr(contains(diagnostic));
        if version == "1.8" {
            server.not_started();
        }
        server.preserved();
    }
    // The unchanged generated entry point invokes the real JAR and selected
    // native product. Runtime interruption must survive both parent processes.
    server.configure("", "java.specification.version = 21\n", 0, 130);
    let output = server
        .script(false, true)
        .assert()
        .code(EmpackExitCode::Interrupted.as_i32())
        .stderr(contains("Runtime interrupted"))
        .get_output()
        .clone();
    assert!(!String::from_utf8_lossy(&output.stderr).contains("Error:"));
    server.preserved();
    assert_eq!(
        snapshot(&server.instance.dir().join(".empack-consumer")),
        bootstrap
    );
    assert_eq!(
        names.map(scripts),
        generated,
        "executing a script must not rewrite it"
    );
    server.configure("", "java.specification.version = 21\n", 0, 0);
    server.script(false, false).assert().success();
    server.preserved();

    // Standalone scripts validate Java but invoke it directly, preserving its status without managed ownership.
    server.generate_scripts(&driver, false);
    let host = snapshot(&server.instance.dir().join(".host-state"));
    for gui in [false, true] {
        server.configure("", "java.specification.version = 21\n", 0, 7);
        server.script(gui, true).assert().code(7);
        server.invocation("probe.json", &["-XshowSettings:properties", "-version"]);
        let mut arguments = vec!["-jar", "server.jar"];
        if !gui {
            arguments.push("nogui");
        }
        arguments.extend(["literal argument", "é"]);
        server.invocation("runtime.json", &arguments);
        server.preserved();
    }
    for major in [17, 25] {
        server.configure("", &format!("java.specification.version = {major}\n"), 0, 0);
        server.script(false, true).assert().code(1);
        server.not_started();
        server.preserved();
    }
    assert_eq!(snapshot(&server.instance.dir().join(".host-state")), host);
}
