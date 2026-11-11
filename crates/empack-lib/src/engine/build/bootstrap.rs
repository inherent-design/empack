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
const JAR: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/empack-bootstrap.jar"));

pub(super) fn file(cancel: &Cancellation) -> Result<AcquiredBuildFile> {
    let mut bytes = JAR;
    Ok(AcquiredBuildFile {
        content: verify_stream(
            &mut bytes,
            &ExpectedContent {
                digests: None,
                size: Some(JAR.len() as u64),
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

pub(super) fn prism() -> String {
    format!(
        "\"$INST_JAVA\" -jar \"$INST_DIR/{PATH}\" --engine-version {} --",
        crate::engine::compatibility_version()
    )
}

pub(super) fn server(script: String, windows: bool) -> String {
    let command = if windows {
        format!(
            "\"%_EMPACK_JAVA%\" -jar \"%cd%\\{}\" --engine-version {} --",
            PATH.replace('/', "\\"),
            crate::engine::compatibility_version()
        )
    } else {
        format!(
            "\"$JAVA_PATH\" -jar \"$PWD/{PATH}\" --engine-version {} --",
            crate::engine::compatibility_version()
        )
    };
    script.replace("empack --workdir", &format!("{command} --workdir"))
}
