//! Embed an explicitly built, source-bound bootstrap; Cargo never builds Java or downloads it.
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, env, fs, io::Read, path::Path};

fn digest(path: &Path) -> String {
    digest_bytes(&fs::read(path).expect("Read bootstrap input"))
}

fn digest_bytes(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let jar = root.join("bootstrap/target/empack-bootstrap.jar");
    let manifest = jar.with_extension("json");
    println!(
        "cargo:rerun-if-changed={}",
        root.join("bootstrap/src/main").display()
    );
    let record: serde_json::Value = serde_json::from_slice(
        &fs::read(&manifest)
            .expect("Build the bootstrap before Cargo: python3 scripts/build-bootstrap.py"),
    )
    .expect("Read bootstrap build identity");
    let mut names = vec![
        root.join("bootstrap/pom.xml"),
        root.join("bootstrap/mise.toml"),
    ];
    fn collect(dir: &Path, names: &mut Vec<std::path::PathBuf>) {
        for entry in fs::read_dir(dir).expect("Read bootstrap source directory") {
            let entry = entry.expect("Read bootstrap source");
            assert!(
                !entry.file_type().unwrap().is_symlink(),
                "Bootstrap sources cannot be symlinks"
            );
            if entry.file_type().unwrap().is_dir() {
                collect(&entry.path(), names);
            } else {
                names.push(entry.path());
            }
        }
    }
    collect(&root.join("bootstrap/src/main"), &mut names);
    let mut sources = BTreeMap::new();
    for path in names {
        println!("cargo:rerun-if-changed={}", path.display());
        sources.insert(
            path.strip_prefix(&root)
                .unwrap()
                .to_str()
                .unwrap()
                .replace('\\', "/"),
            digest(&path),
        );
    }
    for path in [&jar, &manifest] {
        println!("cargo:rerun-if-changed={}", path.display());
        assert!(
            fs::symlink_metadata(path).unwrap().is_file(),
            "Bootstrap build output must be a regular file"
        );
    }
    assert_eq!(record["schema"], 1, "Unsupported bootstrap build identity");
    if env!("CARGO_PKG_VERSION") != "0.0.0-dev" {
        assert_eq!(
            record["version"],
            env!("CARGO_PKG_VERSION"),
            "Rebuild bootstrap for the stamped native version"
        );
    }
    assert_eq!(
        record["sources"],
        serde_json::to_value(sources).unwrap(),
        "Bootstrap source changed; rebuild it"
    );
    let mut bytes = Vec::new();
    fs::File::open(&jar)
        .unwrap()
        .take((8 << 20) + 1)
        .read_to_end(&mut bytes)
        .unwrap();
    assert!(
        !bytes.is_empty() && bytes.len() <= 8 << 20,
        "Bootstrap JAR exceeds its byte allowance"
    );
    let sha256 = digest_bytes(&bytes);
    assert_eq!(
        record["jar_sha256"], sha256,
        "Bootstrap bytes differ from their build identity"
    );
    fs::write(
        Path::new(&env::var_os("OUT_DIR").unwrap()).join("empack-bootstrap.jar"),
        bytes,
    )
    .unwrap();
    println!("cargo:rustc-env=EMPACK_BOOTSTRAP_SHA256={sha256}");
}
