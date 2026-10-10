//! Crash evidence is separate from kernel lock lifetime. Never infer process retirement from a vanished owner.
use super::*;
const MARKER: &str = "runtime.json";
pub(super) fn ensure_stopped(state: &Dir) -> Result<()> {
    match state.symlink_metadata(MARKER) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
        Ok(_) => anyhow::bail!(
            "Instance runtime retirement is unconfirmed; stop remaining processes, then run instance recover-runtime --acknowledge-stopped"
        ),
    }
}
fn marker(state: &Dir) -> Result<Option<Vec<u8>>> {
    let mut file = match native::open_file(state, MARKER) {
        Ok(file) => file,
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    copy_bounded(&mut file, &mut bytes, 4096, &Cancellation::default())?;
    Ok(Some(bytes))
}
impl InstanceRunLease {
    pub(in crate::engine) fn begin(&mut self) -> Result<()> {
        ensure!(self.marker.is_none(), "Runtime lease was already started");
        ensure_stopped(&self.state)?;
        let record = serde_json::json!({
            "schema": 1,
            "parent": std::process::id(),
            "nonce": format!("{:x}-{:x}", SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos(), NEXT_OPERATION.fetch_add(1, Ordering::Relaxed)),
        });
        let bytes = serde_json::to_vec(&record)?;
        write_record(&self.state, MARKER, &record)?;
        self.marker = Some(bytes);
        Ok(())
    }
    /// Only call after process retirement is established, including cancellation and spawn failure.
    pub(in crate::engine) fn complete(self) -> Result<()> {
        if let Some(expected) = &self.marker {
            ensure!(
                marker(&self.state)?.as_ref() == Some(expected),
                "Runtime evidence changed before retirement"
            );
            self.state.remove_file(MARKER)?;
            sync_directory(&self.state).context(
                "Runtime retired and marker removed, but directory synchronization failed",
            )?;
        }
        Ok(())
    }
}
/// An observed private-state file, never a path decoded from imported metadata.
pub(in crate::engine) struct PendingRuntime {
    root: Binding,
    bytes: Vec<u8>,
}
impl Publisher {
    pub(in crate::engine) fn observe_runtime(
        &self,
        root: &ProjectReadRoot,
    ) -> Result<Option<PendingRuntime>> {
        root.check_binding()?;
        let state = match self.host.open_dir_nofollow(root_key(root)?) {
            Ok(state) => state,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        marker(&state).map(|bytes| {
            bytes.map(|bytes| PendingRuntime {
                root: root.binding.into(),
                bytes,
            })
        })
    }
    /// The host explicitly asserts all runtime processes stopped. A live managed lease still refuses this action.
    pub(in crate::engine) fn acknowledge_stopped(
        &self,
        root: &ProjectReadRoot,
        expected: PendingRuntime,
    ) -> Result<()> {
        root.check_binding()?;
        ensure!(
            Binding::from(root.binding) == expected.root,
            "Runtime recovery selected another root"
        );
        let state = self.project_state(root)?;
        let run = lock_file(&state, "instance-run.lock")?;
        run.try_lock().context("Instance is still running")?;
        let publication = lock_file(&state, "operation.lock")?;
        publication
            .try_lock()
            .context("Instance publication is busy")?;
        ensure!(
            marker(&state)?.as_ref() == Some(&expected.bytes),
            "Runtime evidence changed; inspect recovery again"
        );
        state.remove_file(MARKER)?;
        sync_directory(&state)
            .context("Runtime marker removed, but directory synchronization failed")?;
        Ok(())
    }
}
