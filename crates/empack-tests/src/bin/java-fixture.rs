//! A selected local executable for CLI tests, not a Java implementation.
//! Built only by the external test package; no product control selects this fixture.
use serde_json::{Value, json};
use std::{fs, io::Write, path::PathBuf};

fn main() {
    let program = std::env::current_exe().unwrap();
    let config: Value = serde_json::from_slice(
        &fs::read(program.parent().unwrap().join("java-fixture.json")).unwrap(),
    )
    .unwrap();
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    let probe = arguments == ["-XshowSettings:properties", "-version"];
    let observations = PathBuf::from(config["observations"].as_str().unwrap());
    fs::write(
        observations.join(if probe { "probe.json" } else { "runtime.json" }),
        serde_json::to_vec(&json!({
            "program": program,
            "cwd": std::env::current_dir().unwrap(),
            "arguments": arguments,
        }))
        .unwrap(),
    )
    .unwrap();
    let status = if probe {
        std::io::stdout()
            .write_all(config["stdout"].as_str().unwrap().as_bytes())
            .unwrap();
        std::io::stderr()
            .write_all(config["stderr"].as_str().unwrap().as_bytes())
            .unwrap();
        config["probe_status"].as_i64().unwrap()
    } else {
        config["runtime_status"].as_i64().unwrap()
    };
    std::process::exit(i32::try_from(status).unwrap());
}
