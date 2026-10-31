//! Replacement assertions through the real executable, with literal incoming data.
use super::files;
#[path = "approval.rs"]
mod approval;

use empack_tests::harness::{
    process::{TestProject, offline_command},
    workspace::snapshot,
};
use std::{collections::BTreeMap, fs, io::Write, path::Path};

const MANIFEST: &[u8] = br#"{"formatVersion":1,"game":"minecraft","name":"Replacement","versionId":"2","files":[],"dependencies":{"minecraft":"1.21.1"}}"#;
const RETAINED: &[(&str, &[u8])] = &[
    ("notes.txt", b"author notes"),
    ("research/decision.md", b"reason for the old config"),
    (
        "templates/client/instance.cfg.template",
        b"custom launcher template",
    ),
    ("dist/previous.zip", b"previous artifact"),
    ("worlds/playtest/level.dat", b"player-owned world sentinel"),
];

fn fixture() -> TestProject {
    let project = TestProject::new();
    offline_command(project.dir())
        .args([
            "--yes",
            "init",
            "--modloader",
            "none",
            "--mc-version",
            "1.21.1",
            "existing",
        ])
        .assert()
        .success();
    let root = project.dir().join("existing");
    for (name, bytes) in RETAINED.iter().copied().chain([
        ("pack/config/old.properties", b"old=source".as_slice()),
        (
            "overrides/client/config/old.properties",
            b"old=client".as_slice(),
        ),
    ]) {
        let path = root.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    fs::create_dir_all(root.join("research/empty")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            root.join("research/decision.md"),
            fs::Permissions::from_mode(0o640),
        )
        .unwrap();
    }
    project
}

fn archive(index: &[u8], members: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, bytes) in
        std::iter::once(("modrinth.index.json", index)).chain(members.iter().copied())
    {
        writer
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn outside_replacement(path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix("existing") else {
        return false;
    };
    !relative.starts_with("pack")
        && !relative.starts_with("overrides")
        && !["empack.yml", "empack.lock", "empack.bindings.yml"]
            .iter()
            .any(|name| relative == Path::new(name))
}

#[test]
fn cli_forced_import_rejects_invalid_candidates_without_mutation() {
    let cases = [
        (
            "not-zip",
            b"not a ZIP archive".to_vec(),
            "Missing ZIP end record",
        ),
        ("bad-json", archive(b"{", &[]), "Invalid manifest JSON"),
        (
            "traversal",
            archive(MANIFEST, &[("overrides/../escaped.txt", b"escape")]),
            "path",
        ),
        (
            "case-collision",
            archive(
                MANIFEST,
                &[
                    ("overrides/config/A.cfg", b"a"),
                    ("overrides/config/a.cfg", b"b"),
                ],
            ),
            "collides",
        ),
        (
            "ancestor-collision",
            archive(
                MANIFEST,
                &[
                    ("overrides/config", b"file"),
                    ("overrides/config/a.cfg", b"child"),
                ],
            ),
            "ancestor",
        ),
    ];
    for (label, incoming, diagnostic) in cases {
        let project = fixture();
        fs::write(project.dir().join("incoming.mrpack"), incoming).unwrap();
        let before = files::snapshot(project.dir());
        let modes = snapshot(project.dir()).entries;
        for preview in [false, true] {
            let mut command = offline_command(project.dir());
            command.args([
                "--yes",
                "init",
                "--force",
                "--from",
                "incoming.mrpack",
                "existing",
            ]);
            if preview {
                command.arg("--dry-run");
            }
            let output = command.output().unwrap();
            assert!(
                !output.status.success(),
                "{label}, preview={preview}: {output:?}"
            );
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                stderr.to_lowercase().contains(&diagnostic.to_lowercase()),
                "{label}: {stderr}"
            );
            assert_eq!(
                files::snapshot(project.dir()),
                before,
                "{label}, preview={preview}"
            );
            assert_eq!(
                snapshot(project.dir()).entries,
                modes,
                "{label}, preview={preview}"
            );
        }
    }
}

#[test]
fn cli_forced_initialization_rejects_invalid_options_without_mutation() {
    for extra in [
        ["--datapack-folder", "../escape"],
        ["--pack-name", " "],
        ["--loader-version", "unexpected-for-vanilla"],
    ] {
        let project = fixture();
        let before = files::snapshot(project.dir());
        let modes = snapshot(project.dir()).entries;
        for preview in [false, true] {
            let mut command = offline_command(project.dir());
            command
                .args([
                    "--yes",
                    "init",
                    "--force",
                    "--modloader",
                    "none",
                    "--mc-version",
                    "1.21.1",
                    "existing",
                ])
                .args(extra);
            if preview {
                command.arg("--dry-run");
            }
            let output = command.output().unwrap();
            assert!(!output.status.success(), "{extra:?}: {output:?}");
            assert!(
                !String::from_utf8_lossy(&output.stderr).contains("unexpected argument"),
                "{output:?}"
            );
            assert_eq!(
                files::snapshot(project.dir()),
                before,
                "{extra:?}, preview={preview}"
            );
            assert_eq!(
                snapshot(project.dir()).entries,
                modes,
                "{extra:?}, preview={preview}"
            );
        }
    }
}

#[test]
fn cli_forced_replacement_preview_and_publication_keep_unowned_files() {
    for import in [false, true] {
        let project = fixture();
        fs::write(
            project.dir().join("incoming.mrpack"),
            archive(
                MANIFEST,
                &[("overrides/config/new.properties", b"new=import")],
            ),
        )
        .unwrap();
        let arguments = if import {
            vec!["init", "--force", "--from", "incoming.mrpack", "existing"]
        } else {
            vec![
                "init",
                "--force",
                "--modloader",
                "none",
                "--mc-version",
                "1.21.1",
                "--pack-name",
                "Replacement",
                "--author",
                "Tester",
                "--pack-version",
                "2",
                "existing",
            ]
        };
        let before = files::snapshot(project.dir());
        let modes = snapshot(project.dir()).entries;
        let preview = offline_command(project.dir())
            .args(&arguments)
            .args(["--yes", "--dry-run"])
            .output()
            .unwrap();
        assert!(preview.status.success(), "{preview:?}");
        assert!(
            String::from_utf8_lossy(&preview.stdout).contains("old.properties"),
            "{preview:?}"
        );
        assert_eq!(files::snapshot(project.dir()), before);
        assert_eq!(snapshot(project.dir()).entries, modes);

        offline_command(project.dir())
            .args(&arguments)
            .arg("--yes")
            .assert()
            .success();
        let root = project.dir().join("existing");
        assert!(!root.join("pack/config/old.properties").exists());
        assert!(!root.join("overrides/client/config/old.properties").exists());
        let retained_bytes: BTreeMap<_, _> = before
            .iter()
            .filter(|(path, _)| outside_replacement(path))
            .collect();
        let after = files::snapshot(project.dir());
        assert_eq!(
            after
                .iter()
                .filter(|(path, _)| outside_replacement(path))
                .collect::<BTreeMap<_, _>>(),
            retained_bytes
        );
        let retained_attributes: BTreeMap<_, _> = modes
            .iter()
            .filter(|(path, _)| outside_replacement(path))
            .collect();
        let after_attributes = snapshot(project.dir()).entries;
        assert_eq!(
            after_attributes
                .iter()
                .filter(|(path, _)| outside_replacement(path))
                .collect::<BTreeMap<_, _>>(),
            retained_attributes
        );
        if import {
            assert_eq!(
                fs::read(root.join("overrides/common/config/new.properties")).unwrap(),
                b"new=import"
            );
        }
        let document = fs::read_to_string(root.join("empack.yml")).unwrap();
        assert!(document.contains("Replacement"));
        assert!(root.join("empack.lock").is_file());
        assert_eq!(
            fs::read(project.dir().join("incoming.mrpack")).unwrap(),
            before[Path::new("incoming.mrpack")]
        );
    }
}

#[test]
fn cli_pty_forced_replacement_decline_preserves_existing_project() {
    for import in [false, true] {
        let project = fixture();
        fs::write(
            project.dir().join("incoming.mrpack"),
            archive(
                MANIFEST,
                &[("overrides/config/new.properties", b"new=import")],
            ),
        )
        .unwrap();
        let before = files::snapshot(project.dir());
        let modes = snapshot(project.dir()).entries;
        let mut args = vec!["init", "--force"];
        if import {
            args.extend(["--from", "incoming.mrpack"]);
        } else {
            args.extend([
                "--modloader",
                "none",
                "--mc-version",
                "1.21.1",
                "--pack-name",
                "Replacement",
                "--author",
                "Tester",
                "--pack-version",
                "2",
            ]);
        }
        args.push("existing");
        approval::preserve_without_approval(
            project.dir(),
            project.dir(),
            &project.dir().join(".host-state"),
            &args,
            "(?i)apply this (initialization|import) plan",
        );
        assert_eq!(files::snapshot(project.dir()), before);
        assert_eq!(snapshot(project.dir()).entries, modes);
    }
}
