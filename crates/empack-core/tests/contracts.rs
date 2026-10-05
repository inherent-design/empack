use empack_core::path::{ArtifactStem, InstallDestination, PathSyntax, PortableRelPath};
use empack_core::projection::{BuildTarget::*, plan_build_targets};

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
fn build_prerequisites_are_fresh_unique_and_stable_for_every_short_request() {
    let targets = [Mrpack, Client, Server, ClientFull, ServerFull];
    for length in 0..=5u32 {
        for mut combination in 0..5usize.pow(length) {
            let mut request = Vec::new();
            for _ in 0..length {
                request.push(targets[combination % 5]);
                combination /= 5;
            }
            let plan = plan_build_targets(&request);
            assert_eq!(plan_build_targets(&plan), plan);
            for target in targets {
                let count = plan.iter().filter(|&&item| item == target).count();
                let expected = request.contains(&target)
                    || (target == Mrpack && request.iter().any(|t| matches!(t, Client | Server)));
                assert_eq!(count, usize::from(expected));
            }
            for target in [Client, Server] {
                if let Some(position) = plan.iter().position(|&t| t == target) {
                    assert!(plan[..position].contains(&Mrpack));
                }
            }
            let explicit_non_prerequisites: Vec<_> =
                plan.iter().filter(|&&t| t != Mrpack).collect();
            let mut wanted = Vec::new();
            for target in &request {
                if *target != Mrpack && !wanted.contains(&target) {
                    wanted.push(target);
                }
            }
            assert_eq!(explicit_non_prerequisites, wanted);
        }
    }
}
