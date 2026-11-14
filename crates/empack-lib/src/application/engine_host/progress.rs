//! Bounded stderr feedback for actual operation observations; never a completion receipt.
use super::*;
use crate::engine::runtime::WorkProgress;

pub(super) struct ProgressDisplay {
    provider: Box<dyn crate::display::ProgressProvider>,
    spinner: Option<Box<dyn crate::display::ProgressTracker>>,
    quiet: bool,
    terminal: bool,
    last: Option<std::time::Instant>,
    phase: Option<&'static str>,
    plain_phases: std::collections::BTreeSet<&'static str>,
    pending: Option<String>,
    observation: Option<WorkProgress>,
}
impl ProgressDisplay {
    pub(super) fn new(session: &dyn Session, quiet: bool) -> Self {
        Self {
            provider: session.display().progress(),
            spinner: None,
            quiet,
            terminal: session.terminal().is_tty,
            last: None,
            phase: None,
            plain_phases: Default::default(),
            pending: None,
            observation: None,
        }
    }
    pub(super) fn pause(&mut self) {
        if let Some(spinner) = self.spinner.take() {
            spinner.finish_clear();
        }
        self.pending = None;
        self.observation = None;
        self.phase = None;
        self.last = None;
    }
    pub(super) fn observe_work(&mut self, observation: Option<WorkProgress>) {
        let Some(observation) = observation else {
            return;
        };
        if self.observation.as_ref() == Some(&observation) {
            self.flush();
            return;
        }
        let (phase, message) = match &observation {
            WorkProgress::ReadingLocalContent => {
                ("local-content", "Reading local content".to_owned())
            }
            WorkProgress::SavingCachedContent => {
                ("save-cache", "Saving verified content in cache".to_owned())
            }
            WorkProgress::PreparingServerRuntime => {
                ("server-runtime", "Preparing server runtime".to_owned())
            }
            WorkProgress::RunningServerInstaller => {
                ("server-installer", "Running server installer".to_owned())
            }
            WorkProgress::BuildingArchives => {
                ("archives", "Creating and verifying archives".to_owned())
            }
            WorkProgress::StagingInstanceFiles => {
                ("instance-files", "Preparing instance files".to_owned())
            }
            WorkProgress::PublishingChanges => {
                ("publication", "Saving verified changes".to_owned())
            }
            WorkProgress::RecoveringChanges => {
                ("recovery", "Recovering interrupted changes".to_owned())
            }
            WorkProgress::CleaningCache => {
                ("clean-cache", "Removing selected cached files".to_owned())
            }
            WorkProgress::CheckingProjectFile { path, index, total } => (
                "project-files",
                if *total == 0 {
                    format!("Checking {path}")
                } else {
                    format!("Checking file {index} of {total}: {path}")
                },
            ),
            WorkProgress::CheckingJava => ("java", "Checking selected Java".to_owned()),
            WorkProgress::CheckingContentCache => ("cache", "Checking cached content".to_owned()),
            WorkProgress::Downloading {
                received_bytes,
                total_bytes,
            } => (
                "download",
                match total_bytes {
                    Some(total) => {
                        format!("Downloading content: {received_bytes}/{total} bytes received")
                    }
                    None => format!("Downloading content: {received_bytes} bytes received"),
                },
            ),
            WorkProgress::VerifyingContent => ("verification", "Verifying content".to_owned()),
            WorkProgress::CheckingDependency {
                key,
                index,
                total,
                versions,
            } => (
                "catalog",
                format!(
                    "Checking dependency {index} of {total}: {key}{}",
                    if *versions == 0 {
                        String::new()
                    } else {
                        format!(" ({})", counted(*versions, "version read", "versions read"))
                    }
                ),
            ),
            WorkProgress::CheckingRequirements { dependency } => (
                "requirements",
                dependency.as_ref().map_or_else(
                    || "Checking dependency requirements".to_owned(),
                    |key| format!("Checking dependency requirements: {key}"),
                ),
            ),
            WorkProgress::RevalidatingInputs => {
                ("input-verification", "Rechecking project inputs".to_owned())
            }
            WorkProgress::CheckingConflicts => {
                ("conflicts", "Checking reported conflicts".to_owned())
            }
        };
        self.update(phase, &message);
        self.observation = Some(observation);
    }
    pub(super) fn update(&mut self, phase: &'static str, message: &str) {
        self.update_at(phase, message, std::time::Instant::now());
    }
    fn update_at(&mut self, phase: &'static str, message: &str, now: std::time::Instant) {
        if self.quiet {
            return;
        }
        let changed_phase = self.phase != Some(phase);
        self.phase = Some(phase);
        // Plain logs record each phase once. Acquisition cycles cache/download/verify
        // for each file; a phase transition alone must not turn those into log spam.
        if !self.terminal && !self.plain_phases.insert(phase) {
            return;
        }
        self.pending = Some(message.to_owned());
        if changed_phase {
            self.draw(now);
        } else {
            self.flush_at(now);
        }
    }
    pub(super) fn flush(&mut self) {
        self.flush_at(std::time::Instant::now());
    }
    fn flush_at(&mut self, now: std::time::Instant) {
        if self.pending.is_none()
            || self
                .last
                .is_some_and(|last| now.duration_since(last) < Duration::from_millis(500))
        {
            return;
        }
        self.draw(now);
    }
    fn draw(&mut self, now: std::time::Instant) {
        let Some(message) = self.pending.take() else {
            return;
        };
        if self.spinner.is_none() && self.terminal {
            self.spinner = Some(self.provider.spinner(&message));
        }
        if let Some(spinner) = &self.spinner {
            spinner.set_message(&message);
        } else {
            self.provider.message(&message);
        }
        self.last = Some(now);
    }
}
impl Drop for ProgressDisplay {
    fn drop(&mut self) {
        if let Some(spinner) = &self.spinner {
            spinner.finish_clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::{MultiProgressProvider, ProgressProvider, ProgressTracker};
    use std::sync::{Arc, Mutex};

    struct Recorded(Arc<Mutex<Vec<String>>>);
    impl ProgressProvider for Recorded {
        fn message(&self, message: &str) {
            self.0.lock().unwrap().push(message.into());
        }
        fn spinner(&self, message: &str) -> Box<dyn ProgressTracker> {
            self.message(message);
            Box::new(Recorded(self.0.clone()))
        }
        fn bar(&self, _: u64) -> Box<dyn ProgressTracker> {
            unreachable!()
        }
        fn multi(&self) -> Box<dyn MultiProgressProvider> {
            unreachable!()
        }
    }
    impl ProgressTracker for Recorded {
        fn set_position(&self, _: u64) {
            unreachable!()
        }
        fn inc(&self) {
            unreachable!()
        }
        fn inc_by(&self, _: u64) {
            unreachable!()
        }
        fn set_message(&self, message: &str) {
            self.message(message);
        }
        fn tick(&self, _: &str) {
            unreachable!()
        }
        fn finish(&self, _: &str) {
            unreachable!()
        }
        fn abandon(&self, _: &str) {
            unreachable!()
        }
        fn finish_clear(&self) {
            self.message("<clear>");
        }
    }
    fn display(terminal: bool, quiet: bool) -> (ProgressDisplay, Arc<Mutex<Vec<String>>>) {
        let session = crate::application::session_mocks::MockCommandSession::new();
        let messages = Arc::new(Mutex::new(Vec::new()));
        let mut display = ProgressDisplay::new(&session, quiet);
        display.provider = Box::new(Recorded(messages.clone()));
        display.terminal = terminal;
        (display, messages)
    }
    #[test]
    fn progress_preserves_latest_update_and_immediate_phase_changes() {
        let (mut display, messages) = display(true, false);
        let start = std::time::Instant::now();
        display.update_at("catalog", "Reading a", start);
        display.update_at("catalog", "Reading b", start + Duration::from_millis(100));
        display.update_at("catalog", "Reading c", start + Duration::from_millis(200));
        assert!(
            !messages
                .lock()
                .unwrap()
                .iter()
                .any(|line| line == "Reading b")
        );
        display.flush_at(start + Duration::from_millis(500));
        assert_eq!(messages.lock().unwrap().last().unwrap(), "Reading c");
        display.update_at(
            "requirements",
            "Checking requirements",
            start + Duration::from_millis(501),
        );
        assert_eq!(
            messages.lock().unwrap().last().unwrap(),
            "Checking requirements"
        );
        display.update_at(
            "requirements",
            "Checking library",
            start + Duration::from_millis(502),
        );
        display.update_at(
            "conflicts",
            "Checking conflicts",
            start + Duration::from_millis(503),
        );
        display.flush_at(start + Duration::from_secs(1));
        assert_eq!(
            messages.lock().unwrap().last().unwrap(),
            "Checking conflicts"
        );
        display.pause();
        assert_eq!(messages.lock().unwrap().last().unwrap(), "<clear>");
        display.update_at(
            "catalog",
            "Reading after the choice",
            start + Duration::from_secs(2),
        );
        assert_eq!(
            messages.lock().unwrap().last().unwrap(),
            "Reading after the choice"
        );
        drop(display);
        assert_eq!(messages.lock().unwrap().last().unwrap(), "<clear>");
        let (mut shared, recorded) = self::display(true, false);
        for (observation, expected) in [
            (WorkProgress::CheckingJava, "Checking selected Java"),
            (
                WorkProgress::CheckingContentCache,
                "Checking cached content",
            ),
            (WorkProgress::ReadingLocalContent, "Reading local content"),
            (
                WorkProgress::SavingCachedContent,
                "Saving verified content in cache",
            ),
            (
                WorkProgress::PreparingServerRuntime,
                "Preparing server runtime",
            ),
            (
                WorkProgress::RunningServerInstaller,
                "Running server installer",
            ),
            (
                WorkProgress::BuildingArchives,
                "Creating and verifying archives",
            ),
            (
                WorkProgress::StagingInstanceFiles,
                "Preparing instance files",
            ),
            (WorkProgress::PublishingChanges, "Saving verified changes"),
            (
                WorkProgress::RecoveringChanges,
                "Recovering interrupted changes",
            ),
            (
                WorkProgress::CleaningCache,
                "Removing selected cached files",
            ),
            (WorkProgress::VerifyingContent, "Verifying content"),
            (
                WorkProgress::CheckingDependency {
                    key: "example".into(),
                    index: 2,
                    total: 3,
                    versions: 12,
                },
                "Checking dependency 2 of 3: example (12 versions read)",
            ),
            (
                WorkProgress::CheckingRequirements {
                    dependency: Some("library".into()),
                },
                "Checking dependency requirements: library",
            ),
            (
                WorkProgress::CheckingConflicts,
                "Checking reported conflicts",
            ),
            (
                WorkProgress::RevalidatingInputs,
                "Rechecking project inputs",
            ),
        ] {
            shared.observe_work(Some(observation));
            assert_eq!(recorded.lock().unwrap().last().unwrap(), expected);
        }
    }
    #[test]
    fn progress_plain_output_is_phase_only_and_quiet_has_no_progress() {
        for quiet in [false, true] {
            let (mut display, messages) = display(false, quiet);
            let start = std::time::Instant::now();
            display.update_at("catalog", "Reading versions", start);
            display.update_at(
                "catalog",
                "Reading another dependency",
                start + Duration::from_secs(1),
            );
            display.update_at(
                "requirements",
                "Checking requirements",
                start + Duration::from_millis(1001),
            );
            display.flush_at(start + Duration::from_secs(2));
            for index in 0..100 {
                display.update_at(
                    "catalog",
                    "Reading another dependency",
                    start + Duration::from_secs(3 + index * 2),
                );
                display.update_at(
                    "requirements",
                    "Checking another requirement",
                    start + Duration::from_secs(4 + index * 2),
                );
            }
            drop(display);
            let actual = messages.lock().unwrap();
            let expected: &[&str] = if quiet {
                &[]
            } else {
                &["Reading versions", "Checking requirements"]
            };
            assert_eq!(*actual, expected);
        }
    }
}
