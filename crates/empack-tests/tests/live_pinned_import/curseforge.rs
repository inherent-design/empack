//! Independent official metadata, with explicit user-supplied restricted bytes.
use super::{hex, override_inventory};
use empack_tests::harness::{
    archive::{zip_members_with_bounds, zip_members_with_limit},
    process::{TestProject, assert_dist_artifact_suffix, curseforge_key, empack_cmd, read_project},
    workspace::snapshot_excluding,
};
use serde_json::Value;
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha512};
use std::{collections::BTreeMap, fs, io::Read, path::Path, process::Output, time::Duration};

struct Commands {
    host: tempfile::TempDir,
    state: std::path::PathBuf,
    cache: std::path::PathBuf,
    key: String,
}
impl Commands {
    fn new() -> Self {
        let host = tempfile::tempdir().unwrap();
        let key = curseforge_key();
        assert!(!key.trim().is_empty(), "CurseForge credential is empty");
        Self {
            state: host.path().join("state"),
            cache: host.path().join("cache"),
            host,
            key,
        }
    }
    fn output(&self, invocation: &Path, root: &Path, args: &[&str]) -> Output {
        let mut command = empack_cmd(invocation);
        command
            .arg("--yes")
            .arg("--workdir")
            .arg(root)
            .arg("--state-dir")
            .arg(&self.state)
            .arg("--cache-dir")
            .arg(&self.cache)
            .env("EMPACK_KEY_CURSEFORGE", &self.key)
            .args(args);
        // Do not format Command: it contains an explicit credential override.
        let mut output = assert_cmd::Command::from_std(command)
            .timeout(Duration::from_secs(600))
            .output()
            .expect("spawn pinned import command");
        for stream in [&mut output.stdout, &mut output.stderr] {
            *stream = String::from_utf8_lossy(stream)
                .replace(&self.key, "[credential redacted]")
                .into_bytes();
        }
        output
    }
    fn run(&self, invocation: &Path, root: &Path, args: &[&str]) -> Output {
        let output = self.output(invocation, root, args);
        assert!(
            output.status.success(),
            "{args:?}: {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }
    fn pending(&self, invocation: &Path, target: &Path) -> Value {
        let before = snapshot_excluding(&self.state, &[]);
        let output = self.run(invocation, target, &["pending", "list", "--json"]);
        assert_eq!(snapshot_excluding(&self.state, &[]), before);
        let listed: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(listed["operations"].as_array().unwrap().len(), 1);
        let operation = listed["operations"][0].clone();
        assert_eq!(operation["kind"], "import");
        operation
    }
}

fn download(client: &reqwest::blocking::Client, url: &str, size: u64) -> Vec<u8> {
    assert!(size < 4 << 20);
    let response = client.get(url).send().unwrap().error_for_status().unwrap();
    let mut bytes = Vec::new();
    response.take(size + 1).read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes.len() as u64, size);
    bytes
}

fn selector(file: &Value) -> String {
    format!(
        "provider:curseforge:{}:{}:{}",
        file["project_id"],
        file["file_id"],
        file["destination"]
            .as_str()
            .unwrap()
            .rsplit('/')
            .next()
            .unwrap()
    )
}

fn restricted_inputs(output: &Output) -> Vec<String> {
    assert!(!output.status.success());
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(text.contains("Import was saved"), "{text}");
    let mut selectors: Vec<String> = text
        .lines()
        .filter_map(|line| {
            let (_, displayed) = line.split_once("Missing import file ")?;
            let value = serde_json::Deserializer::from_str(displayed);
            value.into_iter::<String>().next()?.ok()
        })
        .collect();
    assert!(!selectors.is_empty(), "{text}");
    selectors.sort();
    selectors
}

fn canonical_manifest(bytes: &[u8]) -> Value {
    let mut value: Value = serde_json::from_slice(bytes).unwrap();
    value["files"]
        .as_array_mut()
        .unwrap()
        .sort_by_key(|file| file["projectID"].as_u64().unwrap());
    value
}

#[test]
#[ignore = "pinned real CurseForge pack, 46 exact files and two explicitly supplied restricted inputs"]
fn live_pinned_curseforge_import_resumes_restricted_files_and_exports_inventory() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/import-corpus/fabulously-optimized-5.4.1-curseforge.json"
    ))
    .unwrap();
    let project = TestProject::new();
    let commands = Commands::new();
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .unwrap();
    let source = &fixture["source"];
    let bytes = download(
        &client,
        source["url"].as_str().unwrap(),
        source["size"].as_u64().unwrap(),
    );
    assert_eq!(hex(Sha256::digest(&bytes)), source["sha256"]);
    assert_eq!(hex(Sha512::digest(&bytes)), source["sha512"]);
    let archive = project.dir().join("original.zip");
    fs::write(&archive, &bytes).unwrap();
    let mut original = zip_members_with_limit(&archive, 128);
    assert_eq!(
        serde_json::from_slice::<Value>(&original.remove("manifest.json").unwrap()).unwrap(),
        fixture["manifest"]
    );
    let auxiliary = original.remove("modlist.html").unwrap();
    assert_eq!(
        auxiliary.len() as u64,
        fixture["auxiliary"]["modlist.html"]["size"]
    );
    assert_eq!(
        hex(Sha256::digest(&auxiliary)),
        fixture["auxiliary"]["modlist.html"]["sha256"]
    );
    assert_eq!(override_inventory(&original), fixture["overrides"]);
    assert_eq!(original.len(), 32);
    fs::create_dir(project.dir().join("empty author notes")).unwrap();
    fs::write(project.dir().join("prior.zip"), b"keep prior artifact").unwrap();
    let source_view = || snapshot_excluding(project.dir(), &[".harness-home"]);
    let target = project.dir().join("imported");
    let before = source_view();
    let output = commands.output(
        project.dir(),
        project.dir(),
        &[
            "init",
            "--from",
            archive.to_str().unwrap(),
            "--exclude-auxiliary",
            target.to_str().unwrap(),
        ],
    );
    let actual = restricted_inputs(&output);
    let restricted: Vec<_> = fixture["files"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|file| file["direct_download_available"] == false)
        .collect();
    let mut expected: Vec<_> = restricted.iter().map(|file| selector(file)).collect();
    expected.sort();
    assert_eq!(actual, expected);
    assert_eq!(restricted.len(), 2);
    assert_eq!(source_view(), before);
    assert!(!target.exists());
    let pending = commands.pending(project.dir(), &target);
    let wrong = commands.host.path().join("wrong.bin");
    fs::write(
        &wrong,
        vec![0; restricted[0]["size"].as_u64().unwrap() as usize],
    )
    .unwrap();
    let association = format!("{}={}", selector(restricted[0]), wrong.display());
    let retained = snapshot_excluding(&commands.state, &[]);
    let output = commands.output(
        project.dir(),
        &target,
        &[
            "init",
            "--continue",
            "--exclude-auxiliary",
            "--import-file",
            &association,
        ],
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("digest mismatch"));
    assert!(!target.exists());
    assert_eq!(source_view(), before);
    assert_eq!(snapshot_excluding(&commands.state, &[]), retained);
    assert_eq!(commands.pending(project.dir(), &target), pending);

    // The fixture acts as the user, supplying exact files from separately pinned
    // public Modrinth releases. Empack must not invent this provider fallback.
    let mut supplied = Vec::new();
    for file in &restricted {
        let source = &file["supplied_source"];
        let bytes = download(
            &client,
            source["url"].as_str().unwrap(),
            source["size"].as_u64().unwrap(),
        );
        assert_eq!(hex(Sha1::digest(&bytes)), file["sha1"]);
        assert_eq!(hex(Sha512::digest(&bytes)), source["sha512"]);
        let path = commands
            .host
            .path()
            .join(format!("renamed-{}.bin", file["file_id"]));
        fs::write(&path, bytes).unwrap();
        supplied.push(format!("{}={}", selector(file), path.display()));
    }
    fs::remove_file(&archive).unwrap();
    let before_resume = source_view();
    let output = commands.output(
        project.dir(),
        &target,
        &[
            "init",
            "--continue",
            "--exclude-auxiliary",
            "--import-file",
            &supplied[0],
        ],
    );
    assert_eq!(restricted_inputs(&output), [selector(restricted[1])]);
    assert!(!target.exists());
    assert_eq!(source_view(), before_resume);
    // Successful partial acquisition is retained even after the user's selected
    // file and the original archive disappear. Continue with only the other file.
    let retained = commands.pending(project.dir(), &target);
    assert!(
        retained["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|input| input["key"] == supplied[0].split_once('=').unwrap().0)
    );
    let first_source = supplied[0].split_once('=').unwrap().1;
    fs::remove_file(first_source).unwrap();
    commands.run(
        project.dir(),
        &target,
        &[
            "init",
            "--continue",
            "--exclude-auxiliary",
            "--import-file",
            &supplied[1],
        ],
    );
    // Importing creates the requested root only; existing author work survives.
    assert_eq!(
        snapshot_excluding(project.dir(), &[".harness-home", "imported"]),
        before_resume
    );
    let observe = || snapshot_excluding(&target, &["dist", ".harness-home"]);
    fs::create_dir_all(target.join("notes/empty")).unwrap();
    fs::write(target.join("notes/keep.txt"), b"author decision").unwrap();
    let author = fs::read(target.join("empack.yml")).unwrap();
    let lock = fs::read(target.join("empack.lock")).unwrap();
    for i in 0..2 {
        let before = (i == 1).then(observe);
        commands.run(project.dir(), &target, &["sync"]);
        assert_eq!(fs::read(target.join("empack.yml")).unwrap(), author);
        assert_eq!(fs::read(target.join("empack.lock")).unwrap(), lock);
        if let Some(before) = before {
            assert_eq!(observe(), before);
        }
    }
    let resolved = read_project(&target);
    for file in fixture["files"].as_array().unwrap() {
        let path = file["destination"].as_str().unwrap();
        let size = file["size"].as_u64().unwrap();
        assert!(size <= 8 << 20);
        let mut payload = Vec::new();
        fs::File::open(target.join("pack").join(path))
            .unwrap()
            .take(size + 1)
            .read_to_end(&mut payload)
            .unwrap();
        assert_eq!(payload.len() as u64, size);
        assert_eq!(hex(Sha1::digest(&payload)), file["sha1"], "{path}");
        let placements: Vec<_> = resolved
            .lock()
            .dependencies
            .values()
            .flat_map(|d| d.files.as_slice())
            .flat_map(|f| f.placements.as_slice())
            .filter(|p| p.destination.relative().as_str() == path)
            .collect();
        assert_eq!(placements.len(), 1, "{path}");
        assert_eq!(
            placements[0].requirements.client,
            empack_core::requirements::Requirement::Required
        );
        assert_eq!(
            placements[0].requirements.server,
            empack_core::requirements::Requirement::Required
        );
    }
    commands.run(project.dir(), &target, &["build", "--ad-hoc", "curseforge"]);
    let exported = assert_dist_artifact_suffix(&target, "curseforge.zip");
    let mut output = zip_members_with_limit(&exported, 256);
    assert_eq!(
        canonical_manifest(&output.remove("manifest.json").unwrap()),
        canonical_manifest(&serde_json::to_vec(&fixture["manifest"]).unwrap())
    );
    assert_eq!(output, original);
    commands.run(
        project.dir(),
        &target,
        &["build", "--ad-hoc", "prism", "--delivery", "bundled"],
    );
    let bundled = assert_dist_artifact_suffix(&target, "prism-bundled.zip");
    let mut output = zip_members_with_bounds(&bundled, 256, 8 << 20, 32 << 20);
    for file in fixture["files"].as_array().unwrap() {
        let path = format!(".minecraft/{}", file["destination"].as_str().unwrap());
        let bytes = output
            .remove(&path)
            .unwrap_or_else(|| panic!("missing {path}"));
        assert_eq!(bytes.len() as u64, file["size"]);
        assert_eq!(hex(Sha1::digest(&bytes)), file["sha1"], "{path}");
    }
    for (path, bytes) in &original {
        assert_eq!(
            output
                .remove(&format!(
                    ".minecraft/{}",
                    path.strip_prefix("overrides/").unwrap()
                ))
                .unwrap(),
            *bytes
        );
    }
    assert_eq!(
        output.keys().map(String::as_str).collect::<Vec<_>>(),
        ["instance.cfg", "mmc-pack.json"]
    );
    let runtime: Value = serde_json::from_slice(&output["mmc-pack.json"]).unwrap();
    let components: BTreeMap<_, _> = runtime["components"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| (v["uid"].as_str().unwrap(), v["version"].as_str().unwrap()))
        .collect();
    assert_eq!(
        components,
        BTreeMap::from([
            ("net.fabricmc.fabric-loader", "0.14.23"),
            ("net.minecraft", "1.20.1")
        ])
    );
    assert_eq!(fs::read(target.join("empack.yml")).unwrap(), author);
    assert_eq!(fs::read(target.join("empack.lock")).unwrap(), lock);
    let prior_export = fs::read(&exported).unwrap();
    let prior_bundle = fs::read(&bundled).unwrap();
    let relative = "config/yosbr/options.txt";
    let path = target.join("overrides/common").join(relative);
    let original_options = fs::read_to_string(&path).unwrap();
    assert_eq!(original_options.matches("autoJump:false").count(), 1);
    let edited = original_options
        .replace("autoJump:false", "autoJump:true")
        .into_bytes();
    fs::write(&path, &edited).unwrap();
    let before = observe();
    let refused = commands.output(project.dir(), &target, &["sync"]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("Source size differs from lock"));
    assert_eq!(observe(), before);
    assert_eq!(fs::read(&exported).unwrap(), prior_export);
    assert_eq!(fs::read(&bundled).unwrap(), prior_bundle);
    commands.run(
        project.dir(),
        &target,
        &["adopt", &format!("CommonOverride:{relative}")],
    );
    commands.run(project.dir(), &target, &["sync"]);
    commands.run(project.dir(), &target, &["build", "--ad-hoc", "curseforge"]);
    let mut refreshed = zip_members_with_limit(&exported, 256);
    assert_eq!(
        canonical_manifest(&refreshed.remove("manifest.json").unwrap()),
        canonical_manifest(&serde_json::to_vec(&fixture["manifest"]).unwrap())
    );
    // The selected references/runtime are unchanged; only the accepted source changes.
    let mut expected = original;
    expected.insert(format!("overrides/{relative}"), edited.clone());
    assert_eq!(refreshed, expected);
    assert_eq!(fs::read(&bundled).unwrap(), prior_bundle);
    assert_eq!(fs::read(&path).unwrap(), edited);
    assert_eq!(fs::read(target.join("empack.yml")).unwrap(), author);
    let accepted = observe();
    assert!(accepted.files.keys().eq(before.files.keys()));
    assert!(accepted.entries.keys().eq(before.entries.keys()));
    for (path, bytes) in before.files {
        if path != Path::new("empack.lock") {
            assert_eq!(accepted.files.get(&path), Some(&bytes), "{path:?}");
        }
    }
    for (path, attributes) in before.entries {
        if path != Path::new("empack.lock") {
            assert_eq!(accepted.entries.get(&path), Some(&attributes), "{path:?}");
        }
    }
}
