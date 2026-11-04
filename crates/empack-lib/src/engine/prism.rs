//! Shared Prism component contract for exports and installed runtime transitions.
use anyhow::{Context, Result, ensure};
use empack_core::model::{LoaderKind, RuntimeResolution};
use serde_json::{Value, json};
use std::collections::BTreeSet;

fn loader(runtime: &RuntimeResolution) -> Result<Option<(&'static str, String)>> {
    let uid = match runtime.loader {
        LoaderKind::Vanilla => return Ok(None),
        LoaderKind::Fabric => "net.fabricmc.fabric-loader",
        LoaderKind::Quilt => "org.quiltmc.quilt-loader",
        LoaderKind::Forge => "net.minecraftforge",
        LoaderKind::NeoForge => "net.neoforged",
    };
    let version = runtime
        .loader_version
        .as_ref()
        .context("Client loader lacks exact version")?
        .as_str();
    let version = if runtime.loader == LoaderKind::Forge {
        crate::engine::runtime_versions::canonicalize_forge_loader_version(
            runtime.minecraft.as_str(),
            version,
        )
    } else {
        version.into()
    };
    Ok(Some((uid, version)))
}
pub(super) fn profile(runtime: &RuntimeResolution) -> Result<Vec<u8>> {
    let mut components =
        vec![json!({"uid":"net.minecraft","version":runtime.minecraft.as_str(),"important":true})];
    if let Some((uid, version)) = loader(runtime)? {
        components.push(json!({"uid":uid,"version":version}));
    }
    Ok(serde_json::to_vec_pretty(
        &json!({"formatVersion":1,"components":components}),
    )?)
}
/// Inspect actual component bytes, including a user-owned replacement. User extra components
/// may extend the profile; they cannot disable, duplicate or change the locked game/loader.
pub(super) fn verify_profile(bytes: &[u8], runtime: &RuntimeResolution) -> Result<()> {
    #[derive(serde::Deserialize)]
    struct Profile {
        #[serde(rename = "formatVersion")]
        version: u32,
        components: Vec<Component>,
    }
    #[derive(serde::Deserialize)]
    struct Component {
        uid: String,
        version: String,
        important: Option<bool>,
        disabled: Option<bool>,
    }
    let value: Profile = serde_json::from_slice(bytes)?;
    ensure!(value.version == 1, "Unsupported client component format");
    let components = &value.components;
    let expected_loader = loader(runtime)?;
    let mut seen = BTreeSet::new();
    let mut game = false;
    let mut selected_loader = false;
    for component in components {
        let uid = component.uid.as_str();
        let version = component.version.as_str();
        ensure!(
            !version.is_empty(),
            "Client component lacks an exact version"
        );
        ensure!(seen.insert(uid), "Duplicate client component");
        if uid == "net.minecraft" {
            ensure!(
                version == runtime.minecraft.as_str()
                    && component.important == Some(true)
                    && component.disabled != Some(true),
                "Client game differs from locked runtime"
            );
            game = true;
        } else if matches!(
            uid,
            "net.fabricmc.fabric-loader"
                | "org.quiltmc.quilt-loader"
                | "net.minecraftforge"
                | "net.neoforged"
        ) {
            ensure!(
                expected_loader
                    .as_ref()
                    .is_some_and(|(expected_uid, expected_version)| uid == *expected_uid
                        && version == expected_version)
                    && component.disabled != Some(true),
                "Client loader differs from locked runtime"
            );
            selected_loader = true;
        }
    }
    ensure!(
        game && (selected_loader == expected_loader.is_some()),
        "Client profile omits the locked runtime"
    );
    Ok(())
}

/// Preserve user components and attributes; only the known game/loader components change.
/// Existing core components must still match the previously completed release.
pub(super) fn transition(
    previous: Option<&[u8]>,
    old: &super::release::ReleaseRuntime,
    new: &super::release::ReleaseRuntime,
) -> Result<Vec<u8>> {
    let new = new.resolution()?;
    let Some(bytes) = previous else {
        return profile(&new);
    };
    verify_profile(bytes, &old.resolution()?)?;
    let mut value: Value = serde_json::from_slice(bytes)?;
    let components = value["components"]
        .as_array_mut()
        .context("Missing Prism components")?;
    // Validate the desired profile separately, including loader canonicalization.
    let desired: Value = serde_json::from_slice(&profile(&new)?)?;
    let mut wanted = desired["components"].as_array().unwrap().clone();
    for component in components.iter_mut() {
        let uid = component["uid"].as_str().unwrap();
        if let Some(index) = wanted.iter().position(|entry| entry["uid"] == uid) {
            let replacement = wanted.remove(index);
            component["version"] = replacement["version"].clone();
        }
    }
    let selected_loader = loader(&new)?.map(|(uid, _)| uid);
    components.retain(|component| {
        let uid = component["uid"].as_str().unwrap();
        !matches!(
            uid,
            "net.fabricmc.fabric-loader"
                | "org.quiltmc.quilt-loader"
                | "net.minecraftforge"
                | "net.neoforged"
        ) || Some(uid) == selected_loader
    });
    components.extend(wanted);
    let output = serde_json::to_vec_pretty(&value)?;
    verify_profile(&output, &new)?;
    // Unchanged metadata remains byte-for-byte stable, including formatting.
    if serde_json::from_slice::<Value>(bytes)? == value {
        Ok(bytes.to_vec())
    } else {
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_transitions_switch_known_loaders_and_preserve_user_metadata() {
        use super::super::release::{ReleaseLoader, ReleaseRuntime};
        let mut old = ReleaseRuntime {
            minecraft: "1.21.1".into(),
            loader: ReleaseLoader::Vanilla,
            java_major: 21,
        };
        let mut bytes = br#"{ "formatVersion":1,"custom":{"keep":true},"components":[{"uid":"net.minecraft","version":"1.21.1","important":true,"cachedName":"Game label"},{"uid":"user.component","version":"7","extra":[1,false]}] }"#.to_vec();
        let base: Value = serde_json::from_slice(&bytes).unwrap();
        for (selected, expected) in [
            (
                ReleaseLoader::Fabric {
                    version: "0.16.0".into(),
                },
                Some(("net.fabricmc.fabric-loader", "0.16.0")),
            ),
            (
                ReleaseLoader::Quilt {
                    version: "0.27.1".into(),
                },
                Some(("org.quiltmc.quilt-loader", "0.27.1")),
            ),
            (
                ReleaseLoader::Forge {
                    version: "52.1.0".into(),
                },
                Some(("net.minecraftforge", "52.1.0")),
            ),
            (
                ReleaseLoader::NeoForge {
                    version: "21.1.42".into(),
                },
                Some(("net.neoforged", "21.1.42")),
            ),
            (ReleaseLoader::Vanilla, None),
        ] {
            let new = ReleaseRuntime {
                loader: selected,
                ..old.clone()
            };
            let changed = transition(Some(&bytes), &old, &new).unwrap();
            let mut wanted = base.clone();
            if let Some((uid, version)) = expected {
                wanted["components"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"uid":uid,"version":version}));
            }
            assert_eq!(serde_json::from_slice::<Value>(&changed).unwrap(), wanted);
            assert_eq!(transition(Some(&changed), &new, &new).unwrap(), changed);
            old = new;
            bytes = changed;
        }
        // Java policy changes do not rewrite unrelated component/profile bytes.
        let changed_java = ReleaseRuntime {
            java_major: 25,
            ..old.clone()
        };
        assert_eq!(
            transition(Some(&bytes), &old, &changed_java).unwrap(),
            bytes
        );
        for invalid in [
            String::from_utf8(bytes.clone())
                .unwrap()
                .replace("\"version\": \"1.21.1\"", "\"version\": \"1.21.2\""),
            String::from_utf8(bytes.clone())
                .unwrap()
                .replace("\"important\": true", "\"important\": false"),
        ] {
            assert_ne!(
                invalid.as_bytes(),
                bytes,
                "negative fixture must change authority"
            );
            assert!(transition(Some(invalid.as_bytes()), &old, &changed_java).is_err());
        }
    }

    #[test]
    fn component_authority_rejects_duplicate_fields_and_preserves_unchanged_bytes() {
        let requirements = super::super::release::ReleaseRuntime {
            minecraft: "1.21.1".into(),
            loader: super::super::release::ReleaseLoader::Vanilla,
            java_major: 21,
        };
        let bytes = br#"{ "formatVersion":1,"components":[{"uid":"net.minecraft","version":"1.21.1","important":true}] }"#;
        assert_eq!(
            transition(Some(bytes), &requirements, &requirements).unwrap(),
            bytes
        );
        for field in [
            "\"version\":\"1.21.1\",\"version\":\"1.21.1\"",
            "\"version\":\"1.21.1\",\"disabled\":true,\"disabled\":false",
        ] {
            let corrupt = std::str::from_utf8(bytes)
                .unwrap()
                .replace("\"version\":\"1.21.1\"", field);
            assert!(transition(Some(corrupt.as_bytes()), &requirements, &requirements).is_err());
        }
    }
}
