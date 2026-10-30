//! Opt-in native fixtures own only disposable images, never caller-selected volumes.
use anyhow::{Context, Result, ensure};
use assert_cmd::Command;
use serde_json::Value;
use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    time::Duration,
};

fn run(program: &str, args: &[&std::ffi::OsStr], input: Option<Vec<u8>>) -> Result<Vec<u8>> {
    let mut command = Command::new(program);
    command.args(args).timeout(Duration::from_secs(90));
    if let Some(bytes) = input {
        command.write_stdin(bytes);
    }
    let output = command.output()?;
    ensure!(
        output.status.success(),
        "{program} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    ensure!(
        output.stdout.len() <= 1 << 20,
        "Native volume metadata exceeds fixture limit"
    );
    Ok(output.stdout)
}

fn plist(program: &str, args: &[&std::ffi::OsStr]) -> Result<Value> {
    let bytes = run(program, args, None)?;
    let json = run(
        "/usr/bin/plutil",
        &[
            "-convert".as_ref(),
            "json".as_ref(),
            "-o".as_ref(),
            "-".as_ref(),
            "-".as_ref(),
        ],
        Some(bytes),
    )?;
    Ok(serde_json::from_slice(&json)?)
}

pub struct ApfsFixture {
    directory: Option<tempfile::TempDir>,
    mount: PathBuf,
    blocker_mount: PathBuf,
    images: [PathBuf; 2],
    finished: bool,
}

impl ApfsFixture {
    pub fn new() -> Result<Self> {
        let directory = tempfile::Builder::new()
            .prefix("empack-apfs-verification-")
            .tempdir()?;
        let root = directory.path().canonicalize()?;
        let fixture = Self {
            directory: Some(directory),
            mount: root.join("volume"),
            blocker_mount: root.join("blocker"),
            images: [
                root.join("subject.sparseimage"),
                root.join("blocker.sparseimage"),
            ],
            finished: false,
        };
        fs::create_dir(&fixture.mount)?;
        fs::create_dir(&fixture.blocker_mount)?;
        for image in &fixture.images {
            run(
                "/usr/bin/hdiutil",
                &[
                    "create".as_ref(),
                    "-quiet".as_ref(),
                    "-size".as_ref(),
                    "128m".as_ref(),
                    "-fs".as_ref(),
                    "APFS".as_ref(),
                    "-type".as_ref(),
                    "SPARSE".as_ref(),
                    "-volname".as_ref(),
                    "EmpackVerification".as_ref(),
                    image.as_os_str(),
                ],
                None,
            )?;
        }
        fixture.attach(0, &fixture.mount)?;
        Ok(fixture)
    }

    pub fn root(&self) -> &Path {
        &self.mount
    }

    fn attach(&self, image: usize, mount: &Path) -> Result<()> {
        run(
            "/usr/bin/hdiutil",
            &[
                "attach".as_ref(),
                "-quiet".as_ref(),
                "-nobrowse".as_ref(),
                "-noautoopen".as_ref(),
                "-mountpoint".as_ref(),
                mount.as_os_str(),
                self.images[image].as_os_str(),
            ],
            None,
        )?;
        ensure!(
            self.owned()?
                .iter()
                .any(|(path, _)| path == &self.images[image]),
            "Image attachment was not observed"
        );
        Ok(())
    }

    // Requery ownership before detaching. A cached device number is never authority
    // to detach a disk that may since have been assigned to another image.
    fn owned(&self) -> Result<Vec<(PathBuf, String)>> {
        let info = plist("/usr/bin/hdiutil", &["info".as_ref(), "-plist".as_ref()])?;
        let mut result = Vec::new();
        for image in info["images"]
            .as_array()
            .context("Missing native image inventory")?
        {
            let Some(path) = image["image-path"].as_str().map(PathBuf::from) else {
                continue;
            };
            if !self.images.contains(&path) {
                continue;
            }
            let device = image["system-entities"]
                .as_array()
                .and_then(|rows| rows.iter().find_map(|row| row["dev-entry"].as_str()))
                .context("Owned image lacks a device")?;
            ensure!(
                device.starts_with("/dev/disk"),
                "Unexpected native image device"
            );
            result.push((path, device.into()));
        }
        Ok(result)
    }

    fn detach(&self, image: usize) -> Result<()> {
        let device = self
            .owned()?
            .into_iter()
            .find(|(path, _)| path == &self.images[image])
            .context("Owned image disappeared before detach")?
            .1;
        run(
            "/usr/bin/hdiutil",
            &["detach".as_ref(), device.as_ref()],
            None,
        )?;
        ensure!(
            !self
                .owned()?
                .iter()
                .any(|(path, _)| path == &self.images[image]),
            "Owned image remains attached"
        );
        Ok(())
    }

    fn uuid(&self) -> Result<String> {
        let info = plist(
            "/usr/sbin/diskutil",
            &["info".as_ref(), "-plist".as_ref(), self.mount.as_os_str()],
        )?;
        Ok(info["VolumeUUID"]
            .as_str()
            .context("APFS volume UUID unavailable")?
            .to_owned())
    }

    /// Force real device reassignment while keeping volume, inode and birth identity.
    pub fn remount(&self) -> Result<()> {
        let before = fs::metadata(&self.mount)?;
        let uuid = self.uuid()?;
        if self
            .owned()?
            .iter()
            .any(|(path, _)| path == &self.images[1])
        {
            // Release the earlier device before the subject, so the subject
            // can take it on the next remount without allocating more images.
            self.detach(1)?;
            self.detach(0)?;
        } else {
            self.detach(0)?;
            self.attach(1, &self.blocker_mount)?;
        }
        self.attach(0, &self.mount)?;
        let after = fs::metadata(&self.mount)?;
        ensure!(
            before.dev() != after.dev(),
            "Experiment did not reassign the device number"
        );
        ensure!(
            before.ino() == after.ino()
                && before.created()? == after.created()?
                && uuid == self.uuid()?,
            "Remount changed persistent root identity"
        );
        eprintln!(
            "APFS remount: device {} -> {}; inode {} and volume {} retained",
            before.dev(),
            after.dev(),
            before.ino(),
            uuid
        );
        Ok(())
    }

    fn cleanup(&self) -> Result<()> {
        let mut failures = Vec::new();
        for image in (0..self.images.len()).rev() {
            if self
                .owned()?
                .iter()
                .any(|(path, _)| path == &self.images[image])
                && let Err(error) = self.detach(image)
            {
                failures.push(format!("{error:#}"));
            }
        }
        ensure!(
            failures.is_empty(),
            "Image cleanup failed: {}",
            failures.join("; ")
        );
        ensure!(self.owned()?.is_empty(), "Fixture images remain attached");
        Ok(())
    }

    fn preserve(&mut self) {
        if let Some(directory) = self.directory.take() {
            eprintln!(
                "Native fixture retained after cleanup failure: {}",
                directory.keep().display()
            );
        }
    }

    pub fn finish(mut self) -> Result<()> {
        let result = self.cleanup();
        if result.is_err() {
            self.preserve();
        }
        self.finished = true;
        result
    }
}

impl Drop for ApfsFixture {
    fn drop(&mut self) {
        if !self.finished
            && let Err(error) = self.cleanup()
        {
            eprintln!("Native fixture cleanup failed: {error:#}");
            self.preserve();
        }
    }
}
