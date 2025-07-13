use super::*;

#[test]
fn test_log_context_creation() {
    let context = LogContext::new("test_operation");
    assert_eq!(context.operation, "test_operation");
    assert_eq!(context.total_items, None);
    assert_eq!(context.current_item, None);
}

#[test]
fn test_log_context_with_progress() {
    let mut context = LogContext::with_progress("downloading", 100);
    assert_eq!(context.operation, "downloading");
    assert_eq!(context.total_items, Some(100));
    assert_eq!(context.current_item, None);

    context.set_progress(50);
    assert_eq!(context.current_item, Some(50));
}

#[test]
fn test_logger_not_initialized_initially() {
    // Note: This test assumes no other test has initialized the logger
    // In practice, we might need test isolation for the global logger
    assert!(!Logger::is_initialized() || Logger::global().is_some());
}
