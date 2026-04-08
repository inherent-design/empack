use empack_tests::e2e::{TestProject, empack_cmd, write_local_mrpack};

const LIVE_IMPORTED_MRPACK_BUILD_TIMEOUT_SECS: &str = "600";

/// Download a file via HTTP to a local path using reqwest blocking.
fn download_file(url: &str, dest: &std::path::Path) {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .expect("build reqwest client");
    let resp = client
        .get(url)
        .send()
        .unwrap_or_else(|e| panic!("failed to download {}: {}", url, e));
    assert!(
        resp.status().is_success(),
        "HTTP {} for {}",
        resp.status(),
        url
    );
    let bytes = resp.bytes().expect("failed to read response body");
    std::fs::write(dest, &bytes)
        .unwrap_or_else(|e| panic!("failed to write {}: {}", dest.display(), e));
}

fn download_featured_modrinth_mrpack(project_id: &str, dest: &std::path::Path) {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .expect("build reqwest client");
    let resp = client
        .get(format!(
            "https://api.modrinth.com/v2/project/{project_id}/version?featured=true"
        ))
        .send()
        .unwrap_or_else(|e| panic!("failed to query Modrinth versions for {project_id}: {e}"));
    assert!(
        resp.status().is_success(),
        "HTTP {} for Modrinth project {}",
        resp.status(),
        project_id
    );

    let versions: serde_json::Value = resp
        .json()
        .expect("failed to parse Modrinth versions response");
    let download_url = versions
        .as_array()
        .and_then(|versions| versions.first())
        .and_then(|version| version.get("files"))
        .and_then(|files| files.as_array())
        .and_then(|files| {
            files
                .iter()
                .find(|file| {
                    file.get("filename")
                        .and_then(|filename| filename.as_str())
                        .is_some_and(|filename| filename.ends_with(".mrpack"))
                })
                .or_else(|| {
                    files.iter().find(|file| {
                        file.get("primary")
                            .and_then(|primary| primary.as_bool())
                            .unwrap_or(false)
                    })
                })
        })
        .and_then(|file| file.get("url"))
        .and_then(|url| url.as_str())
        .expect("no downloadable mrpack URL in Modrinth versions response");

    download_file(download_url, dest);
}

#[test]
fn e2e_import_modrinth_and_build_mrpack() {
    empack_tests::skip_if_no_java!();

    let project = TestProject::new();
    let mrpack_path = project.dir().join("fabulously-optimized.mrpack");

    download_featured_modrinth_mrpack("1KVo5zza", &mrpack_path);

    let output = empack_cmd(project.dir())
        .args([
            "init",
            "--from",
            mrpack_path.to_str().unwrap(),
            "--yes",
            "imported-pack",
        ])
        .output()
        .expect("failed to spawn empack init --from");

    assert!(
        output.status.success(),
        "empack init --from failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let pack_dir = project.dir().join("imported-pack");
    let config =
        std::fs::read_to_string(pack_dir.join("empack.yml")).expect("failed to read empack.yml");
    assert!(
        config.contains("name: Fabulously Optimized") || config.contains("Fabulously Optimized"),
        "empack.yml should contain 'Fabulously Optimized'\n{config}"
    );

    assert!(
        pack_dir.join("pack").join("pack.toml").exists(),
        "pack/pack.toml not found after import"
    );

    let build_output = empack_cmd(&pack_dir)
        .env(
            "EMPACK_PROCESS_TIMEOUT_SECS",
            LIVE_IMPORTED_MRPACK_BUILD_TIMEOUT_SECS,
        )
        .args(["build", "mrpack"])
        .output()
        .expect("failed to spawn empack build mrpack");

    assert!(
        build_output.status.success(),
        "empack build mrpack failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&build_output.stdout),
        String::from_utf8_lossy(&build_output.stderr),
    );

    let dist = pack_dir.join("dist");
    assert!(dist.is_dir(), "dist/ directory not found after build");

    let has_mrpack = std::fs::read_dir(&dist)
        .expect("failed to read dist/")
        .filter_map(Result::ok)
        .any(|entry| entry.path().extension().is_some_and(|ext| ext == "mrpack"));
    assert!(has_mrpack, "no .mrpack file found in dist/");
}

#[test]
fn e2e_import_local_mrpack_and_build_mrpack() {
    empack_tests::skip_if_no_java!();

    let project = TestProject::new();
    let mrpack_path = project.dir().join("local-fixture.mrpack");

    write_local_mrpack(
        &mrpack_path,
        "local-fixture-pack",
        "2.1.0",
        "1.21.1",
        "fabric-loader",
        "0.15.11",
    )
    .expect("failed to create local mrpack fixture");

    let output = empack_cmd(project.dir())
        .args([
            "init",
            "--from",
            mrpack_path.to_str().unwrap(),
            "--yes",
            "imported-pack",
        ])
        .output()
        .expect("failed to spawn empack init --from");

    assert!(
        output.status.success(),
        "empack init --from failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let pack_dir = project.dir().join("imported-pack");
    let config =
        std::fs::read_to_string(pack_dir.join("empack.yml")).expect("failed to read empack.yml");
    assert!(
        config.contains("name: local-fixture-pack"),
        "empack.yml should import the mrpack pack name\n{config}"
    );
    assert!(
        config.contains("loader: fabric"),
        "empack.yml should import the mrpack loader\n{config}"
    );
    assert!(
        pack_dir.join("pack").join("pack.toml").exists(),
        "pack/pack.toml not found after local import"
    );

    let build_output = empack_cmd(&pack_dir)
        .args(["build", "mrpack"])
        .output()
        .expect("failed to spawn empack build mrpack");

    assert!(
        build_output.status.success(),
        "empack build mrpack failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&build_output.stdout),
        String::from_utf8_lossy(&build_output.stderr),
    );

    let dist = pack_dir.join("dist");
    assert!(dist.is_dir(), "dist/ directory not found after build");

    let imported_mrpack = dist.join("local-fixture-pack-v2.1.0.mrpack");
    assert!(
        imported_mrpack.exists(),
        "no imported .mrpack file found in dist/"
    );
}

#[test]
fn e2e_import_curseforge_and_check_restricted() {
    empack_tests::skip_if_no_packwiz!();
    empack_tests::skip_if_no_java!();
    empack_tests::skip_if_no_cf_key!();

    let cf_key = std::env::var("EMPACK_KEY_CURSEFORGE").expect("CurseForge key");

    let client = reqwest::blocking::Client::new();

    let files_resp = client
        .get("https://api.curseforge.com/v1/mods/835044/files?gameVersion=1.20.1&pageSize=1")
        .header("x-api-key", &cf_key)
        .send()
        .expect("failed to query CF files API");

    assert!(
        files_resp.status().is_success(),
        "CF files API returned {}",
        files_resp.status()
    );

    let files_json: serde_json::Value = files_resp
        .json()
        .expect("failed to parse CF files response");

    let file_id = files_json["data"][0]["id"]
        .as_u64()
        .expect("no file ID in CF response");

    let dl_resp = client
        .get(format!(
            "https://api.curseforge.com/v1/mods/835044/files/{}/download-url",
            file_id
        ))
        .header("x-api-key", &cf_key)
        .send()
        .expect("failed to query CF download URL");

    assert!(
        dl_resp.status().is_success(),
        "CF download-url API returned {}",
        dl_resp.status()
    );

    let dl_json: serde_json::Value = dl_resp
        .json()
        .expect("failed to parse CF download-url response");

    let download_url = dl_json["data"]
        .as_str()
        .expect("no download URL in CF response");

    let project = TestProject::new();
    let zip_path = project.dir().join("cobblemon-updated.zip");
    download_file(download_url, &zip_path);

    let output = empack_cmd(project.dir())
        .args([
            "init",
            "--from",
            zip_path.to_str().unwrap(),
            "--yes",
            "cf-imported",
        ])
        .output()
        .expect("failed to spawn empack init --from (CF)");

    assert!(
        output.status.success(),
        "empack init --from (CF) failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let pack_dir = project.dir().join("cf-imported");
    assert!(
        pack_dir.join("empack.yml").exists(),
        "empack.yml not found after CF import"
    );

    let build_output = empack_cmd(&pack_dir)
        .args(["build", "client-full"])
        .output()
        .expect("failed to spawn empack build client-full");

    let stdout = String::from_utf8_lossy(&build_output.stdout);
    let stderr = String::from_utf8_lossy(&build_output.stderr);
    let combined = format!("{stdout}{stderr}");

    if !build_output.status.success() {
        assert!(
            combined.contains("require manual download")
                || combined.contains("excluded from the CurseForge API")
                || combined.contains("restricted"),
            "build failed without a restricted-mod message:\n{combined}"
        );
    }
    // If exit == 0: the pack built successfully, which is also acceptable
    // since CurseForge restriction status can change over time.
}

#[test]
fn e2e_init_from_curseforge_url() {
    empack_tests::skip_if_no_packwiz!();

    let project = TestProject::new();
    let output = empack_cmd(project.dir())
        .args([
            "init",
            "--from",
            "https://www.curseforge.com/minecraft/modpacks/cobblemon-updated",
            "--yes",
            "cf-url-imported",
        ])
        .output()
        .expect("failed to spawn empack init --from (CF URL)");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "empack init --from CF URL failed:\nstdout: {stdout}\nstderr: {stderr}",
    );

    let pack_dir = project.dir().join("cf-url-imported");
    assert!(
        pack_dir.join("empack.yml").exists(),
        "empack.yml not found after CF URL import"
    );
    assert!(
        pack_dir.join("pack").join("pack.toml").exists(),
        "pack/pack.toml not found after CF URL import"
    );
}
