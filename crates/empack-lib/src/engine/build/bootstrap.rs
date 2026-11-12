//! Recipe-owned tool bytes are outside the installed game inventory.
use crate::{
    application::process_runtime::Cancellation,
    engine::{
        content::{InitialObservation, SourceEvidencePolicy, verify_stream},
        mrpack::AcquiredBuildFile,
    },
};
use anyhow::Result;
use empack_core::{files::FilePermissions, model::ExpectedContent};

pub(crate) const PATH: &str = ".empack-tools/empack-bootstrap.jar";
pub(super) const PREPARE: &str = ".empack-tools/prepare.json";
pub(super) const LAUNCH: &str = ".empack-tools/launch.json";
const JAR: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/empack-bootstrap.jar"));

pub(super) fn file(cancel: &Cancellation) -> Result<AcquiredBuildFile> {
    retained(JAR, cancel)
}

fn retained(mut bytes: &[u8], cancel: &Cancellation) -> Result<AcquiredBuildFile> {
    let size = bytes.len() as u64;
    Ok(AcquiredBuildFile {
        content: verify_stream(
            &mut bytes,
            &ExpectedContent {
                digests: None,
                size: Some(size),
                accepted_observation: None,
            },
            8 << 20,
            SourceEvidencePolicy::Compatibility,
            InitialObservation::Accepted,
            cancel,
        )?,
        permissions: FilePermissions {
            readonly: false,
            executable: false,
        },
    })
}

pub(super) fn invocation(
    prepare: bool,
    prism: bool,
    subscribed: bool,
    release: &str,
    cancel: &Cancellation,
) -> Result<AcquiredBuildFile> {
    let mut arguments = vec!["--workdir", ".", "--yes", "instance"];
    if prepare {
        arguments.extend([
            "prepare",
            if prism {
                ".minecraft/.empack-consumer/release.json"
            } else {
                ".empack-consumer/release.json"
            },
            "--sha256",
            release,
            "--layout",
            if prism { "prism" } else { "game" },
            "--side",
            if prism { "client" } else { "server" },
        ]);
        if subscribed {
            arguments.push("--require-subscription");
        }
    } else {
        arguments.push("launch");
        if !prism {
            arguments.push("--server");
        }
        if subscribed {
            arguments.push("--check-updates");
        }
        if prism {
            arguments.push("--java-runtime");
        }
        arguments.push("--");
    }
    let bytes = serde_json::to_vec(&serde_json::json!({
        "schema": "empack-bootstrap-invocation/1",
        "engine_version": crate::engine::compatibility_version(),
        "arguments": arguments,
        "append_arguments": !prepare,
    }))?;
    anyhow::ensure!(
        bytes.len() <= 1 << 20,
        "Bootstrap invocation exceeds its byte limit"
    );
    retained(&bytes, cancel)
}

pub(super) fn prism(file: &str) -> String {
    format!(
        "\"$INST_JAVA\" -Dfile.encoding=UTF-8 -jar \"../{PATH}\" --invocation-file \"../{file}\""
    )
}

pub(super) fn server(file: &str, windows: bool) -> String {
    if windows {
        format!(
            "\"%_EMPACK_JAVA%\" -Dfile.encoding=UTF-8 -jar \"{}\" --invocation-file \"{}\"",
            PATH.replace('/', "\\"),
            file.replace('/', "\\"),
        )
    } else {
        format!("\"$JAVA_PATH\" -Dfile.encoding=UTF-8 -jar \"{PATH}\" --invocation-file \"{file}\"")
    }
}
