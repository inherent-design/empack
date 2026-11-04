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
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    let probe = arguments == ["-XshowSettings:properties", "-version"];
    let observations = PathBuf::from(config["observations"].as_str().unwrap());
    let cwd = std::env::current_dir().unwrap();
    // JSON strings cannot represent every native path or argument. Keep the
    // readable observation when available, and independently retain Unix bytes.
    let observation = json!({
        "program": program.to_str(),
        "cwd": cwd.to_str(),
        "arguments": arguments.iter().map(|value| value.to_str()).collect::<Option<Vec<_>>>(),
    });
    #[cfg(unix)]
    let observation = {
        use std::os::unix::ffi::OsStrExt;
        let mut observation = observation;
        observation["native"] = json!({
            "encoding": "unix-bytes",
            "program": program.as_os_str().as_bytes(),
            "cwd": cwd.as_os_str().as_bytes(),
            "arguments": arguments.iter().map(|value| value.as_bytes()).collect::<Vec<_>>(),
        });
        observation
    };
    fs::write(
        observations.join(if probe { "probe.json" } else { "runtime.json" }),
        serde_json::to_vec(&observation).unwrap(),
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
