//! Present the shared Java policy without deriving pack requirements from this host.
use super::*;
use empack_core::java::JavaPolicy;

pub(super) fn choose(session: &dyn Session, game: &str, explicit: Option<u16>) -> Result<u16> {
    let policy = JavaPolicy::for_game(game);
    let status = session.display().status();
    if let Some(majors) = policy.supported() {
        status.info(&format!(
            "Java {} recommended for Minecraft {game}. Supported majors: {}. Mods and loaders may have stricter requirements.",
            policy.recommended().unwrap(),
            majors.iter().map(u16::to_string).collect::<Vec<_>>().join(", ")
        ));
    } else {
        status.warning(&format!(
            "Java requirements are not known for Minecraft {game}. Choose a version explicitly; compatibility still needs checking."
        ));
    }
    if explicit.is_some() {
        let selected = policy.select(explicit)?;
        status.info(&format!("Java {selected} selected for this pack"));
        return Ok(selected);
    }
    if policy.supported().is_some_and(|majors| majors.len() == 1) {
        return policy.select(None).map_err(Into::into);
    }
    let interactive = session.interactive();
    if !session.config().app_config().yes && interactive.can_choose() {
        if let Some(majors) = policy.supported() {
            let choices = majors.iter().rev().copied().collect::<Vec<_>>();
            let names = choices
                .iter()
                .map(|major| {
                    format!(
                        "Java {major}{}",
                        if Some(*major) == policy.recommended() {
                            " (recommended)"
                        } else {
                            ""
                        }
                    )
                })
                .collect::<Vec<_>>();
            let labels = names.iter().map(String::as_str).collect::<Vec<_>>();
            let selected = interactive.select("Java major for this pack", &labels)?;
            let major = *choices.get(selected).context("Invalid Java selection")?;
            return policy.select(Some(major)).map_err(Into::into);
        }
        let entered = interactive.text_input(
            "Java major for this pack (compatibility unknown)",
            String::new(),
        )?;
        return policy
            .select(Some(
                entered
                    .parse()
                    .context("Java major must be an integer of at least 8")?,
            ))
            .map_err(Into::into);
    }
    policy.select(None).map_err(Into::into)
}
