//! Literal on-disk runtime assertions; no production decoder supplies the oracle.
use serde_json::{Value, json};
use std::{fs, path::Path};

pub fn assert_selection(
    root: &Path,
    game: &str,
    loader: &str,
    pin: Option<&str>,
    java: u16,
) -> Option<String> {
    let author: Value =
        serde_saphyr::from_str(&fs::read_to_string(root.join("empack.yml")).unwrap()).unwrap();
    assert_eq!(
        author["runtime"],
        json!({
            "minecraft": game,
            "java": java,
            "loader": pin.map_or_else(|| json!(loader), |version| json!({"kind": loader, "pin": version}))
        })
    );
    let lock: Value =
        serde_saphyr::from_str(&fs::read_to_string(root.join("empack.lock")).unwrap()).unwrap();
    assert_eq!(lock["schema"], 2);
    assert_eq!(lock["dependencies"], json!({}));
    assert_eq!(lock["runtime"]["minecraft"], game);
    assert_eq!(lock["runtime"]["loader"], loader);
    let selected = lock["runtime"]["loader-version"]
        .as_str()
        .map(str::to_owned);
    if loader == "vanilla" {
        assert_eq!(lock["runtime"]["loader-version"], Value::Null);
    } else {
        assert!(selected.as_ref().is_some_and(|value| !value.is_empty()));
    }
    if let Some(pin) = pin {
        assert_eq!(selected.as_deref(), Some(pin));
    }
    selected
}
