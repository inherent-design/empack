use empack_core::{identity::ProviderProjectId, model::ResolvedIdentity};
use empack_tests::harness::process::{TestProject, read_project};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
#[path = "support/terminal.rs"]
mod terminal;

fn placement_path(placement: &empack_core::model::Placement) -> PathBuf {
    use empack_core::model::ContentLayer;
    let layer = match placement.layer {
        ContentLayer::Common => "pack",
        ContentLayer::CommonOverride => "overrides/common",
        ContentLayer::Client => "overrides/client",
        ContentLayer::Server => "overrides/server",
    };
    PathBuf::from(layer).join(placement.destination.relative().as_str())
}

fn snapshot(root: &Path, excluded: &[&str]) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, path: &Path, excluded: &[&str], files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in std::fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let relative = path.strip_prefix(root).unwrap();
            if excluded.iter().any(|name| relative.starts_with(name)) {
                continue;
            }
            if entry.file_type().unwrap().is_dir() {
                visit(root, &entry.path(), excluded, files);
            } else if entry.file_type().unwrap().is_file() {
                files.insert(
                    entry.path().strip_prefix(root).unwrap().into(),
                    std::fs::read(entry.path()).unwrap(),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, root, excluded, &mut files);
    files
}
// Author checks omit host bookkeeping before reading; previews snapshot everything.
fn author_snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    snapshot(root, &[".host-state", ".empack-cache", ".harness-home"])
}
fn execute(project: &TestProject, args: &[&str], credential: Option<&str>) -> std::process::Output {
    let mut command = project.cmd();
    command.args(args);
    if let Some(key) = credential {
        command.env("EMPACK_KEY_CURSEFORGE", key);
    }
    // Never format the command/environment: the CurseForge variant contains a key.
    let mut output = assert_cmd::Command::from_std(command)
        .timeout(std::time::Duration::from_secs(120))
        .output()
        .expect("spawn empack");
    if let Some(key) = credential {
        for stream in [&mut output.stdout, &mut output.stderr] {
            *stream = String::from_utf8_lossy(stream)
                .replace(key, "[credential redacted]")
                .into_bytes();
        }
    }
    output
}
fn checked(project: &TestProject, args: &[&str], credential: Option<&str>) -> std::process::Output {
    let output = execute(project, args, credential);
    assert!(
        output.status.success(),
        "{args:?} failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}
fn run(project: &TestProject, args: &[&str]) -> std::process::Output {
    checked(project, args, None)
}

#[test]
#[ignore = "requires a real terminal and live Modrinth search/version catalogs"]
fn live_cli_pty_search_requires_choice_and_approval() {
    use expectrl::{Expect, Regex};
    let project = TestProject::workflow_fixture("deliberate-search", "fabric", "1.21.1");
    let scratch = tempfile::tempdir().unwrap();
    std::fs::write(project.dir().join("prior-build.zip"), b"previous artifact").unwrap();
    let before = author_snapshot(project.dir());
    assert!(read_project(project.dir()).intent().roots.is_empty());
    // Search ranking may change. Select the canonical project from the displayed
    // labels rather than assuming the first result or a particular row number.
    let selected = ProviderProjectId::Modrinth(
        empack_core::identity::ModrinthProjectId::parse("SfMw2IZN").unwrap(),
    ); // Nvidium
    for approval in [None, Some(false), Some(true)] {
        let mut command = project.cmd();
        for variable in ["TMPDIR", "TMP", "TEMP"] {
            command.env(variable, scratch.path());
        }
        command.args([
            "add",
            "--platform",
            "modrinth",
            "--type",
            "mod",
            "rendering engine",
        ]);
        let mut console = terminal::spawn(command);
        console.set_expect_timeout(Some(std::time::Duration::from_secs(120)));
        console
            .expect(Regex("Select Mod for rendering engine"))
            .unwrap();
        if let Some(approve) = approval {
            console.send("SfMw2IZN").unwrap();
            console.send_line("").unwrap();
            console
                .expect(Regex("(?i)apply this addition plan"))
                .unwrap();
            // A choice is not publication approval. Candidate files use the
            // separate scratch root rather than the author tree.
            assert_eq!(author_snapshot(project.dir()), before);
            console.send_line(if approve { "y" } else { "n" }).unwrap();
            console
                .expect(Regex(if approve {
                    "Recorded [0-9]+ dependency bindings and [0-9]+ exact references"
                } else {
                    "Addition not applied"
                }))
                .unwrap();
        } else {
            console.send("\u{1b}").unwrap();
            console.expect(Regex("(?i)interrupted")).unwrap();
        }
        #[cfg(not(windows))]
        {
            console.expect(expectrl::Eof).unwrap();
            assert!(matches!(console.get_process().wait().unwrap(),
                expectrl::process::unix::WaitStatus::Exited(_, code)
                if code == if approval.is_none() { 130 } else { 0 }));
        }
        #[cfg(windows)]
        assert_eq!(
            console.get_process().wait(Some(120_000)).unwrap(),
            if approval.is_none() { 130 } else { 0 }
        );
        if approval != Some(true) {
            assert_eq!(author_snapshot(project.dir()), before);
        }
    }
    let result = read_project(project.dir());
    assert_eq!(result.intent().roots.len(), 1);
    let key = result.intent().roots.keys().next().unwrap();
    let dependency = &result.lock().dependencies[key];
    assert_eq!(
        dependency.identity,
        ResolvedIdentity::Provider(selected.clone())
    );
    assert_eq!(dependency.selected.as_ref().unwrap().project, selected);
    for (path, bytes) in before {
        if path != Path::new("empack.yml") && path != Path::new("empack.lock") {
            assert_eq!(std::fs::read(project.dir().join(path)).unwrap(), bytes);
        }
    }
}

#[test]
#[ignore = "requires live Modrinth catalog and downloads"]
fn live_cli_equivalent_provider_selectors_converge() {
    selector_journey(
        "modrinth",
        "fabric",
        "sodium",
        "AANobbMI",
        "https://modrinth.com/mod/sodium",
        None,
    );
}
#[test]
#[ignore = "requires explicit CurseForge API credential and live catalog"]
fn live_cli_curseforge_selectors_converge() {
    let key = std::env::var("EMPACK_KEY_CURSEFORGE")
        .expect("CurseForge qualification requires EMPACK_KEY_CURSEFORGE");
    assert!(!key.trim().is_empty(), "CurseForge credential is empty");
    selector_journey(
        "curseforge",
        "neoforge",
        "jei",
        "238222",
        "https://www.curseforge.com/minecraft/mc-mods/jei",
        Some(&key),
    );
}
fn selector_journey(
    platform: &str,
    loader: &str,
    slug: &str,
    id: &str,
    url: &str,
    credential: Option<&str>,
) {
    let project = TestProject::workflow_fixture("provider-add", loader, "1.21.1");
    std::fs::write(
        project.dir().join("prior-build.zip"),
        b"Retain previous output",
    )
    .unwrap();
    let run = |args: &[&str]| checked(&project, args, credential);
    run(&["--yes", "add", "--platform", platform, slug]);
    let resolved = read_project(project.dir());
    let canonical = match platform {
        "modrinth" => ProviderProjectId::Modrinth(
            empack_core::identity::ModrinthProjectId::parse(id).unwrap(),
        ),
        "curseforge" => ProviderProjectId::CurseForge(
            empack_core::identity::CurseForgeProjectId::parse(id).unwrap(),
        ),
        _ => unreachable!(),
    };
    let (key, record) = resolved
        .lock()
        .dependencies
        .iter()
        .find(|(_, dep)| dep.identity == ResolvedIdentity::Provider(canonical.clone()))
        .expect("expected canonical provider identity");
    assert!(resolved.intent().roots.contains_key(key));
    assert!(
        record
            .selected
            .as_ref()
            .is_some_and(|pin| pin.project == canonical)
    );
    let intent = std::fs::read(project.dir().join("empack.yml")).unwrap();
    let lock = std::fs::read(project.dir().join("empack.lock")).unwrap();
    for _ in 0..2 {
        run(&["--yes", "sync"]);
        assert_eq!(
            std::fs::read(project.dir().join("empack.yml")).unwrap(),
            intent
        );
        assert_eq!(
            std::fs::read(project.dir().join("empack.lock")).unwrap(),
            lock
        );
    }
    // Re-add through every spelling requires deliberate replacement and retains
    // one logical root for this canonical project. Refusal preserves all inputs.
    for selector in [id, url, slug] {
        let author_before = author_snapshot(project.dir());
        let denied = execute(
            &project,
            &["--yes", "add", "--platform", platform, selector],
            credential,
        );
        assert!(
            !denied.status.success(),
            "same-identity replacement needs --force"
        );
        assert_eq!(author_snapshot(project.dir()), author_before);
        run(&["--yes", "add", "--force", "--platform", platform, selector]);
        let updated = read_project(project.dir());
        assert_eq!(updated.intent().roots.len(), resolved.intent().roots.len());
        assert!(updated.intent().roots.contains_key(key));
        assert_eq!(
            updated.lock().dependencies[key].identity,
            ResolvedIdentity::Provider(canonical.clone())
        );
        assert!(
            updated.lock().dependencies[key]
                .selected
                .as_ref()
                .is_some_and(|pin| pin.project == canonical)
        );
        assert_eq!(
            updated
                .lock()
                .dependencies
                .values()
                .filter(|dep| dep.identity == ResolvedIdentity::Provider(canonical.clone()))
                .count(),
            1
        );
    }
    let before = author_snapshot(project.dir());
    let output = execute(
        &project,
        &[
            "--yes",
            "add",
            "--platform",
            platform,
            "empack-nonexistent-project-xyz12345",
        ],
        credential,
    );
    assert!(!output.status.success());
    assert_eq!(author_snapshot(project.dir()), before);
    // An authored alias is distinct from the provider slug/ID. Re-resolution and
    // forced re-add must keep one logical owner, then removal must select that owner.
    let old = format!("\n  {}:", key.as_str());
    let author_path = project.dir().join("empack.yml");
    let author = std::fs::read_to_string(&author_path).unwrap();
    assert_eq!(author.matches(&old).count(), 1);
    std::fs::write(&author_path, author.replacen(&old, "\n  custom-alias:", 1)).unwrap();
    let bindings_path = project.dir().join("empack.bindings.yml");
    if bindings_path.exists() {
        let bindings = std::fs::read_to_string(&bindings_path).unwrap();
        if bindings.contains(&old) {
            assert_eq!(bindings.matches(&old).count(), 1);
            std::fs::write(
                &bindings_path,
                bindings.replacen(&old, "\n  custom-alias:", 1),
            )
            .unwrap();
        }
    }
    run(&["--yes", "sync", "--materialize"]);
    run(&["--yes", "add", "--force", "--platform", platform, id]);
    let aliased = read_project(project.dir());
    let alias = empack_core::model::DependencyKey::parse("custom-alias").unwrap();
    assert!(aliased.intent().roots.contains_key(&alias));
    assert!(!aliased.lock().dependencies.contains_key(key));
    assert_eq!(
        aliased.lock().dependencies[&alias].identity,
        ResolvedIdentity::Provider(canonical.clone())
    );
    assert_eq!(
        aliased
            .lock()
            .dependencies
            .values()
            .filter(|dep| dep.identity == ResolvedIdentity::Provider(canonical.clone()))
            .count(),
        1
    );
    let before = author_snapshot(project.dir());
    for _ in 0..2 {
        run(&["--yes", "sync"]);
        assert_eq!(author_snapshot(project.dir()), before);
    }
    let query = format!("{platform}:{id}");
    let preview = run(&["--dry-run", "remove", &query]);
    let rendered = format!(
        "{}\n{}",
        String::from_utf8_lossy(&preview.stdout),
        String::from_utf8_lossy(&preview.stderr)
    );
    assert!(rendered.contains("Select custom-alias:"), "{rendered}");
    assert_eq!(author_snapshot(project.dir()), before);
    let result = run(&["--yes", "rm", &query]);
    let rendered = format!(
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        rendered.contains("Applied RemoveContent to 1 dependencies"),
        "{rendered}"
    );
    let removed = read_project(project.dir());
    assert!(!removed.intent().roots.contains_key(&alias));
    assert!(!removed.lock().dependencies.contains_key(&alias));
    for (key, dependency) in &aliased.lock().dependencies {
        if key != &alias {
            assert_eq!(&removed.lock().dependencies[key], dependency);
        }
    }
    let mut expected = before;
    for file in aliased.lock().dependencies[&alias].files.as_slice() {
        for placement in file.placements.as_slice() {
            let path = placement_path(placement);
            assert!(
                expected.remove(&path).is_some(),
                "selected installed path absent: {}",
                path.display()
            );
        }
    }
    for name in ["empack.yml", "empack.lock", "empack.bindings.yml"] {
        if project.dir().join(name).exists() {
            expected.insert(
                name.into(),
                std::fs::read(project.dir().join(name)).unwrap(),
            );
        } else {
            expected.remove(Path::new(name));
        }
    }
    assert_eq!(author_snapshot(project.dir()), expected);
    for _ in 0..2 {
        run(&["--yes", "sync"]);
        assert_eq!(author_snapshot(project.dir()), expected);
    }
}

#[test]
#[ignore = "requires live Modrinth catalog and downloads"]
fn live_cli_selected_update_preserves_exact_pin_and_authoring() {
    use empack_core::model::DependencySelection;
    let project = TestProject::workflow_fixture("provider-update", "fabric", "1.21.1");
    run(
        &project,
        &["--yes", "add", "--platform", "modrinth", "sodium"],
    );
    let initial = read_project(project.dir());
    let key = initial.intent().roots.keys().next().unwrap();
    let selected = initial.lock().dependencies[key].selected.clone().unwrap();
    run(&project, &["--yes", "pin", key.as_str()]);
    let pinned = read_project(project.dir());
    assert_eq!(
        pinned.intent().roots[key].selection,
        DependencySelection::ProviderPin(selected.selection.clone())
    );
    let assessment_before = author_snapshot(project.dir());
    let output = run(&project, &["outdated", "--json", "--strict"]);
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema"], 1);
    let rows = report["dependencies"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row["key"], key.as_str());
    assert_eq!(row["state"], "pinned");
    assert_eq!(row["reasons"], serde_json::json!(["pinned"]));
    assert_eq!(row["catalog_complete"], true);
    for column in ["current", "permitted"] {
        assert_eq!(row[column]["project"], selected.project.to_string());
        let empack_core::identity::PinSelector::ModrinthVersion(version) = &selected.selection
        else {
            panic!("expected selected Modrinth version")
        };
        assert_eq!(row[column]["selection"], version.as_str());
    }
    // Live catalog identities are observations, not hardcoded latest versions.
    for column in ["compatible", "upstream"] {
        assert_eq!(row[column]["project"], selected.project.to_string());
        assert!(!row[column]["selection"].as_str().unwrap().is_empty());
    }
    assert_eq!(report["joint"]["state"], "validated");
    assert_eq!(report["joint"]["declarations_complete"], true);
    assert_eq!(report["joint"]["candidate_compatibility"], "not-assessed");
    assert_eq!(author_snapshot(project.dir()), assessment_before);
    let before_preview = snapshot(project.dir(), &[]);
    let before = author_snapshot(project.dir());
    run(
        &project,
        &[
            "--yes",
            "--dry-run",
            "update",
            key.as_str(),
            "--release-policy",
            "any",
        ],
    );
    assert_eq!(
        snapshot(project.dir(), &[]),
        before_preview,
        "preview must not publish author changes"
    );
    let output = run(
        &project,
        &["--yes", "update", key.as_str(), "--release-policy", "any"],
    );
    let rendered = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        rendered.contains("0 selections changed, 1 retained"),
        "{rendered}"
    );
    let updated = read_project(project.dir());
    assert_eq!(updated.intent(), pinned.intent());
    assert_eq!(
        updated.lock().dependencies[key],
        pinned.lock().dependencies[key]
    );
    assert_eq!(
        updated.lock().dependencies[key].selected.as_ref(),
        Some(&selected)
    );
    assert_eq!(
        author_snapshot(project.dir()),
        before,
        "checking an exact pin must preserve author files and content"
    );
    for _ in 0..2 {
        run(&project, &["--yes", "sync"]);
        assert_eq!(author_snapshot(project.dir()), before);
    }
    // Unpinning is a policy edit, not an upgrade. The live catalog may already be
    // current; the frozen host journey separately proves an actual version change.
    run(&project, &["--yes", "unpin", key.as_str()]);
    let automatic = read_project(project.dir());
    assert_eq!(
        automatic.intent().roots[key].selection,
        DependencySelection::Automatic
    );
    assert_eq!(automatic.lock().dependencies, updated.lock().dependencies);
    assert_eq!(automatic.lock().runtime, updated.lock().runtime);
    assert_eq!(
        automatic.lock().required_edges,
        updated.lock().required_edges
    );
    let author = std::fs::read(project.dir().join("empack.yml")).unwrap();
    let before_preview = snapshot(project.dir(), &[]);
    run(
        &project,
        &[
            "--yes",
            "--dry-run",
            "update",
            key.as_str(),
            "--release-policy",
            "any",
        ],
    );
    assert_eq!(snapshot(project.dir(), &[]), before_preview);
    let output = run(
        &project,
        &["--yes", "update", key.as_str(), "--release-policy", "any"],
    );
    let resolved = read_project(project.dir());
    let changed = usize::from(
        resolved.lock().dependencies[key].selected != automatic.lock().dependencies[key].selected,
    );
    let rendered = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(rendered.contains(&format!("Checked 1 selected dependencies: {changed} selections changed, {} retained; preserved authored intent", 1 - changed)), "{rendered}");
    assert_eq!(resolved.intent(), automatic.intent());
    assert_eq!(
        std::fs::read(project.dir().join("empack.yml")).unwrap(),
        author
    );
    assert_eq!(resolved.lock().runtime, automatic.lock().runtime);
    run(&project, &["--yes", "sync", "--materialize"]);
    let materialized = read_project(project.dir());
    assert_eq!(
        materialized.lock().dependencies,
        resolved.lock().dependencies
    );
    use empack_core::digest::ExpectedDigest;
    use sha2::{Digest, Sha512};
    for file in materialized.lock().dependencies[key].files.as_slice() {
        let expected = file
            .expected
            .digests
            .as_ref()
            .unwrap()
            .values()
            .iter()
            .find(|digest| matches!(digest, ExpectedDigest::Sha512(_)))
            .expect("Modrinth selection declares SHA-512");
        for placement in file.placements.as_slice() {
            let bytes = std::fs::read(project.dir().join(placement_path(placement))).unwrap();
            assert_eq!(file.expected.size, Some(bytes.len() as u64));
            assert_eq!(
                &ExpectedDigest::Sha512(Sha512::digest(&bytes).into()),
                expected
            );
        }
    }
    let converged = author_snapshot(project.dir());
    for _ in 0..2 {
        run(&project, &["--yes", "sync"]);
        assert_eq!(author_snapshot(project.dir()), converged);
    }
}

// Reuse the already verified immutable payload; adoption observes the installed
// copies while the live provider routes establish identity, never a latest choice.
fn adopt_existing_live_content(bytes: &[u8], url: &str) {
    use empack_core::{
        digest::{ContentId, ExpectedDigest},
        identity::{ModrinthProjectId, ModrinthVersionId, PinSelector},
        model::SourceIntent,
    };
    use sha2::{Digest, Sha256, Sha512};
    use std::fs;
    for mode in ["url", "exact-provider", "identified-provider"] {
        let project = TestProject::workflow_fixture("untracked-adoption", "fabric", "1.21.1");
        fs::create_dir_all(project.dir().join("pack/mods")).unwrap();
        let installed = project.dir().join("pack/mods/morechathistory-1.3.1.jar");
        fs::write(project.dir().join("observed.jar"), bytes).unwrap();
        fs::write(
            project.dir().join("prior-build.zip"),
            b"Retain prior artifact",
        )
        .unwrap();
        let args = match mode {
            "url" => vec!["--yes", "adopt", "--from", url],
            "exact-provider" => vec![
                "--yes",
                "adopt",
                "--from",
                "8qkXwOnk",
                "--platform",
                "modrinth",
                "--version-id",
                "3siYJiWG",
            ],
            _ => vec![
                "--yes",
                "adopt",
                "--from",
                "observed.jar",
                "--platform",
                "modrinth",
            ],
        };
        // Adoption cannot install an absent target. Provider adoption must match
        // declared bytes; URL adoption instead observes a valid installed archive,
        // so its negative case supplies malformed content, not a remote assertion.
        for mismatch in [false, true] {
            if mismatch {
                fs::write(&installed, b"different installed bytes").unwrap();
            }
            let before = author_snapshot(project.dir());
            let refused = execute(&project, &args, None);
            assert!(
                !refused.status.success(),
                "{mode} unexpectedly accepted mismatching content"
            );
            let diagnostic = format!(
                "{}\n{}",
                String::from_utf8_lossy(&refused.stdout),
                String::from_utf8_lossy(&refused.stderr)
            );
            if mode != "url" {
                assert!(
                    diagnostic.contains(if mismatch {
                        "Source size differs from lock"
                    } else {
                        "Input is not a captured regular file"
                    }),
                    "{mode}: {diagnostic}"
                );
            } else if mismatch {
                assert!(diagnostic.contains("Inspect dependency"), "{diagnostic}");
            }
            assert_eq!(author_snapshot(project.dir()), before, "{mode}");
        }
        fs::write(&installed, bytes).unwrap();
        let attributes = |path: &Path| {
            let metadata = fs::metadata(path).unwrap();
            (
                metadata.modified().unwrap(),
                metadata.permissions().readonly(),
                metadata.len(),
            )
        };
        let source_attributes = attributes(&project.dir().join("observed.jar"));
        let installed_attributes = attributes(&installed);
        let before = snapshot(project.dir(), &[]);
        let mut preview = args.clone();
        preview.push("--dry-run");
        run(&project, &preview);
        assert_eq!(
            snapshot(project.dir(), &[]),
            before,
            "{mode} preview changed files"
        );
        let before = author_snapshot(project.dir());
        let output = run(&project, &args);
        let rendered = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            rendered.contains("Adopted 1 verified dependency bindings"),
            "{mode}: {rendered}"
        );
        let resolved = read_project(project.dir());
        assert_eq!(resolved.intent().roots.len(), 1);
        assert_eq!(resolved.lock().dependencies.len(), 1);
        let (key, dependency) = resolved.lock().dependencies.first_key_value().unwrap();
        if mode == "url" {
            assert!(
                matches!(&resolved.intent().roots[key].source, SourceIntent::Url(urls) if urls.as_slice() == [url])
            );
            assert!(dependency.selected.is_none());
            assert!(dependency.files.as_slice()[0].expected.digests.is_none());
        } else {
            let pin = dependency.selected.as_ref().unwrap();
            assert_eq!(
                pin.project,
                ProviderProjectId::Modrinth(ModrinthProjectId::parse("8qkXwOnk").unwrap())
            );
            assert_eq!(
                pin.selection,
                PinSelector::ModrinthVersion(ModrinthVersionId::parse("3siYJiWG").unwrap())
            );
            assert!(
                dependency.files.as_slice()[0]
                    .provenance
                    .declared_digests
                    .is_some()
            );
        }
        assert_eq!(dependency.files.as_slice().len(), 1);
        let file = &dependency.files.as_slice()[0];
        assert_eq!(file.expected.size, Some(bytes.len() as u64));
        if mode == "url" {
            assert_eq!(
                file.expected.accepted_observation,
                Some(ContentId::from_sha256(Sha256::digest(bytes).into()))
            );
            assert!(file.provenance.declared_digests.is_none());
        } else {
            // Provider evidence remains declared evidence, not a fabricated
            // accept-first-observation assertion merely because adoption ran.
            assert!(file.expected.accepted_observation.is_none());
            assert_eq!(file.expected.digests, file.provenance.declared_digests);
            assert!(
                file.expected
                    .digests
                    .as_ref()
                    .unwrap()
                    .values()
                    .contains(&ExpectedDigest::Sha512(Sha512::digest(bytes).into()))
            );
        }
        assert_eq!(attributes(&installed), installed_attributes);
        assert_eq!(
            attributes(&project.dir().join("observed.jar")),
            source_attributes
        );
        let mut after = author_snapshot(project.dir());
        let mut expected = before;
        for path in ["empack.yml", "empack.bindings.yml", "empack.lock"] {
            after.remove(Path::new(path));
            expected.remove(Path::new(path));
        }
        assert_eq!(after, expected, "{mode} changed non-document files");
        let stable = author_snapshot(project.dir());
        for _ in 0..2 {
            run(&project, &["--yes", "sync"]);
            assert_eq!(
                author_snapshot(project.dir()),
                stable,
                "{mode} did not converge"
            );
        }
    }
}

#[test]
#[ignore = "immutable direct HTTPS JAR from the pinned import corpus; live network required"]
fn live_cli_direct_url_retains_observed_content() {
    use empack_core::{digest::ContentId, model::SourceIntent};
    use sha2::{Digest, Sha256, Sha512};
    use std::fs;
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/import-corpus/fabulously-optimized-6.5.0.json"
    ))
    .unwrap();
    let selected = corpus["index"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["path"] == "mods/morechathistory-1.3.1.jar")
        .unwrap();
    let url = selected["downloads"][0].as_str().unwrap();
    let project = TestProject::workflow_fixture("direct-url", "fabric", "1.21.1");
    fs::write(project.dir().join("prior-build.zip"), b"prior artifact").unwrap();
    let before = snapshot(project.dir(), &[]);
    run(&project, &["--dry-run", "add", url]);
    assert_eq!(snapshot(project.dir(), &[]), before);
    run(&project, &["--yes", "add", url]);
    let bytes = fs::read(project.dir().join("pack/mods/morechathistory-1.3.1.jar")).unwrap();
    assert_eq!(bytes.len() as u64, selected["fileSize"].as_u64().unwrap());
    let digest = Sha512::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(digest, selected["hashes"]["sha512"].as_str().unwrap());
    let resolved = read_project(project.dir());
    assert_eq!(resolved.intent().roots.len(), 1);
    let (key, root) = resolved.intent().roots.first_key_value().unwrap();
    assert!(matches!(&root.source, SourceIntent::Url(urls) if urls.as_slice() == [url]));
    let dependency = &resolved.lock().dependencies[key];
    assert!(
        dependency.selected.is_none(),
        "a direct URL is not a provider pin"
    );
    let file = &dependency.files.as_slice()[0];
    assert!(file.expected.digests.is_none());
    assert!(file.provenance.declared_digests.is_none());
    assert_eq!(
        file.expected.accepted_observation,
        Some(ContentId::from_sha256(Sha256::digest(&bytes).into()))
    );
    let accepted = author_snapshot(project.dir());
    for _ in 0..2 {
        run(&project, &["--yes", "sync"]);
        assert_eq!(author_snapshot(project.dir()), accepted);
    }
    let report = run(&project, &["manifest", "check", "--json"]);
    let report: serde_json::Value = serde_json::from_slice(&report.stdout).unwrap();
    assert_eq!(
        report["dependencies"][0]["evidence_policy"],
        "accept-first-observation"
    );
    assert_eq!(
        fs::read(project.dir().join("prior-build.zip")).unwrap(),
        b"prior artifact"
    );
    adopt_existing_live_content(&bytes, url);
}
