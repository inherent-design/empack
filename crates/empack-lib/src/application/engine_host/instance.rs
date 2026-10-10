//! Explicit snapshot installation, with no implicit subscription or author resolution.
use super::*;
use crate::{
    application::cli::InstanceCommand,
    engine::{
        api::{InstallInstanceRequest, OperationPreview},
        instance::{ChoiceSelection, InstanceSide, SelectedRelease},
        release::{DecodedRelease, trust::SelectedSnapshot},
    },
};
use std::collections::BTreeMap;

pub(in crate::application) async fn dispatch(
    session: &dyn Session,
    command: InstanceCommand,
) -> Result<()> {
    let InstanceCommand::Install {
        release,
        sha256,
        side,
        choices,
        files,
    } = command;
    let (invocation, root) = project_path(session)?;
    let selected = absolute(&invocation, &release);
    let assets = selected
        .parent()
        .context("Release source has no directory")?
        .to_owned();
    let mut local_files = BTreeMap::new();
    for input in files {
        let (key, path) = input
            .split_once('=')
            .context("File association must be KEY=PATH")?;
        ensure!(
            !key.is_empty() && !path.is_empty(),
            "File association must name a key and path"
        );
        ensure!(
            local_files
                .insert(key.to_owned(), absolute(&invocation, Path::new(path)))
                .is_none(),
            "Duplicate file association"
        );
    }
    let choices = choices
        .into_iter()
        .map(|input| {
            let (key, value) = input.split_once('=').context("Choice must be KEY=VALUE")?;
            Ok(ChoiceSelection {
                key: key.into(),
                value: value.into(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let decoded = initialize::discover(session, move |mut scope| async move {
        let mut resources = operation_resources().capture;
        resources.scratch_bytes = 0; // Bounded document reads create no temporary content.
        let work =
            scope.spawn_blocking(resources, operation_resources().prepared, move |cancel| {
                DecodedRelease::read(&selected, &cancel)
            })?;
        scope.accept(work.wait().await?)?.transpose()
    })
    .await?;
    ensure!(
        local_files
            .keys()
            .all(|key| decoded.document().files.iter().any(|f| f.key == *key)),
        "File association does not name a release file"
    );
    // Development builds implement this protocol version without pretending to be a tagged binary.
    let version = if env!("CARGO_PKG_VERSION") == "0.0.0-dev" {
        "0.6.0-beta"
    } else {
        env!("CARGO_PKG_VERSION")
    };
    let snapshot =
        SelectedSnapshot::select(decoded.bytes(), &sha256, &semver::Version::parse(version)?)?;
    let request = InstallInstanceRequest {
        release: SelectedRelease::Snapshot(snapshot),
        side: match side.as_str() {
            "server" => InstanceSide::Server,
            "client" => InstanceSide::Client,
            _ => anyhow::bail!("Instance side must be client or server"),
        },
        choices,
        supplied: BTreeMap::new(),
        local_files,
        assets: Some(assets),
    };
    let engine = engine(session.config().app_config(), &invocation)?;
    let result = async {
        let Preparation::Ready(prepared) =
            cancellable(session, engine.prepare(root, request)).await?
        else {
            anyhow::bail!("Instance content requires input");
        };
        let OperationPreview::Instance(view) = prepared.view() else {
            anyhow::bail!("Unexpected instance preview");
        };
        session.display().status().info(&format!(
            "Release {}: {} exact file changes",
            view.record.release,
            view.files.changes().len()
        ));
        apply(session, &engine, prepared, "Instance content", |receipt| {
            let ExecutionReceipt::Instance(receipt) = receipt else {
                anyhow::bail!("Unexpected instance receipt");
            };
            Ok(format!(
                "Applied release {} to game/",
                receipt.record.release
            ))
        })
        .await
    }
    .await;
    engine.shutdown().await;
    result
}
