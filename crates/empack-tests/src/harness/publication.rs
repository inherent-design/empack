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
        for name in [
            "engine::publication::tests::crash_worker",
            "engine::publication::creation::tests::new_root_crash_child",
        ] {
            assert!(
                drivers[0]["testcases"]
                    .as_object()
                    .unwrap()
                    .contains_key(name),
                "missing private driver {name}"
            );
        }
        let digest = file_sha256(&path).unwrap();
        eprintln!(
            "Publication crash driver: {} SHA256={digest}",
            path.display()
        );
        Self { path, digest }
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
