//! Explicit subscribed prelaunch: transport fallback ends before authentication/publication.
use super::*;
use crate::engine::{
    api::SubscriptionRequest,
    instance::subscription::{self, ChannelFetch},
};

/// None means preview or declined approval; callers must not start a runtime.
pub(super) async fn update(
    session: &dyn Session,
    allow_offline: bool,
    server: bool,
) -> Result<Option<String>> {
    update_with_transport(
        session,
        allow_offline,
        server,
        crate::engine::acquisition::HttpAcquisition::new()?,
    )
    .await
}
async fn update_with_transport(
    session: &dyn Session,
    allow_offline: bool,
    server: bool,
    transport: crate::engine::acquisition::HttpAcquisition,
) -> Result<Option<String>> {
    let (invocation, root) = project_path(session)?;
    let state = state_root(session.config().app_config(), &invocation)?;
    // A fallback is meaningful only for an existing completed installation. Launch later
    // independently verifies the selected bytes, ownership, recovery and runtime lease.
    let installed = inspect(session, root.clone(), None, state.clone()).await?;
    let (record, release) = &*installed;
    let selected_root = root.clone();
    let selected_state = state.clone();
    let channel_transport = transport.clone();
    let fetched = initialize::discover(session, move |mut scope| async move {
        subscription::fetch_channel(
            &mut scope,
            selected_root,
            selected_state,
            &channel_transport,
        )
        .await
    })
    .await?;
    let envelope = match fetched {
        ChannelFetch::Available(bytes) => bytes,
        ChannelFetch::Unavailable(error) => {
            if !allow_offline {
                return Err(error.context("Channel unavailable; explicitly select --allow-offline to launch the completed release"));
            }
            session.display().status().warning(&format!(
                "Channel unavailable ({error:#}); checking completed release {} for offline launch",
                record.release
            ));
            return Ok(Some(record.release.clone()));
        }
    };
    let version = semver::Version::parse(if env!("CARGO_PKG_VERSION") == "0.0.0-dev" {
        "0.6.0-beta"
    } else {
        env!("CARGO_PKG_VERSION")
    })?;
    let engine = engine(session.config().app_config(), &invocation)?;
    let result = async {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs()
            .try_into()?;
        let Preparation::Ready(prepared) = cancellable(
            session,
            engine.prepare(
                root.clone(),
                SubscriptionRequest::Observe {
                    envelope: envelope.to_vec(),
                    now,
                    engine: version.clone(),
                },
            ),
        )
        .await?
        else {
            anyhow::bail!("Channel observation requires input")
        };
        // Never continue after preview/decline: fetching a new release requires the durable floor.
        if !approve(session, "Channel observation")? {
            return Ok(None);
        }
        execute_approved(session, &engine, prepared, "Channel observation", |receipt| {
            ensure!(matches!(receipt, ExecutionReceipt::Subscription(_)), "Unexpected channel receipt");
            Ok("Authenticated channel observation saved".into())
        }).await?;
        let selected_root = root.clone();
        let proof = initialize::discover(session, move |mut scope| async move {
            subscription::fetch_release(
                &mut scope,
                selected_root,
                state,
                &transport,
                version,
            ).await
        }).await?;
        ensure!(proof.release().document().runtime == release.document().runtime
            || (server && record.side == InstanceSide::Server && proof.release().document().server_launch.is_some()),
            if record.layout == InstanceLayout::Prism {
                "Channel release changes the runtime; stop Prism, run instance update, then relaunch so Prism reloads its components"
            } else {
                "Channel release changes the runtime; prepare its launcher/runtime integration before launch"
            });
        let expected = proof.release().id().to_owned();
        let proof = proof.map(std::sync::Arc::new);
        let request = InstallInstanceRequest {
            require_subscription: false,
            conflicts: Vec::new(),
            action: InstanceAction::Apply,
            release: SelectedRelease::Subscribed(std::sync::Arc::clone(&proof)),
            side: record.side,
            layout: Some(record.layout),
            choices: Vec::new(),
            supplied: BTreeMap::new(),
            local_files: BTreeMap::new(),
            assets: None,
        };
        let Some(prepared) = require_instance_ready(
            session, &engine, cancellable(session, engine.prepare(root, request)).await?
        ).await? else { return Ok(None); };
        if !approve(session, "Prelaunch update")? {
            return Ok(None);
        }
        execute_approved(session, &engine, prepared, "Prelaunch update", |receipt| {
            let ExecutionReceipt::Instance(receipt) = receipt else { anyhow::bail!("Unexpected instance receipt"); };
            ensure!(receipt.record.release == expected, "Prelaunch installed another release");
            Ok(format!("Prepared release {} for launch", receipt.record.release))
        }).await?;
        Ok(Some(expected))
    }.await;
    engine.shutdown().await;
    result
}

#[cfg(test)]
mod tests;
