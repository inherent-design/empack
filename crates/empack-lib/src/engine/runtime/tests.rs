use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
fn request() -> ResourceRequest {
    ResourceRequest {
        jobs: 1,
        memory_bytes: 8,
        scratch_bytes: 4,
        open_files: 1,
    }
}
fn retained() -> ResourceRequest {
    ResourceRequest {
        jobs: 0,
        ..request()
    }
}
fn runtime<T: Send + Sync + 'static>() -> (OperationRuntime<T>, ResourceGovernor) {
    let governor = ResourceGovernor::new(request());
    (OperationRuntime::new(governor.clone(), 4), governor)
}
#[tokio::test]
async fn missed_notifications_retain_terminal_results_and_explicit_eviction() {
    let (runtime, _) = runtime::<u8>();
    let mut first = runtime.start(|_| async { Ok(42) }).unwrap();
    let id = first.id();
    let outcome = first.wait().await;
    assert!(matches!(&*outcome, OperationOutcome::Completed(42)));
    drop(first);
    let mut observer = runtime.observe(id).unwrap();
    assert!(Arc::ptr_eq(&outcome, &observer.wait().await));
    assert!(runtime.release_completed(id));
    assert!(runtime.observe(id).is_none());
    assert!(matches!(
        &*observer.wait().await,
        OperationOutcome::Completed(42)
    ));
    runtime.shutdown().await;
    assert!(matches!(
        runtime.start(|_| async { Ok(1) }),
        Err(RuntimeError::Closed)
    ));
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropped_handle_cannot_release_running_blocking_work_or_skip_retirement() {
    let (runtime, governor) = runtime::<()>();
    let released = Arc::new(AtomicBool::new(false));
    let worker_release = released.clone();
    let (started, ready) = oneshot::channel();
    let handle = runtime
        .start(move |scope| async move {
            let worker =
                scope.spawn_blocking(request(), ResourceRequest::default(), move |_| {
                    let _ = started.send(());
                    while !worker_release.load(Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                })?;
            drop(worker); // Dropping a result receiver must not detach worker ownership.
            Ok(())
        })
        .unwrap();
    let id = handle.id();
    ready.await.unwrap();
    drop(handle);
    assert_eq!(governor.status().reserved, request());
    assert!(!runtime.release_completed(id));
    let shutdown = runtime.shutdown();
    tokio::pin!(shutdown);
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut shutdown)
            .await
            .is_err()
    );
    assert_eq!(governor.status().reserved, request());
    released.store(true, Ordering::SeqCst);
    shutdown.await;
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}
#[tokio::test]
async fn stale_results_are_rejected_and_retained_outputs_stay_charged() {
    let (runtime, governor) = runtime::<()>();
    let observed = governor.clone();
    let mut handle = runtime
        .start(move |mut scope| async move {
            let worker = scope.spawn(request(), retained(), |_| async { vec![1u8; 8] })?;
            let result = worker.wait().await?;
            assert_eq!(observed.status().reserved, retained());
            scope.next_attempt()?;
            assert!(matches!(
                scope.accept(result),
                Err(RuntimeError::StaleResult)
            ));
            assert_eq!(observed.status().reserved, ResourceRequest::default());
            let worker = scope.spawn(request(), retained(), |_| async { 42 })?;
            let output = scope.accept(worker.wait().await?)?;
            assert_eq!(*output, 42);
            assert_eq!(observed.status().reserved, retained());
            scope.retire().await?;
            assert_eq!(observed.status().reserved, retained());
            drop(output);
            Ok(())
        })
        .unwrap();
    assert!(matches!(
        &*handle.wait().await,
        OperationOutcome::Completed(())
    ));
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}
#[tokio::test]
async fn failed_replacement_admission_does_not_invalidate_usable_attempt() {
    let (runtime, _) = runtime::<()>();
    let mut handle = runtime
        .start(|mut scope| async move {
            let worker = scope.spawn(request(), retained(), |_| async { 7 })?;
            let result = worker.wait().await?;
            assert!(matches!(
                scope.spawn(request(), retained(), |_| async { 8 }),
                Err(RuntimeError::Admission(AdmissionError::Busy { .. }))
            ));
            assert_eq!(*scope.accept(result)?, 7);
            Ok(())
        })
        .unwrap();
    assert!(matches!(
        &*handle.wait().await,
        OperationOutcome::Completed(())
    ));
}
#[tokio::test]
async fn panicking_driver_still_retires_its_workers() {
    let (runtime, governor) = runtime::<()>();
    let mut handle = runtime
        .start(|scope| async move {
            scope.spawn(request(), ResourceRequest::default(), |cancel| async move {
                cancel.cancelled().await
            })?;
            panic!("fixture driver failure");
        })
        .unwrap();
    assert!(matches!(
        &*handle.wait().await,
        OperationOutcome::Failed(RuntimeError::Panicked)
    ));
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}
#[tokio::test]
async fn completed_registry_is_bounded_until_explicit_release() {
    let runtime = OperationRuntime::new(ResourceGovernor::new(request()), 1);
    let mut handle = runtime.start(|_| async { Ok(1) }).unwrap();
    handle.wait().await;
    assert!(matches!(
        runtime.start(|_| async { Ok(2) }),
        Err(RuntimeError::RegistryFull)
    ));
    assert!(runtime.release_completed(handle.id()));
    let mut second = runtime.start(|_| async { Ok(2) }).unwrap();
    assert!(matches!(
        &*second.wait().await,
        OperationOutcome::Completed(2)
    ));
}

#[tokio::test]
async fn unobserved_worker_panic_cannot_be_reported_as_completed() {
    let (runtime, governor) = runtime::<()>();
    let mut handle = runtime
        .start(|scope| async move {
            drop(
                scope.spawn(request(), ResourceRequest::default(), |_| async {
                    panic!("worker fixture")
                })?,
            );
            Ok(())
        })
        .unwrap();
    assert!(matches!(
        &*handle.wait().await,
        OperationOutcome::Failed(RuntimeError::Panicked)
    ));
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_serializes_with_admission_and_concurrent_shutdown_waits() {
    let (runtime, governor) = runtime::<()>();
    let runtime = Arc::new(runtime);
    let start_barrier = Arc::new(tokio::sync::Barrier::new(2));
    let admission_runtime = runtime.clone();
    let admission_barrier = start_barrier.clone();
    let submit = tokio::spawn(async move {
        admission_barrier.wait().await;
        admission_runtime.start(|scope| async move {
            let worker =
                scope.spawn(request(), ResourceRequest::default(), |cancel| async move {
                    cancel.cancelled().await
                })?;
            drop(worker);
            Ok(())
        })
    });
    start_barrier.wait().await;
    let ((), ()) = tokio::join!(runtime.shutdown(), runtime.shutdown());
    match submit.await.unwrap() {
        Ok(mut handle) => {
            handle.wait().await;
        }
        Err(RuntimeError::Closed) => {}
        Err(error) => panic!("unexpected submission result: {error}"),
    }
    assert_eq!(governor.status().reserved, ResourceRequest::default());
}

#[tokio::test]
async fn preparation_retirement_does_not_cancel_authorized_publication() {
    let (runtime, _) = runtime::<()>();
    let mut handle = runtime
        .start(|scope| async move {
            let operation_cancel = scope.cancellation();
            let worker =
                scope.spawn(request(), ResourceRequest::default(), |cancel| async move {
                    cancel.cancelled().await
                })?;
            drop(worker);
            scope.retire().await?;
            assert!(!operation_cancel.is_cancelled());
            Ok(())
        })
        .unwrap();
    assert!(matches!(
        &*handle.wait().await,
        OperationOutcome::Completed(())
    ));
}
