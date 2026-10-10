//! Admission reservations belong to actual work or explicitly retained outputs.
use crate::application::process_runtime::Cancellation;
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

/// Scheduling estimates. Byte and file limits inside adapters remain independently enforced.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResourceRequest {
    pub jobs: u64,
    pub memory_bytes: u64,
    pub scratch_bytes: u64,
    pub open_files: u64,
}
impl ResourceRequest {
    fn values(self) -> [u64; 4] {
        [
            self.jobs,
            self.memory_bytes,
            self.scratch_bytes,
            self.open_files,
        ]
    }
    fn from_values(value: [u64; 4]) -> Self {
        Self {
            jobs: value[0],
            memory_bytes: value[1],
            scratch_bytes: value[2],
            open_files: value[3],
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    Jobs,
    MemoryBytes,
    ScratchBytes,
    OpenFiles,
}
const KINDS: [ResourceKind; 4] = [
    ResourceKind::Jobs,
    ResourceKind::MemoryBytes,
    ResourceKind::ScratchBytes,
    ResourceKind::OpenFiles,
];
impl std::fmt::Display for ResourceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Jobs => "worker slots",
            Self::MemoryBytes => "estimated memory",
            Self::ScratchBytes => "temporary storage",
            Self::OpenFiles => "file reservations",
        })
    }
}
impl ResourceKind {
    pub(super) fn quantity(self, value: u64) -> String {
        match self {
            Self::MemoryBytes | Self::ScratchBytes if value >= 1 << 20 => {
                format!("{:.1} MiB ({value} bytes)", value as f64 / (1 << 20) as f64)
            }
            Self::MemoryBytes | Self::ScratchBytes => format!("{value} bytes"),
            Self::Jobs | Self::OpenFiles => value.to_string(),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdmissionError {
    Closed,
    Cancelled,
    TooLarge {
        resource: ResourceKind,
        requested: u64,
        maximum: u64,
    },
    Busy {
        resource: ResourceKind,
        requested: u64,
        available: u64,
    },
    InvalidTransfer,
}
impl std::error::Error for AdmissionError {}
impl std::fmt::Display for AdmissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::Closed => f.write_str("Operation admission is closed"),
            Self::Cancelled => f.write_str("Operation admission was cancelled"),
            Self::InvalidTransfer => {
                f.write_str("Cannot transfer more resources than the permit owns")
            }
            Self::TooLarge {
                resource,
                requested,
                maximum,
            } => {
                write!(
                    f,
                    "Resource budget exceeded for {resource}: requested {}; configured limit {}",
                    resource.quantity(requested),
                    resource.quantity(maximum)
                )?;
                if resource == ResourceKind::MemoryBytes {
                    f.write_str(". This is a work estimate, not measured RAM usage")?;
                }
                Ok(())
            }
            Self::Busy {
                resource,
                requested,
                available,
            } => {
                write!(
                    f,
                    "Resource budget unavailable for {resource}: requested {}; currently available {}",
                    resource.quantity(requested),
                    resource.quantity(available)
                )
            }
        }
    }
}
#[derive(Debug, Clone, Copy)]
pub struct ResourceStatus {
    pub limits: ResourceRequest,
    pub reserved: ResourceRequest,
    pub closed: bool,
}
struct State {
    limits: [u64; 4],
    reserved: [u64; 4],
    closed: bool,
}
struct Inner {
    state: Mutex<State>,
    changed: Notify,
}
/// One coordinated capacity ledger shared by admitted work.
#[derive(Clone)]
pub struct ResourceGovernor(Arc<Inner>);
impl ResourceGovernor {
    pub fn new(limits: ResourceRequest) -> Self {
        Self(Arc::new(Inner {
            state: Mutex::new(State {
                limits: limits.values(),
                reserved: [0; 4],
                closed: false,
            }),
            changed: Notify::new(),
        }))
    }
    pub fn status(&self) -> ResourceStatus {
        let state = self
            .0
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        ResourceStatus {
            limits: ResourceRequest::from_values(state.limits),
            reserved: ResourceRequest::from_values(state.reserved),
            closed: state.closed,
        }
    }
    /// Check and reserve all dimensions atomically. Oversized work never waits for impossible capacity.
    pub fn try_admit(&self, request: ResourceRequest) -> Result<AdmissionPermit, AdmissionError> {
        let requested = request.values();
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if state.closed {
            return Err(AdmissionError::Closed);
        }
        for (index, value) in requested.iter().enumerate() {
            if *value > state.limits[index] {
                return Err(AdmissionError::TooLarge {
                    resource: KINDS[index],
                    requested: *value,
                    maximum: state.limits[index],
                });
            }
        }
        for (index, value) in requested.iter().enumerate() {
            let available = state.limits[index] - state.reserved[index];
            if *value > available {
                return Err(AdmissionError::Busy {
                    resource: KINDS[index],
                    requested: *value,
                    available,
                });
            }
        }
        for (index, value) in requested.iter().enumerate() {
            state.reserved[index] += value;
        }
        Ok(AdmissionPermit {
            owner: self.0.clone(),
            held: requested,
        })
    }
    /// Wait on the host runtime. Cancellation does not affect already admitted permits.
    pub async fn admit(
        &self,
        request: ResourceRequest,
        cancel: &Cancellation,
    ) -> Result<AdmissionPermit, AdmissionError> {
        loop {
            let changed = self.0.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if cancel.is_cancelled() {
                return Err(AdmissionError::Cancelled);
            }
            match self.try_admit(request) {
                Err(AdmissionError::Busy { .. }) => {}
                result => return result,
            }
            tokio::select! {
                _ = changed => {},
                _ = cancel.cancelled() => return Err(AdmissionError::Cancelled),
            }
        }
    }
    /// Close new admission before cancelling and retiring existing work.
    pub fn close(&self) {
        {
            self.0
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .closed = true;
        }
        self.0.changed.notify_waiters();
    }
}

/// Unique reservation ownership. Dropping a caller handle cannot release a worker-owned permit.
pub struct AdmissionPermit {
    owner: Arc<Inner>,
    held: [u64; 4],
}
impl AdmissionPermit {
    pub fn reserved(&self) -> ResourceRequest {
        ResourceRequest::from_values(self.held)
    }
    /// Transfer part of a reservation to a retained output without changing global accounting.
    pub fn split(&mut self, retained: ResourceRequest) -> Result<Self, AdmissionError> {
        let values = retained.values();
        if values
            .iter()
            .zip(self.held)
            .any(|(requested, held)| *requested > held)
        {
            return Err(AdmissionError::InvalidTransfer);
        }
        for (held, transfer) in self.held.iter_mut().zip(values) {
            *held -= transfer;
        }
        Ok(Self {
            owner: self.owner.clone(),
            held: values,
        })
    }
}
impl Drop for AdmissionPermit {
    fn drop(&mut self) {
        {
            let mut state = self
                .owner
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            for (reserved, held) in state.reserved.iter_mut().zip(self.held) {
                *reserved -= held;
            }
        }
        self.owner.changed.notify_waiters();
    }
}

#[cfg(test)]
mod tests;
