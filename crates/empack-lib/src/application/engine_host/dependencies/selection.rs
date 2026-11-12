//! A deliberate version choice crosses back to the host; no execution authority does.
use super::*;
use crate::engine::providers::{UpdateVersionChoices, UpdateVersionSelector};
use empack_core::identity::PinSelector;
use tokio::sync::{mpsc, oneshot};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::application::engine_host) enum UpdateSelection {
    #[default]
    Automatic,
    Interactive,
}
pub(in crate::application::engine_host) struct Question {
    choices: UpdateVersionChoices,
    reply: oneshot::Sender<Result<PinSelector>>,
}
impl Question {
    pub(in crate::application::engine_host) fn answer(self, session: &dyn Session) {
        let answer = self.choose(session);
        let _ = self.reply.send(answer);
    }
    fn choose(&self, session: &dyn Session) -> Result<PinSelector> {
        session.process().check_cancelled()?;
        ensure!(
            session.interactive().can_choose() && !session.config().app_config().yes,
            crate::application::cli::CommandInputRequired(
                "Version selection requires an interactive terminal; use --pin KEY=VERSION_ID without --select"
            )
        );
        let labels: Vec<_> = self
            .choices
            .versions
            .iter()
            .map(|version| {
                use crate::engine::providers::ReleaseChannel;
                let pin = crate::application::labels::pin(&version.pin);
                format!(
                    "{} ({}) [{}]{}",
                    crate::terminal::display_line(version.version.as_deref().unwrap_or(&pin)),
                    match version.channel {
                        ReleaseChannel::Release => "release",
                        ReleaseChannel::Beta => "beta",
                        ReleaseChannel::Alpha => "alpha",
                    },
                    crate::application::labels::pin(&version.pin),
                    if version.pin == self.choices.current {
                        " — installed"
                    } else {
                        ""
                    }
                )
            })
            .collect();
        session.display().status().info("Selecting a version pins this dependency; review the proposed changes before applying.");
        let index = session
            .interactive()
            .fuzzy_select(
                &format!(
                    "Choose a version for {}",
                    crate::terminal::display_line(self.choices.key.as_str())
                ),
                &labels,
            )?
            .ok_or(crate::application::process_runtime::Interrupted)?;
        self.choices
            .versions
            .get(index)
            .map(|value| value.pin.selection.clone())
            .context("Version selection is out of range")
    }
}
pub(super) struct SelectionBridge(mpsc::Sender<Question>);
pub(super) fn channel() -> (SelectionBridge, mpsc::Receiver<Question>) {
    let (sender, receiver) = mpsc::channel(1);
    (SelectionBridge(sender), receiver)
}
impl UpdateVersionSelector for SelectionBridge {
    fn choose<'a>(
        &'a mut self,
        choices: &'a UpdateVersionChoices,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<PinSelector>> + Send + 'a>> {
        Box::pin(async move {
            let (reply, receive) = oneshot::channel();
            self.0
                .send(Question {
                    choices: choices.clone(),
                    reply,
                })
                .await
                .context("Version selection host is unavailable")?;
            receive
                .await
                .context("Version selection response was lost")?
        })
    }
}

pub(super) fn repeat_command(
    project: &Path,
    requested: &[String],
    pins: &BTreeMap<empack_core::model::DependencyKey, PinSelector>,
) -> Option<String> {
    if pins.is_empty() {
        return None;
    }
    let mut arguments = vec![
        "--workdir".into(),
        project.to_str()?.into(),
        "update".into(),
    ];
    arguments.extend(requested.iter().cloned());
    for (key, pin) in pins {
        let version = match pin {
            PinSelector::ModrinthVersion(id) => id.to_string(),
            PinSelector::CurseForgeFile(id) => id.to_string(),
        };
        crate::application::shell::association(key.as_str(), &version)?;
        arguments.extend(["--pin".into(), format!("{}={version}", key.as_str())]);
    }
    crate::application::shell::arguments(&arguments).map(|args| format!("empack {args}"))
}
