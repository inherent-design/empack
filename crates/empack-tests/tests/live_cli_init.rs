//! Fresh official catalog selection, not Java acquisition or successful game startup.
#[path = "support/runtime_selection.rs"]
mod runtime_selection;
use empack_tests::harness::process::TestProject;
use std::fs;

fn initialize(loader: &str, game: Option<&str>, java: &str, prefix: Option<&str>) {
    let project = TestProject::new();
    let mut command = project.cmd();
    command.args([
        "init",
        "--yes",
        "--modloader",
        loader,
        "--java",
        java,
        "--author",
        "Catalog Fixture",
        "pack",
    ]);
    if let Some(game) = game {
        command.args(["--mc-version", game]);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{loader}/{game:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let root = project.dir().join("pack");
    let author: serde_json::Value =
        serde_saphyr::from_str(&fs::read_to_string(root.join("empack.yml")).unwrap()).unwrap();
    let selected_game = author["runtime"]["minecraft"].as_str().unwrap();
    assert!(!selected_game.is_empty());
    if let Some(game) = game {
        assert_eq!(selected_game, game);
    }
    let selected = runtime_selection::assert_selection(
        &root,
        selected_game,
        if loader == "none" { "vanilla" } else { loader },
        None,
        java.parse().unwrap(),
    );
    if let Some(prefix) = prefix {
        assert!(
            selected.as_deref().unwrap().starts_with(prefix),
            "{selected:?}"
        );
    }
    // Every test starts with an empty isolated cache and omits a required version:
    // Minecraft for vanilla, loader for the others. Exact-pin init is tested offline.
}

macro_rules! live_init_loader {
    ($name:ident, $loader:expr, $game:expr, $java:expr, $prefix:expr) => {
        #[test]
        #[ignore = "requires official runtime catalogs; run the live suite explicitly"]
        fn $name() {
            initialize($loader, $game, $java, $prefix);
        }
    };
}

live_init_loader!(
    live_cli_init_vanilla_discovers_latest_game,
    "none",
    None,
    "21",
    None
);
live_init_loader!(
    live_cli_init_fabric_binds_runtime,
    "fabric",
    Some("1.21.1"),
    "21",
    None
);
live_init_loader!(
    live_cli_init_quilt_binds_runtime,
    "quilt",
    Some("1.21.1"),
    "21",
    None
);
live_init_loader!(
    live_cli_init_forge_binds_runtime,
    "forge",
    Some("1.21.1"),
    "21",
    Some("52.")
);
live_init_loader!(
    live_cli_init_legacy_forge_binds_runtime,
    "forge",
    Some("1.7.10"),
    "8",
    Some("10.13.")
);
live_init_loader!(
    live_cli_init_early_neoforge_binds_runtime,
    "neoforge",
    Some("1.20.1"),
    "21",
    Some("47.1.")
);
live_init_loader!(
    live_cli_init_neoforge_binds_runtime,
    "neoforge",
    Some("1.21.1"),
    "21",
    Some("21.1.")
);
