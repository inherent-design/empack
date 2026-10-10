//! Engine-owned drivers retain tasks and terminal results independently of UI handles.
use super::resources::{AdmissionError, AdmissionPermit, ResourceGovernor, ResourceRequest};
use crate::application::process_runtime::Cancellation;
use std::{
    collections::BTreeMap,
    future::Future,
    ops::Deref,
    sync::{Arc, Mutex, MutexGuard, Weak},
};
use tokio::{
    sync::{oneshot, watch},
    task::JoinHandle,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct OperationId(u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttemptId(u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationPhase {
    Preparing,
    Running,
    Retiring,
    Terminal,
}
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeError {
    #[error(transparent)]
    Admission(#[from] AdmissionError),
    #[error("An operation needs an existing Tokio runtime")]
    NoRuntime,
    #[error("Operation registry is full; release a completed result first")]
    RegistryFull,
    #[error("Operation admission is closed")]
    Closed,
    #[error("Operation was cancelled before publication")]
    Cancelled,
    #[error("Worker result belongs to a closed or superseded preparation")]
    StaleResult,
    #[error("A driver or worker panicked")]
    Panicked,
    #[error("Task reservations require a job and cannot retain job capacity")]
    InvalidReservation,
    #[error("Operation identifier space is exhausted")]
    IdentifierExhausted,
}

impl RuntimeError {
    /// Disposable work may skip exhausted capacity, never cancellation or closed ownership.
    pub(super) fn is_capacity_exhausted(&self) -> bool {
        matches!(
            self,
            Self::Admission(AdmissionError::Busy { .. } | AdmissionError::TooLarge { .. })
        )
    }
}

pub enum OperationOutcome<T> {
    Completed(T),
    Failed(RuntimeError),
}
pub struct OperationStatus<T> {
    pub phase: OperationPhase,
    pub terminal: Option<Arc<OperationOutcome<T>>>,
}
struct Entry<T> {
    scope: Arc<ScopeInner>,
    status: watch::Sender<Arc<OperationStatus<T>>>,
    driver: Option<JoinHandle<()>>,
}
struct Registry<T> {
    open: bool,
    next: u64,
    entries: BTreeMap<OperationId, Entry<T>>,
}
/// Uses the host runtime. Shutdown closes admission, cancels preparation, and awaits retirement.
pub struct OperationRuntime<T> {
    governor: ResourceGovernor,
    maximum: usize,
    registry: Arc<Mutex<Registry<T>>>,
}
fn locked<T>(value: &Mutex<T>) -> MutexGuard<'_, T> {
    value.lock().unwrap_or_else(|error| error.into_inner())
}
impl<T: Send + Sync + 'static> OperationRuntime<T> {
    pub fn new(governor: ResourceGovernor, retained_operations: usize) -> Self {
        Self {
            governor,
            maximum: retained_operations,
            registry: Arc::new(Mutex::new(Registry {
                open: true,
                next: 0,
                entries: BTreeMap::new(),
            })),
        }
    }
    /// Registration and admission close share one lock; no started driver can escape shutdown.
    pub fn start<F, Fut>(&self, run: F) -> Result<OperationHandle<T>, RuntimeError>
    where
        F: FnOnce(WorkScope) -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, RuntimeError>> + Send + 'static,
    {
        self.start_owned(run, true)
    }
    /// Preparation results travel through a separate owned channel. Retire the registry entry
    /// automatically even if the waiting preparation future is dropped.
    pub(super) fn start_ephemeral<F, Fut>(&self, run: F) -> Result<OperationHandle<T>, RuntimeError>
    where
        F: FnOnce(WorkScope) -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, RuntimeError>> + Send + 'static,
    {
        self.start_owned(run, false)
    }
    fn start_owned<F, Fut>(&self, run: F, retain: bool) -> Result<OperationHandle<T>, RuntimeError>
    where
        F: FnOnce(WorkScope) -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, RuntimeError>> + Send + 'static,
    {
        let host = tokio::runtime::Handle::try_current().map_err(|_| RuntimeError::NoRuntime)?;
        let mut registry = locked(&self.registry);
        if !registry.open {
            return Err(RuntimeError::Closed);
        }
        if registry.entries.len() >= self.maximum {
            return Err(RuntimeError::RegistryFull);
        }
        registry.next = registry
            .next
            .checked_add(1)
            .ok_or(RuntimeError::IdentifierExhausted)?;
        let id = OperationId(registry.next);
        let (status, receiver) = watch::channel(Arc::new(OperationStatus {
            phase: OperationPhase::Preparing,
            terminal: None,
        }));
        let operation_cancel = Cancellation::default();
        let scope = Arc::new(ScopeInner {
            operation: id,
            governor: self.governor.clone(),
            cancel: operation_cancel.child(),
            operation_cancel,
            state: Mutex::new(ScopeState {
                open: true,
                accepting: true,
                attempt: AttemptId(0),
                tasks: Vec::new(),
            }),
        });
        let owned_scope = scope.clone();
        let final_status = status.clone();
        let owner_registry = self.registry.clone();
        let driver = host.spawn(async move {
            // The supervisor retains the scope even if construction or polling of the driver panics.
            let worker_scope = owned_scope.clone();
            let work_status = final_status.clone();
            let result = tokio::spawn(async move {
                work_status.send_replace(Arc::new(OperationStatus {
                    phase: OperationPhase::Running,
                    terminal: None,
                }));
                run(WorkScope {
                    inner: worker_scope,
                })
                .await
            })
            .await;
            final_status.send_replace(Arc::new(OperationStatus {
                phase: OperationPhase::Retiring,
                terminal: None,
            }));
            let retired = owned_scope.close_and_retire().await;
            let outcome = match (result, retired) {
                (Ok(Ok(value)), Ok(())) => OperationOutcome::Completed(value),
                (Ok(Err(error)), _) | (_, Err(error)) => OperationOutcome::Failed(error),
                (Err(_), _) => OperationOutcome::Failed(RuntimeError::Panicked),
            };
            // Store first; watch notifications are hints and can be missed or coalesced.
            final_status.send_replace(Arc::new(OperationStatus {
                phase: OperationPhase::Terminal,
                terminal: Some(Arc::new(outcome)),
            }));
            if !retain {
                locked(&owner_registry).entries.remove(&id);
            }
        });
        registry.entries.insert(
            id,
            Entry {
                scope: scope.clone(),
                status,
                driver: Some(driver),
            },
        );
        Ok(OperationHandle {
            id,
            receiver,
            cancel: scope.operation_cancel.clone(),
            cancel_on_drop: true,
        })
    }
    pub fn observe(&self, id: OperationId) -> Option<OperationHandle<T>> {
        locked(&self.registry)
            .entries
            .get(&id)
            .map(|entry| OperationHandle {
                id,
                receiver: entry.status.subscribe(),
                cancel: entry.scope.operation_cancel.clone(),
                cancel_on_drop: false,
            })
    }
    /// Explicit result retention policy. Active entries cannot be evicted by a disconnected client.
    pub fn release_completed(&self, id: OperationId) -> bool {
        let mut registry = locked(&self.registry);
        if registry
            .entries
            .get(&id)
            .is_some_and(|entry| entry.status.borrow().terminal.is_some())
        {
            registry.entries.remove(&id);
            true
        } else {
            false
        }
    }
    pub async fn shutdown(&self) {
        let entries = {
            let mut registry = locked(&self.registry);
            registry.open = false;
            registry
                .entries
                .iter_mut()
                .map(|(id, entry)| {
                    entry.scope.close();
                    (
                        OperationHandle {
                            id: *id,
                            receiver: entry.status.subscribe(),
                            cancel: entry.scope.operation_cancel.clone(),
                            cancel_on_drop: false,
                        },
                        entry.driver.take(),
                    )
                })
                .collect::<Vec<_>>()
        };
        for (mut handle, driver) in entries {
            // Another concurrent shutdown may own the join; retained status still waits for retirement.
            handle.wait().await;
            if let Some(driver) = driver {
                let _ = driver.await;
            }
        }
    }
}
impl<T> Drop for OperationRuntime<T> {
    fn drop(&mut self) {
        let mut registry = locked(&self.registry);
        registry.open = false;
        for entry in registry.entries.values() {
            entry.scope.close();
        }
        // Drivers continue retiring on the host runtime. Destruction never blocks a runtime thread.
    }
}
pub struct OperationHandle<T> {
    id: OperationId,
    receiver: watch::Receiver<Arc<OperationStatus<T>>>,
    cancel: Cancellation,
    cancel_on_drop: bool,
}
impl<T> OperationHandle<T> {
    pub fn id(&self) -> OperationId {
        self.id
    }
    pub fn status(&self) -> Arc<OperationStatus<T>> {
        self.receiver.borrow().clone()
    }
    pub fn subscribe(&self) -> watch::Receiver<Arc<OperationStatus<T>>> {
        self.receiver.clone()
    }
    pub fn cancel(&self) {
        self.cancel.cancel();
    }
    pub async fn wait(&mut self) -> Arc<OperationOutcome<T>> {
        loop {
            if let Some(result) = self.receiver.borrow_and_update().terminal.clone() {
                return result;
            }
            if self.receiver.changed().await.is_err() {
                return Arc::new(OperationOutcome::Failed(RuntimeError::Closed));
            }
        }
    }
}
impl<T> Drop for OperationHandle<T> {
    fn drop(&mut self) {
        if self.cancel_on_drop {
            self.cancel.cancel();
        }
    }
}

struct ScopeState {
    open: bool,
    accepting: bool,
    attempt: AttemptId,
    tasks: Vec<JoinHandle<Result<(), RuntimeError>>>,
}
struct ScopeInner {
    operation: OperationId,
    governor: ResourceGovernor,
    cancel: Cancellation,
    operation_cancel: Cancellation,
    state: Mutex<ScopeState>,
}
impl ScopeInner {
    fn close(&self) {
        let mut state = locked(&self.state);
        state.open = false;
        state.accepting = false;
        self.operation_cancel.cancel();
    }
    async fn close_and_retire(&self) -> Result<(), RuntimeError> {
        let tasks = {
            let mut state = locked(&self.state);
            state.open = false;
            state.accepting = false;
            self.cancel.cancel();
            std::mem::take(&mut state.tasks)
        };
        let mut result = Ok(());
        for task in tasks {
            match task.await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => result = Err(error),
                Err(_) => result = Err(RuntimeError::Panicked),
            }
        }
        result
    }
}
/// The driver owns this capability. Workers get only a cancellation token and owned inputs.
pub struct WorkScope {
    inner: Arc<ScopeInner>,
}
impl WorkScope {
    /// Reserve retained storage before a worker can write it. The storage owner receives the
    /// permit, including when a failed append leaves charged bytes until the backing retires.
    pub(super) fn reserve_storage(
        &self,
        request: ResourceRequest,
    ) -> Result<AdmissionPermit, RuntimeError> {
        if request.jobs != 0 {
            return Err(RuntimeError::InvalidReservation);
        }
        let state = locked(&self.inner.state);
        if !state.open || !state.accepting {
            return Err(RuntimeError::Closed);
        }
        if self.inner.cancel.is_cancelled() {
            return Err(RuntimeError::Cancelled);
        }
        Ok(self.inner.governor.try_admit(request)?)
    }

    /// An admission estimate only; registering work atomically reserves capacity afterward.
    pub(super) fn available_scratch_bytes(&self) -> u64 {
        let status = self.inner.governor.status();
        status
            .limits
            .scratch_bytes
            .saturating_sub(status.reserved.scratch_bytes)
    }

    pub fn cancellation(&self) -> Cancellation {
        self.inner.operation_cancel.clone()
    }
    /// Changing attempts invalidates prior results, without pretending old workers have retired.
    pub fn next_attempt(&mut self) -> Result<AttemptId, RuntimeError> {
        let mut state = locked(&self.inner.state);
        if !state.open {
            return Err(RuntimeError::Closed);
        }
        state.attempt.0 = state
            .attempt
            .0
            .checked_add(1)
            .ok_or(RuntimeError::IdentifierExhausted)?;
        Ok(state.attempt)
    }
    /// Close the acceptance gate before freezing a candidate. No worker result can enter it afterward.
    pub async fn retire(self) -> Result<(), RuntimeError> {
        self.inner.close_and_retire().await
    }
    pub fn spawn<F, Fut, T>(
        &self,
        request: ResourceRequest,
        retained: ResourceRequest,
        run: F,
    ) -> Result<WorkHandle<T>, RuntimeError>
    where
        T: Send + 'static,
        F: FnOnce(Cancellation) -> Fut + Send + 'static,
        Fut: Future<Output = T> + Send + 'static,
    {
        self.register(request, retained, |cancel, mut permit| {
            tokio::spawn(async move {
                let output_permit = permit.split(retained)?;
                let value = run(cancel).await;
                drop(permit);
                Ok(RetainedOutput {
                    value,
                    _permit: output_permit,
                })
            })
        })
    }
    pub fn spawn_blocking<F, T>(
        &self,
        request: ResourceRequest,
        retained: ResourceRequest,
        run: F,
    ) -> Result<WorkHandle<T>, RuntimeError>
    where
        T: Send + 'static,
        F: FnOnce(Cancellation) -> T + Send + 'static,
    {
        self.spawn_blocking_deferred(request, retained, || run)
    }
    /// Create the worker only after admission. Callers keep ownership of inputs when
    /// optional work cannot reserve capacity; no worker starts on a failed reservation.
    pub(super) fn spawn_blocking_deferred<F, T>(
        &self,
        request: ResourceRequest,
        retained: ResourceRequest,
        make_worker: impl FnOnce() -> F,
    ) -> Result<WorkHandle<T>, RuntimeError>
    where
        T: Send + 'static,
        F: FnOnce(Cancellation) -> T + Send + 'static,
    {
        self.register(request, retained, |cancel, mut permit| {
            let run = make_worker();
            tokio::task::spawn_blocking(move || {
                let output_permit = permit.split(retained)?;
                let value = run(cancel);
                drop(permit);
                Ok(RetainedOutput {
                    value,
                    _permit: output_permit,
                })
            })
        })
    }
    fn register<T: Send + 'static>(
        &self,
        request: ResourceRequest,
        retained: ResourceRequest,
        start: impl FnOnce(
            Cancellation,
            AdmissionPermit,
        ) -> JoinHandle<Result<RetainedOutput<T>, AdmissionError>>,
    ) -> Result<WorkHandle<T>, RuntimeError> {
        if request.jobs == 0
            || retained.jobs != 0
            || retained.memory_bytes > request.memory_bytes
            || retained.scratch_bytes > request.scratch_bytes
            || retained.open_files > request.open_files
        {
            return Err(RuntimeError::InvalidReservation);
        }
        let mut state = locked(&self.inner.state);
        if !state.open {
            return Err(RuntimeError::Closed);
        }
        if self.inner.cancel.is_cancelled() {
            return Err(RuntimeError::Cancelled);
        }
        let permit = self.inner.governor.try_admit(request)?;
        let token = WorkToken {
            operation: self.inner.operation,
            attempt: state.attempt,
            owner: Arc::downgrade(&self.inner),
        };
        let task = start(self.inner.cancel.clone(), permit);
        let (sender, receiver) = oneshot::channel();
        let monitor = tokio::spawn(async move {
            let result = match task.await {
                Ok(Ok(output)) => Ok(WorkResult { token, output }),
                Ok(Err(error)) => Err(error.into()),
                Err(_) => Err(RuntimeError::Panicked),
            };
            let retirement = result.as_ref().map(|_| ()).map_err(Clone::clone);
            let _ = sender.send(result);
            retirement
        });
        state.tasks.push(monitor);
        Ok(WorkHandle { receiver })
    }
    /// This owner-side check, after receipt, is authoritative. A worker-side check is only advisory.
    pub fn accept<T>(&mut self, result: WorkResult<T>) -> Result<RetainedOutput<T>, RuntimeError> {
        let state = locked(&self.inner.state);
        if !state.accepting
            || self.inner.cancel.is_cancelled()
            || result.token.operation != self.inner.operation
            || result.token.attempt != state.attempt
            || !Weak::ptr_eq(&result.token.owner, &Arc::downgrade(&self.inner))
        {
            return Err(RuntimeError::StaleResult);
        }
        Ok(result.output)
    }

    /// Runtime retirement evidence must survive cancellation of its owning caller.
    pub(super) fn accept_retirement<T>(
        &self,
        result: WorkResult<T>,
    ) -> Result<RetainedOutput<T>, RuntimeError> {
        self.accept_publication(result)
    }

    /// Once publication has been admitted, cancellation cannot erase a receipt describing
    /// durable effects. Only the owning engine driver can collect this terminal worker result.
    pub(super) fn accept_publication<T>(
        &self,
        result: WorkResult<T>,
    ) -> Result<RetainedOutput<T>, RuntimeError> {
        if result.token.operation != self.inner.operation
            || !Weak::ptr_eq(&result.token.owner, &Arc::downgrade(&self.inner))
        {
            return Err(RuntimeError::StaleResult);
        }
        Ok(result.output)
    }
}
struct WorkToken {
    operation: OperationId,
    attempt: AttemptId,
    owner: Weak<ScopeInner>,
}
pub struct WorkResult<T> {
    token: WorkToken,
    output: RetainedOutput<T>,
}
pub struct WorkHandle<T> {
    receiver: oneshot::Receiver<Result<WorkResult<T>, RuntimeError>>,
}
impl<T> WorkHandle<T> {
    pub async fn wait(self) -> Result<WorkResult<T>, RuntimeError> {
        self.receiver.await.map_err(|_| RuntimeError::Panicked)?
    }
}
/// Values retain their memory/scratch/file reservation after the worker's job slot retires.
pub struct RetainedOutput<T> {
    value: T,
    _permit: AdmissionPermit,
}
impl<T> Deref for RetainedOutput<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.value
    }
}
impl<T> RetainedOutput<T> {
    /// Release a conservative parsing allowance after the retained shape is known. This
    /// cannot acquire additional capacity or transfer a live worker's reservation.
    pub(super) fn shrink_resources(
        self,
        retained: ResourceRequest,
    ) -> Result<Self, AdmissionError> {
        let Self { value, mut _permit } = self;
        let smaller = _permit.split(retained)?;
        Ok(Self {
            value,
            _permit: smaller,
        })
    }
    pub(super) fn reserved(&self) -> ResourceRequest {
        self._permit.reserved()
    }

    /// Reassemble an owned value with an already retained reservation. The caller must keep
    /// the same charged resources alive; this never creates or increases admission.
    pub(super) fn from_parts(value: T, permit: AdmissionPermit) -> Self {
        Self {
            value,
            _permit: permit,
        }
    }

    pub(super) fn into_parts(self) -> (T, AdmissionPermit) {
        (self.value, self._permit)
    }

    pub fn map<U>(self, convert: impl FnOnce(T) -> U) -> RetainedOutput<U> {
        RetainedOutput {
            value: convert(self.value),
            _permit: self._permit,
        }
    }
}

#[cfg(test)]
mod tests;

impl<T, E> RetainedOutput<Result<T, E>> {
    pub fn transpose(self) -> Result<RetainedOutput<T>, E> {
        Ok(RetainedOutput {
            value: self.value?,
            _permit: self._permit,
        })
    }
}
