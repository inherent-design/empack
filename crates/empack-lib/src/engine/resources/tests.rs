use super::*;
use std::time::Duration;
fn limits() -> ResourceRequest {
    ResourceRequest {
        jobs: 2,
        memory_bytes: 100,
        scratch_bytes: 200,
        open_files: 5,
    }
}
#[test]
fn multidimensional_admission_is_atomic_and_checked_without_overflow() {
    let governor = ResourceGovernor::new(limits());
    let permit = governor
        .try_admit(ResourceRequest {
            jobs: 1,
            memory_bytes: 80,
            scratch_bytes: 160,
            open_files: 4,
        })
        .unwrap();
    assert!(matches!(
        governor.try_admit(ResourceRequest {
            jobs: 1,
            memory_bytes: 30,
            ..ResourceRequest::default()
        }),
        Err(AdmissionError::Busy {
            resource: ResourceKind::MemoryBytes,
            requested: 30,
            available: 20,
        })
    ));
    assert_eq!(governor.status().reserved, permit.reserved());
    // Each later dimension can refuse a mixed request without charging any of
    // its otherwise available worker/memory/storage/file capacity.
    for (request, expected) in [
        (
            ResourceRequest {
                jobs: 1,
                memory_bytes: 1,
                scratch_bytes: 41,
                open_files: 1,
            },
            AdmissionError::Busy {
                resource: ResourceKind::ScratchBytes,
                requested: 41,
                available: 40,
            },
        ),
        (
            ResourceRequest {
                jobs: 1,
                memory_bytes: 1,
                scratch_bytes: 1,
                open_files: 2,
            },
            AdmissionError::Busy {
                resource: ResourceKind::OpenFiles,
                requested: 2,
                available: 1,
            },
        ),
        (
            ResourceRequest {
                jobs: 2,
                ..Default::default()
            },
            AdmissionError::Busy {
                resource: ResourceKind::Jobs,
                requested: 2,
                available: 1,
            },
        ),
    ] {
        assert_eq!(governor.try_admit(request).err(), Some(expected));
        assert_eq!(governor.status().reserved, permit.reserved());
    }
    for (request, expected) in [
        (
            ResourceRequest {
                memory_bytes: 101,
                ..Default::default()
            },
            AdmissionError::TooLarge {
                resource: ResourceKind::MemoryBytes,
                requested: 101,
                maximum: 100,
            },
        ),
        (
            ResourceRequest {
                scratch_bytes: 201,
                ..Default::default()
            },
            AdmissionError::TooLarge {
                resource: ResourceKind::ScratchBytes,
                requested: 201,
                maximum: 200,
            },
        ),
        (
            ResourceRequest {
                open_files: 6,
                ..Default::default()
            },
            AdmissionError::TooLarge {
                resource: ResourceKind::OpenFiles,
                requested: 6,
                maximum: 5,
            },
        ),
    ] {
        assert_eq!(governor.try_admit(request).err(), Some(expected));
        assert_eq!(governor.status().reserved, permit.reserved());
    }
    assert!(matches!(
        governor.try_admit(ResourceRequest {
            jobs: u64::MAX,
            ..ResourceRequest::default()
        }),
        Err(AdmissionError::TooLarge {
            resource: ResourceKind::Jobs,
            requested: u64::MAX,
            maximum: 2,
        })
    ));
    drop(permit);
    assert_eq!(governor.status().reserved, ResourceRequest::default());
    let maximum = ResourceGovernor::new(ResourceRequest {
        memory_bytes: u64::MAX,
        ..ResourceRequest::default()
    });
    let _permit = maximum
        .try_admit(ResourceRequest {
            memory_bytes: u64::MAX,
            ..ResourceRequest::default()
        })
        .unwrap();
    assert!(matches!(
        maximum.try_admit(ResourceRequest {
            memory_bytes: 1,
            ..ResourceRequest::default()
        }),
        Err(AdmissionError::Busy {
            resource: ResourceKind::MemoryBytes,
            requested: 1,
            available: 0,
        })
    ));
}
#[test]
fn retained_outputs_keep_their_share_until_they_retire() {
    let governor = ResourceGovernor::new(limits());
    let mut permit = governor.try_admit(limits()).unwrap();
    let retained = permit
        .split(ResourceRequest {
            scratch_bytes: 100,
            open_files: 1,
            ..ResourceRequest::default()
        })
        .unwrap();
    assert_eq!(governor.status().reserved, limits());
    let before = permit.reserved();
    assert!(permit.split(limits()).is_err());
    assert_eq!(permit.reserved(), before);
    drop(permit);
    assert_eq!(governor.status().reserved, retained.reserved());
    governor.close();
    assert!(matches!(
        governor.try_admit(ResourceRequest::default()),
        Err(AdmissionError::Closed)
    ));
    drop(retained);
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}
#[tokio::test]
async fn cancellation_does_not_release_a_running_blocking_worker_reservation() {
    let governor = ResourceGovernor::new(limits());
    let permit = governor.try_admit(limits()).unwrap();
    let cancel = Cancellation::default();
    let (release, wait) = std::sync::mpsc::channel();
    let worker = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        wait.recv().unwrap();
    });
    cancel.cancel();
    assert_eq!(governor.status().reserved, limits());
    assert!(matches!(
        governor.admit(limits(), &cancel).await,
        Err(AdmissionError::Cancelled)
    ));
    assert_eq!(governor.status().reserved, limits());
    release.send(()).unwrap();
    worker.await.unwrap();
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}
#[tokio::test]
async fn waiters_wake_on_retirement_and_scope_close() {
    let governor = ResourceGovernor::new(limits());
    let permit = governor.try_admit(limits()).unwrap();
    let waiting = governor.clone();
    let task = tokio::spawn(async move { waiting.admit(limits(), &Cancellation::default()).await });
    tokio::task::yield_now().await;
    drop(permit);
    let permit = tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let waiting = governor.clone();
    let task = tokio::spawn(async move { waiting.admit(limits(), &Cancellation::default()).await });
    tokio::task::yield_now().await;
    governor.close();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap(),
        Err(AdmissionError::Closed)
    ));
    assert_eq!(governor.status().reserved, limits());
    drop(permit);
}
#[test]
fn concurrent_close_and_admission_never_admit_after_closed_state() {
    for _ in 0..64 {
        let governor = ResourceGovernor::new(limits());
        let other = governor.clone();
        let worker = std::thread::spawn(move || other.try_admit(limits()));
        governor.close();
        let result = worker.join().unwrap();
        assert!(matches!(
            governor.try_admit(ResourceRequest::default()),
            Err(AdmissionError::Closed)
        ));
        drop(result);
        assert_eq!(governor.status().reserved, ResourceRequest::default());
    }
}

#[test]
fn admission_failures_report_requested_capacity_without_charging_it() {
    let governor = ResourceGovernor::new(ResourceRequest {
        memory_bytes: 512 << 20,
        open_files: 16,
        ..Default::default()
    });
    let error = governor
        .try_admit(ResourceRequest {
            memory_bytes: 1152 << 20,
            ..Default::default()
        })
        .err()
        .unwrap();
    assert!(matches!(error, AdmissionError::TooLarge {
        resource: ResourceKind::MemoryBytes,
        requested,
        maximum,
    } if requested == 1152 << 20 && maximum == 512 << 20));
    let message = error.to_string();
    assert!(message.contains("1152.0 MiB"));
    assert!(message.contains("512.0 MiB"));
    assert!(message.contains("not measured RAM usage"));
    assert_eq!(governor.status().reserved, ResourceRequest::default());

    let held = governor
        .try_admit(ResourceRequest {
            open_files: 12,
            ..Default::default()
        })
        .unwrap();
    let error = governor
        .try_admit(ResourceRequest {
            open_files: 8,
            ..Default::default()
        })
        .err()
        .unwrap();
    assert!(matches!(
        error,
        AdmissionError::Busy {
            resource: ResourceKind::OpenFiles,
            requested: 8,
            available: 4,
        }
    ));
    assert!(error.to_string().contains("file reservations"));
    assert_eq!(governor.status().reserved, held.reserved());
}
