//! Discover the compiled public CLI without executing a product operation.
use clap::CommandFactory;
use empack_lib::application::Cli;
use serde_json::{Value, json};

fn surfaces(command: &clap::Command, parent: &str, rows: &mut Vec<Value>) {
    let path = if parent.is_empty() {
        command.get_name().to_owned()
    } else {
        format!("{parent} {}", command.get_name())
    };
    let children: Vec<_> = command
        .get_subcommands()
        .filter(|child| !child.is_hide_set())
        .collect();
    rows.push(json!({
        "id": path,
        "kind": if children.is_empty() { "leaf" } else { "command" },
        "aliases": command.get_all_aliases().collect::<Vec<_>>(),
    }));
    // Inherited global options have one owner at the root. Leaf commands still
    // carry their own help/version arguments and all local options.
    for argument in command
        .get_arguments()
        .filter(|arg| !arg.is_hide_set() && (parent.is_empty() || !arg.is_global_set()))
    {
        rows.push(json!({
            "id": format!("{path}::arg:{}", argument.get_id()),
            "kind": "argument",
            "long": argument.get_long(),
            "short": argument.get_short(),
            "global": argument.is_global_set(),
            "required": argument.is_required_set(),
            "aliases": argument.get_all_aliases().unwrap_or_default(),
            "values": argument.get_possible_values().iter()
                .filter(|value| !value.is_hide_set())
                .map(|value| value.get_name()).collect::<Vec<_>>(),
        }));
    }
    for group in command.get_groups() {
        rows.push(json!({
            "id": format!("{path}::group:{}", group.get_id()),
            "kind": "group",
            "arguments": group.get_args().map(|id| id.as_str()).collect::<Vec<_>>(),
            "required": group.is_required_set(),
        }));
    }
    for child in children {
        surfaces(child, &path, rows);
    }
}

fn main() {
    let mut command = Cli::command();
    command.build();
    let mut rows = Vec::new();
    surfaces(&command, "", &mut rows);
    rows.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": 1,
            "source": "clap-command-factory",
            "surfaces": rows,
        }))
        .unwrap()
    );
}
