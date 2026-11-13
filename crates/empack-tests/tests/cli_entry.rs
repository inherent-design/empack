//! Public entry points must work before loading or creating unrelated project state.
use empack_tests::harness::process::{
    TestProject, empack_bin, expected_cli_version, offline_command,
};
use empack_tests::harness::workspace::snapshot;
use std::{fs, io::ErrorKind, net::TcpListener, path::PathBuf};

struct EntryFixture {
    project: TestProject,
    root: PathBuf,
    proxy: TcpListener,
}

impl EntryFixture {
    fn new(hostile_paths: bool) -> Self {
        let project = TestProject::new();
        let root = project.dir().join("entry point Ω with spaces");
        fs::create_dir(&root).unwrap();
        assert!(
            !root.canonicalize().unwrap().starts_with(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../..")
                    .canonicalize()
                    .unwrap()
            )
        );
        // Entry-point display does not require a parseable author project.
        fs::write(root.join("empack.yml"), b"not: [valid author text\n").unwrap();
        if hostile_paths {
            for name in [".empack-cache", ".host-state", ".harness-home"] {
                fs::write(
                    root.join(name),
                    b"retained file, not a writable directory\n",
                )
                .unwrap();
            }
        }
        let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
        proxy.set_nonblocking(true).unwrap();
        Self {
            project,
            root,
            proxy,
        }
    }

    fn invoke(&self, args: &[&str], code: i32) -> std::process::Output {
        self.invoke_with_path(args, code, std::ffi::OsStr::new(""))
    }

    fn invoke_with_path(
        &self,
        args: &[&str],
        code: i32,
        path: &std::ffi::OsStr,
    ) -> std::process::Output {
        let before = snapshot(self.project.dir());
        let mut command = offline_command(&self.root);
        let address = format!("http://{}", self.proxy.local_addr().unwrap());
        for name in [
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "http_proxy",
            "https_proxy",
            "all_proxy",
        ] {
            command.env(name, &address);
        }
        let output = command
            .env("NO_PROXY", "")
            .env("no_proxy", "")
            .env("PATH", path)
            .env("PATHEXT", ".CMD")
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(code), "{args:?}: {output:?}");
        assert_eq!(snapshot(self.project.dir()), before, "{args:?}");
        match self.proxy.accept() {
            Err(error) if error.kind() == ErrorKind::WouldBlock => {}
            other => panic!("entry point attempted HTTP transport: {args:?}: {other:?}"),
        }
        output
    }
}

#[test]
fn cli_identity_outside_checkout_preserves_state() {
    let expected = format!("empack {}", expected_cli_version());
    for hostile in [false, true] {
        let fixture = EntryFixture::new(hostile);
        for args in [["--version"], ["-V"]] {
            let output = fixture.invoke(&args, 0);
            let stdout = String::from_utf8(output.stdout).unwrap();
            assert_eq!(stdout.trim(), expected);
            assert!(output.stderr.is_empty(), "{args:?}: {:?}", output.stderr);
        }
    }
}

#[test]
fn cli_registered_help_routes_ignore_unrelated_dotenv() {
    // Reviewed public routes are independent of the running binary. Compiler
    // discovery separately checks this registry for additions, removals and drift.
    let registry: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/verification/cli-bindings.json"
    ))
    .unwrap();
    let routes: Vec<_> = registry["surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| matches!(row["definition"]["kind"].as_str(), Some("command" | "leaf")))
        .map(|row| row["id"].as_str().unwrap())
        .collect();
    assert!(routes.contains(&"empack") && routes.contains(&"empack instance launch"));
    // Clap uses argv[0]'s filename for usage, including .exe on Windows.
    // Keep command routes independent of the observed help output.
    let binary = empack_bin();
    let binary_name = binary.file_name().unwrap().to_str().unwrap();
    for filename in [None, Some(".env"), Some(".env.local")] {
        let fixture = EntryFixture::new(filename.is_some());
        if let Some(filename) = filename {
            fs::write(
                fixture.root.join(filename),
                b"EMPACK_DRY_RUN='unterminated\n",
            )
            .unwrap();
        }
        for route in &routes {
            let expected_usage = format!(
                "Usage: {} ",
                route
                    .split_ascii_whitespace()
                    .enumerate()
                    .map(|(index, part)| if index == 0 { binary_name } else { part })
                    .filter(|part| *part != "help")
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            let mut args: Vec<_> = route.split_ascii_whitespace().collect();
            assert_eq!(args.remove(0), "empack");
            // Generated `help ...` routes already request display; adding
            // --help would instead make it an argument to the help command.
            if !args.contains(&"help") {
                args.push("--help");
            }
            let output = fixture.invoke(&args, 0);
            let stdout = String::from_utf8(output.stdout).unwrap();
            assert!(
                stdout.lines().any(|line| line.starts_with(&expected_usage)),
                "{route}: expected {expected_usage:?} in {stdout}"
            );
            assert!(output.stderr.is_empty(), "{route}: {:?}", output.stderr);
        }
        for args in [["-h"], ["--version"], ["-V"]] {
            let output = fixture.invoke(&args, 0);
            assert!(String::from_utf8(output.stdout).unwrap().contains("empack"));
        }
    }
}

#[test]
fn cli_unknown_commands_report_usage_without_mutation() {
    for hostile in [false, true] {
        let fixture = EntryFixture::new(hostile);
        for args in [
            vec!["unknown-fixture-command"],
            vec!["instance", "unknown-fixture-command"],
            vec!["--unknown-fixture-option"],
        ] {
            let output = fixture.invoke(&args, 2);
            assert!(output.stdout.is_empty(), "{args:?}");
            let stderr = String::from_utf8(output.stderr).unwrap();
            assert!(
                stderr.contains("Usage: empack") && stderr.contains("unknown-fixture"),
                "{stderr}"
            );
        }
        for args in [
            vec!["add"],
            vec!["remove"],
            vec!["add", "--force", "fixture"],
            vec!["add", "-f", "fixture"],
            vec!["remove", "--deps", "fixture"],
            vec!["remove", "-d", "fixture"],
        ] {
            let output = fixture.invoke(&args, 2);
            assert!(output.stdout.is_empty(), "{args:?}");
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("Usage:"),
                "{args:?}: {output:?}"
            );
        }
    }
}

#[test]
fn cli_no_command_guides_without_mutation() {
    for hostile in [false, true] {
        let fixture = EntryFixture::new(hostile);
        let output = fixture.invoke(&[], 0);
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(
            stdout.contains("Run 'empack --help' for usage information"),
            "{stdout}"
        );
        assert!(output.stderr.is_empty(), "{:?}", output.stderr);
    }
}

#[test]
fn cli_malformed_dotenv_reports_configuration_error_without_mutation() {
    for filename in [".env", ".env.local"] {
        let fixture = EntryFixture::new(false);
        fs::write(
            fixture.root.join(filename),
            b"EMPACK_KEY_CURSEFORGE='empack-synthetic-secret-do-not-render\n",
        )
        .unwrap();
        for args in [vec!["doctor"], vec![]] {
            let output = fixture.invoke(&args, 2);
            let stderr = String::from_utf8(output.stderr).unwrap();
            assert!(stderr.contains("Error:"), "{stderr}");
            assert!(
                stderr.contains("Invalid environment-file syntax"),
                "{stderr}"
            );
            assert!(stderr.contains(filename), "{stderr}");
            assert!(!stderr.contains("empack-synthetic-secret-do-not-render"));
            assert!(!stderr.contains("EMPACK_KEY_CURSEFORGE"));
            assert!(output.stdout.is_empty());
        }
    }
}

/// Runtime inspection describes native capabilities without installing tooling.
#[test]
fn cli_doctor_describes_native_capabilities_without_bootstrap() {
    let missing_java = |output: &std::process::Output| {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.is_empty(), "{stderr}");
        assert!(stdout.contains("Java: not found"), "{stdout}");
    };
    let fixture = EntryFixture::new(false);
    let output = fixture.invoke(&["doctor", "--details"], 0);
    missing_java(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("native engine; no external dependency manager required"),
        "{stdout}"
    );
    assert!(stdout.contains("ZIP, TAR.GZ and 7z"), "{stdout}");
    assert!(
        stdout.contains("Provider connectivity and credentials: not checked"),
        "{stdout}"
    );
    assert!(stdout.contains("Built from commit:"), "{stdout}");
    assert!(stdout.contains("Build date:"), "{stdout}");
    assert!(stdout.contains("Executable:"), "{stdout}");

    let bin = fixture.root.join("controlled tools Ω");
    fs::create_dir(&bin).unwrap();
    let program = bin.join(if cfg!(windows) { "java.CMD" } else { "java" });
    // Valid executable fixtures advertise different versions only if invoked.
    // Default doctor must discover the path without running any of them.
    for version in ["1.8", "21", "not-a-version"] {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::write(&program, format!("#!/bin/sh\nprintf invoked > \"$0.invoked\"\nprintf 'java.specification.version = {version}\\n' >&2\nexit 77\n")).unwrap();
            fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
        }
        #[cfg(windows)]
        fs::write(&program, format!("@echo off\r\n> \"%~f0.invoked\" echo invoked\r\necho java.specification.version = {version} 1>&2\r\nexit /b 77\r\n")).unwrap();
        let output = fixture.invoke_with_path(&["doctor", "--details"], 0, bin.as_os_str());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains(program.to_str().unwrap()), "{stdout}");
        assert!(!String::from_utf8_lossy(&output.stderr).contains("Java was not found"));
        assert!(stdout.contains("version not checked"), "{stdout}");
        let marker = program.with_file_name(format!(
            "{}.invoked",
            program.file_name().unwrap().to_str().unwrap()
        ));
        assert!(!marker.exists());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&program, fs::Permissions::from_mode(0o600)).unwrap();
        let output = fixture.invoke_with_path(&["doctor", "--details"], 0, bin.as_os_str());
        missing_java(&output);
    }
    fs::remove_file(&program).unwrap();
    fs::create_dir(&program).unwrap();
    let output = fixture.invoke_with_path(&["doctor", "--details"], 0, bin.as_os_str());
    missing_java(&output);
}

fn java_fixture(fixture: &EntryFixture, properties: &[&str], status: i32) -> PathBuf {
    let bin = fixture.root.join("selected Java Ω");
    fs::create_dir_all(&bin).unwrap();
    let program = bin.join(if cfg!(windows) { "java.CMD" } else { "java" });
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut text = String::from(
            "#!/bin/sh\n[ \"$#\" = 2 ] && [ \"$1\" = '-XshowSettings:properties' ] && [ \"$2\" = '-version' ] || exit 101\n",
        );
        for property in properties {
            assert!(!property.contains('\''));
            text.push_str(&format!("printf '%s\\n' '{property}' >&2\n"));
        }
        text.push_str(&format!("exit {status}\n"));
        fs::write(&program, text).unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
    }
    #[cfg(windows)]
    {
        let mut text = String::from(
            "@echo off\r\nif not \"%~1\"==\"-XshowSettings:properties\" exit /b 101\r\nif not \"%~2\"==\"-version\" exit /b 101\r\nif not \"%~3\"==\"\" exit /b 101\r\n",
        );
        for property in properties {
            assert!(!property.contains(['&', '|', '%', '<', '>']));
            text.push_str(&format!("echo {property} 1>&2\r\n"));
        }
        text.push_str(&format!("exit /b {status}\r\n"));
        fs::write(&program, text).unwrap();
    }
    program
}

#[test]
fn cli_doctor_probes_only_explicit_tools_and_reports_java_policy() {
    let fixture = EntryFixture::new(true);
    let missing = fixture.invoke(&["doctor", "--minecraft", "1.21.1", "--json"], 1);
    let report: serde_json::Value = serde_json::from_slice(&missing.stdout).unwrap();
    assert_eq!(report["complete"], false);
    assert_eq!(report["java"]["state"], "failed");
    assert_eq!(report["target"]["actual_admitted"], serde_json::Value::Null);
    assert!(String::from_utf8_lossy(&missing.stderr).contains("Java was not found"));

    for (version, game, code, admitted, recommended) in [
        ("1.8", None, 0, None, None),
        ("16", Some("1.17.1"), 0, Some(true), Some(17)),
        ("17", Some("1.17.1"), 0, Some(true), Some(17)),
        ("21", Some("1.17.1"), 2, Some(false), Some(17)),
        ("21", Some("1.21.1"), 0, Some(true), Some(21)),
        ("25", Some("1.21.1"), 2, Some(false), Some(21)),
        ("21", Some("custom"), 1, None, None),
    ] {
        let property = format!("java.specification.version = {version}");
        let program = java_fixture(
            &fixture,
            &[&property, "user.home = do-not-render-jvm-home"],
            0,
        );
        // Explicit paths resolve from the invocation directory, independently of workdir.
        let relative = program
            .strip_prefix(&fixture.root)
            .unwrap()
            .to_str()
            .unwrap();
        let mut args = vec![
            "--workdir",
            "missing-project",
            "doctor",
            "--java",
            relative,
            "--json",
            "--quiet",
        ];
        if let Some(game) = game {
            args.extend(["--minecraft", game]);
        }
        let output = fixture.invoke(&args, code);
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["schema"], 1);
        assert_eq!(report["complete"], code == 0);
        assert_eq!(report["java"]["state"], "checked");
        assert_eq!(
            report["java"]["major"],
            version.trim_start_matches("1.").parse::<u16>().unwrap()
        );
        let observed = PathBuf::from(report["java"]["program"].as_str().unwrap());
        assert!(observed.ends_with(relative));
        assert_eq!(
            observed.canonicalize().unwrap(),
            program.canonicalize().unwrap()
        );
        assert_eq!(report["network_checked"], false);
        if game.is_some() {
            assert_eq!(
                report["target"]["actual_admitted"],
                serde_json::json!(admitted)
            );
            assert_eq!(
                report["target"]["recommended"],
                serde_json::json!(recommended)
            );
        } else {
            assert!(report["target"].is_null());
        }
        for bytes in [&output.stdout, &output.stderr] {
            assert!(!String::from_utf8_lossy(bytes).contains("do-not-render-jvm-home"));
        }
        if code == 0 {
            assert!(output.stderr.is_empty(), "{output:?}");
        }
    }
    let program = java_fixture(&fixture, &["java.specification.version = 21"], 0);
    let output = fixture.invoke(
        &[
            "doctor",
            "--java",
            program.to_str().unwrap(),
            "--minecraft",
            "1.21.1",
            "--details",
            "--quiet",
        ],
        0,
    );
    let human = String::from_utf8(output.stdout).unwrap();
    for expected in [
        "Java: reported major 21",
        "Java executable:",
        program.to_str().unwrap(),
        "Recommended Java: 21",
        "Mods and loaders can have stricter requirements; startup was not tested.",
    ] {
        assert!(human.contains(expected), "{human}");
    }
    assert!(!human.contains("Call with --details"));
    assert!(output.stderr.is_empty());
}

#[test]
fn cli_doctor_retains_failed_probes_without_exposing_jvm_properties() {
    let fixture = EntryFixture::new(true);
    for (properties, status, expected) in [
        (
            vec![
                "java.specification.version = 21",
                "user.home = do-not-render-jvm-home",
            ],
            77,
            "failed its version check",
        ),
        (
            vec![
                "java.specification.version = 21",
                "java.specification.version = 17",
            ],
            0,
            "ambiguous specification versions",
        ),
        (
            vec!["java.specification.version = 21.0.1"],
            0,
            "Invalid Java specification version",
        ),
        (
            vec!["user.home = do-not-render-jvm-home"],
            0,
            "did not report a Java specification version",
        ),
    ] {
        let program = java_fixture(&fixture, &properties, status);
        let output = fixture.invoke(
            &[
                "doctor",
                "--java",
                program.to_str().unwrap(),
                "--json",
                "--quiet",
            ],
            1,
        );
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["complete"], false);
        assert_eq!(report["java"]["state"], "failed");
        assert!(report["java"]["major"].is_null());
        assert!(
            report["java"]["problem"]
                .as_str()
                .unwrap()
                .contains(expected)
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains(expected));
        for bytes in [&output.stdout, &output.stderr] {
            assert!(!String::from_utf8_lossy(bytes).contains("do-not-render-jvm-home"));
        }
    }
    let program = java_fixture(&fixture, &["java.specification.version = 21"], 0);
    let output = fixture.invoke(
        &["--dry-run", "doctor", "--java", program.to_str().unwrap()],
        1,
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("Java probes require execution"));
    fixture.invoke(&["doctor", "--details", "--json"], 2);
    for removed in ["requirements", "version"] {
        let output = fixture.invoke(&[removed], 2);
        assert!(String::from_utf8_lossy(&output.stderr).contains("unrecognized subcommand"));
    }
}
