//! Public entry points must work before loading or creating unrelated project state.
#[path = "support/files.rs"]
mod files;

use empack_tests::harness::process::{TestProject, expected_cli_version, offline_command};
use std::{collections::BTreeSet, fs, io::ErrorKind, net::TcpListener, path::PathBuf};

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
        let before = files::snapshot(self.project.dir());
        let names = || -> BTreeSet<_> {
            fs::read_dir(&self.root)
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect()
        };
        let before_names = names();
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
            .env("PATH", "")
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(code), "{args:?}: {output:?}");
        assert_eq!(files::snapshot(self.project.dir()), before, "{args:?}");
        assert_eq!(
            names(),
            before_names,
            "entry point created a directory: {args:?}"
        );
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
        for args in [["--version"], ["-V"], ["version"]] {
            let output = fixture.invoke(&args, 0);
            let stdout = String::from_utf8(output.stdout).unwrap();
            if args == ["version"] {
                assert!(
                    stdout.lines().any(|line| line.trim() == expected),
                    "{stdout}"
                );
                for label in ["Built from commit", "Build date", "Target"] {
                    assert!(stdout.contains(label), "{stdout}");
                }
            } else {
                assert_eq!(stdout.trim(), expected);
            }
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
            b"EMPACK_DRY_RUN='unterminated\n",
        )
        .unwrap();
        for args in [vec!["version"], vec![]] {
            let output = fixture.invoke(&args, 2);
            let stderr = String::from_utf8(output.stderr).unwrap();
            assert!(stderr.contains("Error:"), "{stderr}");
        }
    }
}

/// Runtime inspection describes native capabilities without installing tooling.
#[test]
fn cli_requirements_describes_native_capabilities_without_bootstrap() {
    let fixture = EntryFixture::new(false);
    let output = fixture.invoke(&["requirements"], 0);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("native engine; no external dependency manager required"),
        "{stdout}"
    );
    assert!(stdout.contains("ZIP, TAR.GZ and 7z"), "{stdout}");
    assert!(
        stdout.contains("CurseForge also requires an API key"),
        "{stdout}"
    );
}
