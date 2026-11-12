//! Bounded stderr feedback for actual operation observations; never a completion receipt.
use super::*;
use crate::engine::runtime::WorkProgress;

pub(super) struct ProgressDisplay {
    provider: Box<dyn crate::display::ProgressProvider>,
    spinner: Option<Box<dyn crate::display::ProgressTracker>>,
    quiet: bool,
    terminal: bool,
    last: Option<std::time::Instant>,
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
            observation: None,
        }
    }
    pub(super) fn acquisition(&mut self, observation: Option<WorkProgress>) {
        let Some(observation) = observation else {
            return;
        };
        if self.observation.as_ref() == Some(&observation) {
            return;
        }
        let message = match &observation {
            WorkProgress::CheckingContentCache => "Checking exact content cache".to_owned(),
            WorkProgress::Downloading {
                received_bytes,
                total_bytes,
            } => match total_bytes {
                Some(total) => {
                    format!("Downloading content: {received_bytes}/{total} bytes received")
                }
                None => format!("Downloading content: {received_bytes} bytes received"),
            },
            WorkProgress::VerifyingContent => "Verifying downloaded content".to_owned(),
        };
        if self.update(&message) {
            self.observation = Some(observation);
        }
    }
    pub(super) fn update(&mut self, message: &str) -> bool {
        if self.quiet {
            return false;
        }
        let now = std::time::Instant::now();
        if self
            .last
            .is_some_and(|last| now.duration_since(last) < Duration::from_millis(500))
        {
            return false;
        }
        if self.spinner.is_none() && self.terminal {
            self.spinner = Some(self.provider.spinner(message));
        }
        if let Some(spinner) = &self.spinner {
            spinner.set_message(message);
        } else {
            self.provider.message(message);
        }
        self.last = Some(now);
        true
    }
}
impl Drop for ProgressDisplay {
    fn drop(&mut self) {
        if let Some(spinner) = &self.spinner {
            spinner.finish_clear();
        }
    }
}
