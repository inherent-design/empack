//! Private crash fixtures selected from compiler discovery, never product controls.
use super::process::{file_sha256, offline_empack_cmd};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

pub struct Driver {
    path: PathBuf,
    digest: String,
}

impl Driver {
    pub fn from_inventory() -> Self {
        let inventory = PathBuf::from(
            std::env::var_os("EMPACK_TEST_INVENTORY")
                .expect("run publication fixtures through scripts/verify.py"),
        );
        assert!(inventory.is_absolute());
        assert_eq!(
            file_sha256(&inventory).unwrap(),
            std::env::var("EMPACK_TEST_INVENTORY_SHA256").unwrap()
        );
        let inventory: Value = serde_json::from_slice(&fs::read(&inventory).unwrap()).unwrap();
        let drivers: Vec<_> = inventory["rust-suites"]
            .as_object()
            .unwrap()
            .values()
            .filter(|suite| suite["package-name"] == "empack-lib" && suite["kind"] == "lib")
            .collect();
        assert_eq!(
            drivers.len(),
            1,
            "one compiler-discovered library test binary required"
        );
        let path = PathBuf::from(drivers[0]["binary-path"].as_str().unwrap());
        assert!(path.is_absolute() && path.is_file());
        let digest = file_sha256(&path).unwrap();
        let driver = Self { path, digest };
        let listing_root = tempfile::tempdir().unwrap();
        // Filtered Nextest inventories retain the compiled binary but omit its
        // unselected cases. Ask that exact binary to list each private fixture.
        for name in [
            "engine::publication::tests::crash_worker",
            "engine::publication::creation::tests::new_root_crash_child",
        ] {
            let output = driver
                .command(listing_root.path())
                .args(["--exact", name, "--list"])
                .assert()
                .success()
                .get_output()
                .stdout
                .clone();
            let output = String::from_utf8(output).unwrap();
            let names: Vec<_> = output
                .lines()
                .filter(|line| line.ends_with(": test"))
                .map(str::to_owned)
                .collect();
            assert_eq!(names, [format!("{name}: test")]);
            eprintln!("Publication crash fixture: {name}");
        }
        driver.verify();
        eprintln!(
            "Publication crash driver: {} SHA256={}",
            driver.path.display(),
            driver.digest
        );
        driver
    }

    pub fn command(&self, invocation: &Path) -> assert_cmd::Command {
        let environment = offline_empack_cmd(invocation);
        let mut command = assert_cmd::Command::new(&self.path);
        command
            .env_clear()
            .current_dir(invocation)
            .timeout(Duration::from_secs(45));
        for (key, value) in environment.get_envs() {
            if let Some(value) = value {
                command.env(key, value);
            }
        }
        command
    }

    pub fn verify(&self) {
        assert_eq!(
            file_sha256(&self.path).unwrap(),
            self.digest,
            "private fixture driver changed"
        );
    }
}
