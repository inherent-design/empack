use super::*;
use empack_core::{
    path::PathSyntax,
    requirements::{ChoiceKey, OptionalChoice, Requirement},
};
const DOCUMENT: &str = r#"filename = "renderer.jar"
side = "client"
[download]
url = "https://example.com/download"
hash-format = "md5"
hash = "321c3cf486ed509164edec1e1981fec8"
[update.modrinth]
mod-id = "AANobbMI"
version = "Version1"
[option]
optional = true
description = "Feature"
"#;
fn parse(document: &str) -> Result<BackendFile> {
    BackendFile::parse(
        PortableRelPath::parse("mods/alias.pw.toml", PathSyntax::ProjectContent).unwrap(),
        document.as_bytes(),
    )
}
#[test]
fn backend_observation_keeps_alias_destination_pin_and_optional_metadata_distinct() {
    let observed = parse(DOCUMENT).unwrap();
    assert_eq!(observed.metadata_path.as_str(), "mods/alias.pw.toml");
    assert_eq!(
        observed.destination.relative().as_str(),
        "mods/renderer.jar"
    );
    let provider = observed.provider.as_ref().unwrap();
    let mut pin = ResolvedPin {
        project: provider.project.clone(),
        selection: provider.selection.clone().unwrap(),
    };
    let mut requirements = Requirements {
        client: Requirement::Optional(OptionalChoice {
            key: ChoiceKey::parse("rendering").unwrap(),
            default_enabled: false,
            description: Some("Feature".into()),
        }),
        server: Requirement::Unsupported,
    };
    assert!(
        observed
            .matches_selection_and_requirements(Some(&pin), &requirements)
            .unwrap()
    );
    pin.selection = provider.project.parse_pin("Version2").unwrap();
    assert!(
        !observed
            .matches_selection_and_requirements(Some(&pin), &requirements)
            .unwrap()
    );
    requirements.server = Requirement::Required;
    assert!(
        observed
            .matches_selection_and_requirements(Some(&pin), &requirements)
            .is_err()
    );
}
#[test]
fn backend_records_cannot_hide_unsafe_paths_or_malformed_semantic_fields() {
    for name in ["../outside.jar", "dir/file.jar", "CON", "", r"dir\file.jar"] {
        assert!(
            parse(&DOCUMENT.replace("renderer.jar", name)).is_err(),
            "{name}"
        );
    }
    for document in [
        DOCUMENT.replace("side = \"client\"", "side = \"unknown\""),
        DOCUMENT.replace("optional = true", "optional = 'true'"),
        DOCUMENT.replace("hash-format = \"md5\"", "hash-format = \"unknown\""),
        DOCUMENT.replace("version = \"Version1\"", "version = true"),
    ] {
        assert!(parse(&document).is_err());
    }
}
#[test]
fn restricted_download_mode_requires_the_corresponding_provider() {
    let invalid = DOCUMENT.replace(
        "url = \"https://example.com/download\"",
        "mode = 'metadata:curseforge'",
    );
    assert!(parse(&invalid).is_err());
    let valid = invalid.replace(
        "[update.modrinth]\nmod-id = \"AANobbMI\"\nversion = \"Version1\"",
        "[update.curseforge]\nproject-id = 238222\nfile-id = 1234567",
    );
    assert!(matches!(
        parse(&valid).unwrap().download,
        BackendDownload::CurseForgeMetadata
    ));
}
