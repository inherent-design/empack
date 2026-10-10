use empack_core::distribution::{Recipe, plan_recipes};
use empack_core::path::{ArtifactStem, InstallDestination, PathSyntax, PortableRelPath};

#[test]
fn portable_paths_reject_host_dependent_or_ambiguous_names() {
    for path in [
        "",
        ".",
        "..",
        "/mods/a.jar",
        "../a",
        "mods/../a",
        "mods/./a",
        "mods//a",
        "mods/a/",
        "C:a",
        "C:/a",
        "mods\\a",
        "a\0b",
        "a\nb",
        "mods/a.",
        "mods/a ",
        "mods/CON",
        "NUL.jar",
        "con .txt",
        "COM1.zip",
        "lpt9",
        "LPT².txt",
        "CONIN$",
        "CONOUT$.txt",
        "a:b",
        "a*b",
        "a?b",
    ] {
        for syntax in [PathSyntax::ProjectContent, PathSyntax::ArchiveMember] {
            assert!(PortableRelPath::parse(path, syntax).is_err(), "{path:?}");
        }
    }
}

#[test]
fn file_values_preserve_unicode_and_punctuation() {
    let spelling = "resourcepacks/[夏] A Pack $(literal).zip";
    let file = InstallDestination::parse(spelling).unwrap();
    assert_eq!(file.relative().as_str(), spelling);
    assert_eq!(
        file.relative().components().collect::<Vec<_>>(),
        ["resourcepacks", "[夏] A Pack $(literal).zip"]
    );
    for ordinary in ["console.txt", "COM10.zip", "lpt0", "auxiliary", "nulled"] {
        assert_eq!(ArtifactStem::parse(ordinary).unwrap().as_str(), ordinary);
    }
    assert!(ArtifactStem::parse("mods/file").is_err());
}

#[test]
fn build_selection_is_unique_and_stable_for_every_short_request() {
    let targets = [
        Recipe::MODRINTH,
        Recipe::CURSEFORGE,
        Recipe::PRISM_REFERENCES,
        Recipe::SERVER_REFERENCES,
        Recipe::PRISM_BUNDLED,
        Recipe::SERVER_BUNDLED,
    ];
    for length in 0..=5u32 {
        for mut combination in 0..targets.len().pow(length) {
            let mut request = Vec::new();
            for _ in 0..length {
                request.push(targets[combination % targets.len()]);
                combination /= targets.len();
            }
            let plan = plan_recipes(&request);
            let mut expected = Vec::new();
            for target in request {
                if !expected.contains(&target) {
                    expected.push(target);
                }
            }
            assert_eq!(plan, expected);
            assert_eq!(plan_recipes(&plan), plan);
        }
    }
}
