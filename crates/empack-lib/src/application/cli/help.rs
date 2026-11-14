//! Group the parser's rendered root commands without duplicating their help or wrapping.
use std::collections::BTreeMap;

const GROUPS: &[(&str, &[&str])] = &[
    (
        "Author",
        &[
            "init", "add", "remove", "sync", "update", "pin", "unpin", "adopt", "migrate",
            "manifest", "check",
        ],
    ),
    ("Install", &["instance"]),
    ("Publish", &["build", "release"]),
    (
        "Tools",
        &["status", "doctor", "pending", "recover", "clean", "help"],
    ),
];

pub(super) fn group_root_commands(help: &str) -> String {
    let Some((before, rest)) = help.split_once("\nCommands:\n") else {
        return help.to_owned();
    };
    let mut end = 0;
    for line in rest.split_inclusive('\n') {
        if !line.trim().is_empty() && !line.starts_with(' ') {
            break;
        }
        end += line.len();
    }
    let (commands, after) = rest.split_at(end);
    let mut rows = BTreeMap::<&str, String>::new();
    let mut current = None;
    for line in commands.split_inclusive('\n') {
        if let Some(row) = line.strip_prefix("  ")
            && !row.starts_with(char::is_whitespace)
            && let Some(name) = row.split_ascii_whitespace().next()
        {
            if rows.insert(name, String::new()).is_some() {
                return help.to_owned();
            }
            current = Some(name);
        }
        if let Some(name) = current {
            rows.get_mut(name)
                .expect("command row exists")
                .push_str(line);
        }
    }
    // Nested help has its own command set. Unknown root commands must remain
    // visible until their group is assigned and the command-graph test is updated.
    if rows.len() != GROUPS.iter().map(|(_, names)| names.len()).sum::<usize>()
        || !GROUPS
            .iter()
            .flat_map(|(_, names)| *names)
            .all(|name| rows.contains_key(name))
    {
        return help.to_owned();
    }
    let mut grouped = format!("{before}\n");
    for (heading, names) in GROUPS {
        grouped.push_str(heading);
        grouped.push_str(":\n");
        for name in *names {
            grouped.push_str(rows[name].trim_end_matches('\n'));
            grouped.push('\n');
        }
        grouped.push('\n');
    }
    grouped.push_str(after);
    grouped
}
