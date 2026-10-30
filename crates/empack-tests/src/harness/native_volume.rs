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

const DETACH_ATTEMPTS: usize = 5;

#[derive(Debug)]
struct CommandFailure {
    program: String,
    status: std::process::ExitStatus,
    stderr: String,
}
impl std::fmt::Display for CommandFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} failed ({}): {}",
            self.program, self.status, self.stderr
        )
    }
}
impl std::error::Error for CommandFailure {}

fn is_busy(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<CommandFailure>()
        .is_some_and(|failure| {
            failure.program == "/usr/bin/hdiutil"
                && failure.status.code().is_some_and(|code| {
                    std::io::Error::from_raw_os_error(code).kind()
                        == std::io::ErrorKind::ResourceBusy
                })
        })
}

fn run(program: &str, args: &[&std::ffi::OsStr], input: Option<Vec<u8>>) -> Result<Vec<u8>> {
    let mut command = Command::new(program);
    command.args(args).timeout(Duration::from_secs(90));
    if let Some(bytes) = input {
        command.write_stdin(bytes);
    }
    let output = command.output()?;
    if !output.status.success() {
        return Err(CommandFailure {
            program: program.into(),
            status: output.status,
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
        .into());
    }
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
        self.detach_with_busy(image, || Ok(()))
    }

    fn detach_with_busy(
        &self,
        image: usize,
        mut on_busy: impl FnMut() -> Result<()>,
    ) -> Result<()> {
        for attempt in 1..=DETACH_ATTEMPTS {
            // Every request is authorized by a fresh backing-to-device mapping.
            // A device returned by a failed attempt is never reused as authority.
            let device = self
                .owned()?
                .into_iter()
                .find(|(path, _)| path == &self.images[image])
                .context("Owned image disappeared before detach")?
                .1;
            match run(
                "/usr/bin/hdiutil",
                &["detach".as_ref(), device.as_ref()],
                None,
            ) {
                Ok(_) => {
                    ensure!(
                        !self
                            .owned()?
                            .iter()
                            .any(|(path, _)| path == &self.images[image]),
                        "Owned image remains attached"
                    );
                    return Ok(());
                }
                Err(error) => {
                    if attempt == 1 || attempt == DETACH_ATTEMPTS || !is_busy(&error) {
                        self.report_open_files(image);
                    }
                    if !is_busy(&error) || attempt == DETACH_ATTEMPTS {
                        return Err(error).with_context(|| {
                            format!("Owned image detach failed after {attempt} attempts")
                        });
                    }
                    eprintln!(
                        "Owned APFS backing busy on detach attempt {attempt}; rechecking ownership before retry"
                    );
                    on_busy()?;
                    std::thread::sleep(Duration::from_millis(100 << (attempt - 1)));
                }
            }
        }
        unreachable!("the final detach attempt returns its failure")
    }

    fn report_open_files(&self, image: usize) {
        let mount = if image == 0 {
            &self.mount
        } else {
            &self.blocker_mount
        };
        // Never let lsof's filesystem selection fall back to the host filesystem
        // after an unmount. Confirm this mount belongs to the exact owned backing.
        let info = match plist("/usr/bin/hdiutil", &["info".as_ref(), "-plist".as_ref()]) {
            Ok(info) => info,
            Err(error) => {
                eprintln!("Owned image diagnostics unavailable: {error:#}");
                return;
            }
        };
        let owned = info["images"].as_array().and_then(|images| {
            images.iter().find(|row| {
                row["image-path"]
                    .as_str()
                    .is_some_and(|path| Path::new(path) == self.images[image].as_path())
            })
        });
        let Some(owned) = owned else {
            eprintln!("Owned image is no longer attached; open-file diagnostic skipped");
            return;
        };
        let mounted = owned["system-entities"].as_array().is_some_and(|entities| {
            entities.iter().any(|entity| {
                entity["mount-point"]
                    .as_str()
                    .is_some_and(|path| Path::new(path) == mount.as_path())
            })
        });
        let mut command = Command::new("/usr/sbin/lsof");
        command.arg("-Fpcn");
        let selection = if mounted {
            command.args(["+f", "--"]).arg(mount);
            "mount"
        } else {
            eprintln!(
                "Owned image lacks the expected mount; current entities: {}",
                owned["system-entities"]
                    .to_string()
                    .chars()
                    .take(8192)
                    .collect::<String>()
            );
            // Select one freshly observed backing file, never the host filesystem
            // containing the now-unmounted directory. This grants no detach authority.
            command.arg("--").arg(&self.images[image]);
            "backing"
        };
        let output = command.timeout(Duration::from_secs(5)).output();
        match output {
            Ok(output) => eprintln!(
                "Owned {selection} open-file diagnostic ({}):\n{}\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout[..output.stdout.len().min(32 * 1024)]),
                String::from_utf8_lossy(&output.stderr[..output.stderr.len().min(32 * 1024)])
            ),
            Err(error) => eprintln!("Owned mount open-file diagnostic unavailable: {error}"),
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;

    #[test]
    fn busy_detach_requires_the_owned_tool_and_os_status() {
        for (program, code, message, expected) in [
            ("/usr/bin/hdiutil", 16, "localized diagnostic", true),
            ("/usr/bin/hdiutil", 1, "Resource busy", false),
            ("/other/program", 16, "Resource busy", false),
        ] {
            let error = anyhow::Error::new(CommandFailure {
                program: program.into(),
                status: std::process::ExitStatus::from_raw(code << 8),
                stderr: message.into(),
            });
            assert_eq!(is_busy(&error), expected);
        }
        assert!(!is_busy(&anyhow::anyhow!("Resource busy")));
    }

    #[test]
    #[ignore = "opt-in native APFS images; exercises real busy handles and device reassignment"]
    fn native_image_cleanup_rechecks_ownership_after_busy_and_bounds_retirement() -> Result<()> {
        let volume = ApfsFixture::new()?;
        let mut held = Some(fs::File::create(volume.root().join("held-by-fixture.txt"))?);
        let mut reassigned = false;
        volume.detach_with_busy(0, || {
            if !reassigned {
                drop(held.take());
                // Reassign the old device to the blocker before the retry. A
                // cached device would detach the wrong backing or leave ours live.
                volume.remount()?;
                reassigned = true;
            }
            Ok(())
        })?;
        assert!(reassigned, "the real held file must have blocked detach");
        let owned = volume.owned()?;
        assert!(!owned.iter().any(|(path, _)| path == &volume.images[0]));
        assert!(owned.iter().any(|(path, _)| path == &volume.images[1]));

        volume.attach(0, &volume.mount)?;
        let held = fs::File::create(volume.root().join("held-until-failure.txt"))?;
        let mut still_busy = 0;
        let error = volume
            .detach_with_busy(0, || {
                still_busy += 1;
                Ok(())
            })
            .unwrap_err();
        assert!(is_busy(&error), "{error:#}");
        assert_eq!(still_busy, DETACH_ATTEMPTS - 1);
        assert!(
            volume.images[0].is_file(),
            "failed retirement discarded its backing"
        );
        assert!(
            volume
                .owned()?
                .iter()
                .any(|(path, _)| path == &volume.images[0])
        );
        drop(held);
        volume.finish()
    }
}
