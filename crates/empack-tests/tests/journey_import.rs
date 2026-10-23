use empack_tests::harness::process::{TestProject, offline_command, write_local_mrpack};

#[test]
fn journey_local_mrpack_import_reexport() {
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

    let output = offline_command(project.dir())
        .args([
            "init",
            "--from",
            mrpack_path.to_str().unwrap(),
            "--yes",
            "--import-optional-default",
            "true",
            "--exclude-auxiliary",
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
    let decoded = empack_lib::testing::read_authoring(&pack_dir.join("empack.yml")).unwrap();
    assert_eq!(
        decoded.intent().runtime.loader,
        empack_core::model::LoaderKind::Fabric
    );
    assert_eq!(
        decoded
            .intent()
            .runtime
            .loader_version
            .as_ref()
            .unwrap()
            .as_str(),
        "0.15.11"
    );
    assert!(
        pack_dir.join("empack.lock").is_file(),
        "native lock not found after local import"
    );

    let build_output = offline_command(&pack_dir)
        .args([
            "--yes",
            "build",
            "modrinth",
            "--allow-optional-metadata-loss",
        ])
        .output()
        .expect("failed to spawn empack build modrinth");

    assert!(
        build_output.status.success(),
        "empack build modrinth failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&build_output.stdout),
        String::from_utf8_lossy(&build_output.stderr),
    );

    let dist = pack_dir.join("dist");
    assert!(dist.is_dir(), "dist/ directory not found after build");

    let imported_mrpack = dist.join("local-fixture-pack-2.1.0.mrpack");
    assert!(
        imported_mrpack.exists(),
        "no imported .mrpack file found in dist/"
    );
}
