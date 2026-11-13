//! Human dependency changes use receipt counts, not the number of provider requests.
use empack_core::removal::RemovalMode;

pub(super) fn independence_unverified(dependencies: &[String], details: bool) -> String {
    let shown = if details {
        dependencies.len()
    } else {
        dependencies.len().min(5)
    };
    let mut names = dependencies[..shown]
        .iter()
        .map(|key| crate::terminal::display_line(key))
        .collect::<Vec<_>>()
        .join(", ");
    if shown < dependencies.len() {
        names.push_str(&format!(" and {} others", dependencies.len() - shown));
    }
    format!(
        "Cannot apply these changes separately: dependency information is incomplete for {names}."
    )
}

pub(super) fn update_plan(selected: usize, changed: usize) -> String {
    format!(
        "Update plan: {selected} {}\n    Change  {changed}\n    Keep    {}",
        if selected == 1 {
            "dependency"
        } else {
            "dependencies"
        },
        selected - changed
    )
}

pub(super) fn checked(checked: usize) -> String {
    let noun = if checked == 1 {
        "dependency"
    } else {
        "dependencies"
    };
    format!("Checked {checked} {noun}")
}

pub(super) fn update_details(checked: usize, changed: usize, pins: usize) -> String {
    let mut lines = vec![
        format!("    Changed  {changed}"),
        format!("    Kept     {}", checked - changed),
    ];
    if pins == 0 {
        lines.push("Author settings were kept.".into());
    } else {
        lines.push(format!(
            "Updated {pins} {}; other author settings were kept.",
            if pins == 1 { "pin" } else { "pins" }
        ));
    }
    lines.join("\n")
}

pub(super) fn removal(mode: RemovalMode, count: usize) -> String {
    let noun = if count == 1 {
        "dependency"
    } else {
        "dependencies"
    };
    match mode {
        RemovalMode::ForgetRoots => {
            format!("Removed {count} direct {noun} from empack.yml; installed files were kept")
        }
        RemovalMode::RemoveContent => format!("Removed {count} {noun}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checked_dependencies_are_not_presented_as_upgrades() {
        assert_eq!(
            format!("{}\n{}", checked(2), update_details(2, 0, 0)),
            "Checked 2 dependencies\n    Changed  0\n    Kept     2\nAuthor settings were kept."
        );
        assert_eq!(
            format!("{}\n{}", checked(1), update_details(1, 1, 1)),
            "Checked 1 dependency\n    Changed  1\n    Kept     0\nUpdated 1 pin; other author settings were kept."
        );
    }
    #[test]
    fn forgetting_a_direct_dependency_does_not_claim_content_removal() {
        assert_eq!(
            removal(RemovalMode::ForgetRoots, 1),
            "Removed 1 direct dependency from empack.yml; installed files were kept"
        );
        assert_eq!(
            removal(RemovalMode::RemoveContent, 2),
            "Removed 2 dependencies"
        );
    }
}
