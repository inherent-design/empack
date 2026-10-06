//! Owned installer execution followed by independent file and launch verification.
use super::*;
use crate::engine::{
    content::verify_stream,
    snapshot::{Observation, SnapshotLimits},
    staging::{FrozenStage, MutableStage, StageToolArgument},
};
use std::{collections::BTreeSet, path::PathBuf, time::Duration};

/// Host Java and explicit external-tool allowances. The process tree is owned, not OS-sandboxed.
#[derive(Clone)]
pub struct InstallerExecution {
    pub java: PathBuf,
    pub deadline: Duration,
    pub heap_megabytes: u32,
    pub output: SnapshotLimits,
}
#[derive(Debug, Clone)]
pub struct InstallerRuntimeEvidence {
    pub installer: ContentId,
    pub expected: ExpectedContent,
    pub checksum_document: ContentId,
    pub contract: InstallerContract,
    pub main_class: String,
    pub alternative_matches: BTreeMap<PortableRelPath, ExpectedDigest>,
}
impl InstallerServerPlan {
    pub async fn prepare(
        mut self,
        transport: &HttpAcquisition,
        scope: &mut WorkScope,
        transfer: TransferLimits,
        archive: ArchiveLimits,
        policy: SourceEvidencePolicy,
        execution: InstallerExecution,
    ) -> Result<PreparedServerRuntime> {
        ensure!(
            execution.heap_megabytes > 0 && !execution.deadline.is_zero(),
            "Installer execution needs finite allowances"
        );
        for library in &mut self.contract.libraries {
            if library.expected.digests.is_none() && library.acceptable_sha1.is_empty() {
                let url = library
                    .download
                    .as_deref()
                    .context("Historical library lacks a source locator")?;
                let (digest, document) = repository_digest(transport, scope, url, transfer)
                    .await
                    .with_context(|| {
                    format!(
                        "Cannot resolve integrity evidence for {}",
                        library.coordinate
                    )
                })?;
                library.expected.digests = Some(DigestSet::new(vec![digest])?);
                library.checksum_documents.push(document);
            }
        }
        // Verify the independently selected base before any installer can consume or replace it.
        let vanilla = VanillaServerPlan::resolve(
            transport,
            scope,
            RuntimeResolution {
                minecraft: self.contract.runtime.minecraft.clone(),
                loader: LoaderKind::Vanilla,
                loader_version: None,
            },
            transfer,
        )
        .await?
        .acquire(transport, scope, transfer, archive, policy)
        .await?;
        // Acquire declared remote inputs through the same bounded transport as other content.
        // The installer still validates these copies and owns embedded/generated inputs.
        let libraries = self
            .acquire_libraries(transport, scope, transfer, policy, execution.output)
            .await?;
        let retained = ResourceRequest {
            scratch_bytes: execution.output.total_bytes,
            open_files: execution.output.entries as u64,
            ..ResourceRequest::default()
        };
        let request = ResourceRequest {
            jobs: 1,
            memory_bytes: (u64::from(execution.heap_megabytes) + 128)
                .checked_mul(1 << 20)
                .context("Installer memory allowance overflow")?,
            scratch_bytes: execution
                .output
                .total_bytes
                .checked_mul(2)
                .context("Installer scratch allowance overflow")?,
            open_files: retained
                .open_files
                .checked_add(16)
                .context("Installer file allowance overflow")?,
        };
        let worker = scope.spawn(request, retained, move |cancel| async move {
            let stage_cancel = cancel.clone();
            let staged = tokio::task::spawn_blocking(move || -> Result<_> {
                let mut stage = MutableStage::empty()?;
                stage.write(
                    &path(".empack-installer.jar")?,
                    &mut self.installer.lease().open(),
                    self.installer.lease().len(),
                    &stage_cancel,
                )?;
                let base = vanilla
                    .files
                    .get(&path("server.jar")?)
                    .context("Vanilla preparation lacks server file")?;
                stage.write(
                    &self.contract.minecraft_path,
                    &mut base.content.lease().open(),
                    base.content.lease().len(),
                    &stage_cancel,
                )?;
                for (relative, content) in libraries {
                    stage.write(
                        &relative,
                        &mut content.lease().open(),
                        content.lease().len(),
                        &stage_cancel,
                    )?;
                    // Each acquisition lease retires before the installer/output capture starts.
                }
                Ok((self, vanilla, stage))
            })
            .await
            .context("Installer staging worker panicked")??;
            let (plan, vanilla, stage) = staged;
            let arguments = [
                StageToolArgument::Text(format!("-Xmx{}M", execution.heap_megabytes)),
                StageToolArgument::Text("-Duser.home=.".into()),
                StageToolArgument::Text("-Djava.io.tmpdir=.".into()),
                StageToolArgument::Text("-jar".into()),
                StageToolArgument::Path(path(".empack-installer.jar")?),
                StageToolArgument::Text("--installServer".into()),
                StageToolArgument::Root,
            ];
            let stage = stage
                .run_tool(
                    &execution.java,
                    &arguments,
                    execution.deadline,
                    cancel.clone(),
                )
                .await?;
            tokio::task::spawn_blocking(move || {
                let frozen = stage.freeze(execution.output, &cancel)?;
                plan.verify_outputs(vanilla, frozen, archive, &cancel)
            })
            .await
            .context("Installer verification worker panicked")?
        })?;
        let (mut prepared, mut permit) = scope
            .accept(worker.wait().await?)?
            .transpose()?
            .into_parts();
        for file in prepared.files.values_mut() {
            file.content.retain_reservation(&mut permit)?;
        }
        Ok(prepared)
    }

    pub(super) async fn acquire_libraries(
        &self,
        transport: &HttpAcquisition,
        scope: &mut WorkScope,
        transfer: TransferLimits,
        policy: SourceEvidencePolicy,
        output: SnapshotLimits,
    ) -> Result<Vec<(PortableRelPath, AcquiredContent)>> {
        let mut acquired = Vec::new();
        let mut remaining = output.total_bytes;
        for library in &self.contract.libraries {
            let Some(url) = &library.download else {
                continue;
            };
            ensure!(
                acquired.len() < output.entries,
                "Installer inputs exceed entry allowance"
            );
            let content = transport
                .acquire(
                    scope,
                    DownloadRequest {
                        alternatives: NonEmpty::new(vec![url.clone()])?,
                        expected: library.expected.clone(),
                        limits: TransferLimits {
                            file_bytes: transfer.file_bytes.min(output.file_bytes).min(remaining),
                            ..transfer
                        },
                        evidence: policy,
                        // Alternative digests are checked below before this private lease is used.
                        initial: if library.acceptable_sha1.is_empty() {
                            InitialObservation::RequireEvidence
                        } else {
                            InitialObservation::Accepted
                        },
                    },
                )
                .await
                .with_context(|| {
                    format!("Cannot acquire installer library {}", library.coordinate)
                })?;
            verify_library_alternatives(library, &content)?;
            remaining = remaining
                .checked_sub(content.lease().len())
                .context("Installer inputs exceed byte allowance")?;
            acquired.push((library.path.clone(), content));
        }
        Ok(acquired)
    }

    pub(super) fn verify_outputs(
        self,
        mut vanilla: PreparedServerRuntime,
        mut stage: FrozenStage,
        archive: ArchiveLimits,
        cancel: &Cancellation,
    ) -> Result<PreparedServerRuntime> {
        let contract = &self.contract;
        let mut required: BTreeMap<PortableRelPath, ExpectedContent> = contract
            .libraries
            .iter()
            .map(|library| (library.path.clone(), library.expected.clone()))
            .collect();
        required.insert(
            contract.minecraft_path.clone(),
            vanilla.evidence.expected.clone(),
        );
        for (path, expected) in &contract.generated {
            if let Some(previous) = required.get(path) {
                ensure!(
                    expected.digests.is_none() || previous.digests == expected.digests,
                    "Conflicting installed output requirements"
                );
            } else {
                required.insert(path.clone(), expected.clone());
            }
        }
        // Libraries and generated launch files must be regular captured objects. Logs and the
        // installer executable are temporary tool outputs, never distribution members.
        let mut selected = BTreeSet::from([contract.minecraft_path.clone()]);
        for (relative, observation) in stage.inventory() {
            if relative.as_str().starts_with("libraries/")
                && matches!(observation, Observation::File(_))
            {
                selected.insert(relative.clone());
            }
        }
        let launch = match &contract.layout {
            InstallerLayout::Arguments {
                unix,
                windows,
                directory,
            } => {
                let unix_path = path(&format!("{}/unix_args.txt", directory.as_str()))?;
                let windows_path = path(&format!("{}/win_args.txt", directory.as_str()))?;
                for (relative, expected) in [(&unix_path, unix), (&windows_path, windows)] {
                    let mut actual = Vec::new();
                    stage.copy_verified(relative, &mut actual, cancel)?;
                    ensure!(
                        &actual == expected,
                        "Installer launch arguments differ from the verified tool"
                    );
                }
                let user = path("user_jvm_args.txt")?;
                ensure!(
                    matches!(stage.inventory().get(&user), Some(Observation::File(_))),
                    "Installer omitted JVM configuration"
                );
                selected.insert(user);
                ServerLaunch::Arguments {
                    unix: unix_path,
                    windows: windows_path,
                }
            }
            InstallerLayout::ExecutableJar { destination, .. } => {
                selected.insert(destination.clone());
                ServerLaunch::Jar(destination.clone())
            }
        };
        for path in required.keys() {
            ensure!(
                matches!(stage.inventory().get(path), Some(Observation::File(_))),
                "Installer omitted required file: {}",
                path.as_str()
            );
            selected.insert(path.clone());
        }
        let mut files = BTreeMap::new();
        let mut alternative_matches = BTreeMap::new();
        for relative in selected {
            cancel.check()?;
            let observation = match stage.inventory().get(&relative) {
                Some(Observation::File(file)) => file.clone(),
                _ => anyhow::bail!(
                    "Installer output is not a regular file: {}",
                    relative.as_str()
                ),
            };
            let mut expected = required.remove(&relative).unwrap_or(ExpectedContent {
                digests: None,
                size: Some(observation.bytes),
                accepted_observation: None,
            });
            expected.accepted_observation = Some(ContentId::from_sha256(observation.content));
            let mut reader = FrozenReader {
                stage: &mut stage,
                path: &relative,
                offset: 0,
            };
            let content = verify_stream(
                &mut reader,
                &expected,
                archive.file_bytes,
                SourceEvidencePolicy::Compatibility,
                InitialObservation::Accepted,
                cancel,
            )?;
            if let Some(library) = contract
                .libraries
                .iter()
                .find(|library| library.path == relative && !library.acceptable_sha1.is_empty())
            {
                let matched = verify_library_alternatives(library, &content)?
                    .context("Missing alternative digest evidence")?;
                alternative_matches.insert(relative.clone(), matched);
            }
            stage.retire_input(&relative)?;
            files.insert(
                relative,
                AcquiredBuildFile {
                    content,
                    permissions: FilePermissions {
                        readonly: false,
                        executable: false,
                    },
                },
            );
        }
        let main = verify_launch(
            &launch,
            &files,
            &self.installer,
            &contract.layout,
            archive,
            cancel,
        )?;
        vanilla.files = files;
        vanilla.runtime = contract.runtime.clone();
        vanilla.launch = launch;
        vanilla.evidence.loader = Some(LoaderRuntimeEvidence::Installer(Box::new(
            InstallerRuntimeEvidence {
                installer: self.installer.lease().id(),
                expected: self.expected,
                checksum_document: self.checksum_document,
                contract: self.contract,
                main_class: main,
                alternative_matches,
            },
        )));
        Ok(vanilla)
    }
}
fn verify_library_alternatives(
    library: &InstallerLibrary,
    content: &AcquiredContent,
) -> Result<Option<ExpectedDigest>> {
    if library.acceptable_sha1.is_empty() {
        return Ok(None);
    }
    library
        .acceptable_sha1
        .iter()
        .find(|expected| content.observed_digests().values().contains(expected))
        .cloned()
        .map(Some)
        .context("Installer library matches no accepted checksum")
}

struct FrozenReader<'a> {
    stage: &'a mut FrozenStage,
    path: &'a PortableRelPath,
    offset: u64,
}
impl Read for FrozenReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let count = self.stage.read_at(self.path, self.offset, buffer)?;
        self.offset += count as u64;
        Ok(count)
    }
}
fn verify_launch(
    launch: &ServerLaunch,
    files: &BTreeMap<PortableRelPath, AcquiredBuildFile>,
    installer: &AcquiredContent,
    layout: &InstallerLayout,
    limits: ArchiveLimits,
    cancel: &Cancellation,
) -> Result<String> {
    match launch {
        ServerLaunch::Jar(relative) => {
            let server = &files
                .get(relative)
                .context("Missing executable runtime")?
                .content;
            let InstallerLayout::ExecutableJar {
                member: embedded, ..
            } = layout
            else {
                anyhow::bail!("Executable layout mismatch")
            };
            let mut source = JarReader::open(installer, limits, cancel)?;
            let mut entry = source.archive.by_name(embedded.as_str())?;
            let (hash, size) = crate::engine::io::copy_bounded(
                &mut entry,
                &mut std::io::sink(),
                limits.file_bytes,
                cancel,
            )?;
            ensure!(
                &hash == server.lease().id().bytes() && size == server.lease().len(),
                "Installed executable differs from verified installer member"
            );
            let mut jar = JarReader::open(server, limits, cancel)?;
            let attributes = jar.main_attributes()?;
            let main = attributes
                .get("main-class")
                .context("Server executable lacks main class")?
                .clone();
            check_class(&mut jar, &main, cancel)?;
            if let Some(classpath) = attributes.get("class-path") {
                for entry in classpath.split_whitespace() {
                    ensure!(
                        files.contains_key(&path(entry)?),
                        "Historical runtime omits classpath entry: {entry}"
                    );
                }
            }
            Ok(main)
        }
        ServerLaunch::Arguments { unix, windows } => {
            let mut actual_main = None;
            for (relative, separator) in [(unix, ':'), (windows, ';')] {
                let mut text = String::new();
                files[relative]
                    .content
                    .lease()
                    .open()
                    .read_to_string(&mut text)?;
                let (main, libraries) = launch_arguments(&text, separator)?;
                for relative in &libraries {
                    ensure!(
                        files.contains_key(relative),
                        "Runtime omits declared classpath entry: {}",
                        relative.as_str()
                    );
                }
                let mut found = false;
                for relative in libraries {
                    let mut jar = JarReader::open(&files[&relative].content, limits, cancel)?;
                    if jar
                        .archive
                        .file_names()
                        .any(|name| name == format!("{}.class", main.replace('.', "/")))
                    {
                        check_class(&mut jar, &main, cancel)?;
                        found = true;
                    }
                }
                ensure!(found, "Runtime libraries omit declared main class");
                if let Some(previous) = actual_main.replace(main.clone()) {
                    ensure!(
                        previous == main,
                        "Host launch files select different main classes"
                    );
                }
            }
            actual_main.context("Missing runtime main class")
        }
    }
}
fn check_class(jar: &mut JarReader, main: &str, cancel: &Cancellation) -> Result<()> {
    ensure!(
        !main.is_empty()
            && main.split('.').all(|part| !part.is_empty()
                && part
                    .bytes()
                    .all(|ch| ch.is_ascii_alphanumeric() || b"_$".contains(&ch))),
        "Invalid launcher class"
    );
    let mut entry = jar
        .archive
        .by_name(&format!("{}.class", main.replace('.', "/")))?;
    ensure!(
        !entry.encrypted() && entry.size() <= 16 << 20,
        "Invalid launcher class entry"
    );
    let mut magic = [0; 4];
    entry.read_exact(&mut magic)?;
    ensure!(
        magic == [0xca, 0xfe, 0xba, 0xbe],
        "Invalid launcher bytecode"
    );
    crate::engine::io::copy_bounded(&mut entry, &mut std::io::sink(), 16 << 20, cancel)?;
    Ok(())
}
fn parse_arguments(text: &str) -> Result<Vec<String>> {
    ensure!(
        !text.contains(['\0', '\'', '"', '\\']),
        "Runtime arguments need an unsupported quoting rule"
    );
    Ok(text
        .lines()
        .map(|line| line.split('#').next().unwrap_or(""))
        .flat_map(str::split_whitespace)
        .map(str::to_owned)
        .collect())
}

fn launch_arguments(text: &str, separator: char) -> Result<(String, BTreeSet<PortableRelPath>)> {
    let tokens = parse_arguments(text)?;
    let mut arguments = tokens.iter();
    let mut libraries = BTreeSet::new();
    let mut add_paths = |value: &str| -> Result<()> {
        ensure!(!value.is_empty(), "Empty runtime classpath");
        for entry in value.split(separator) {
            libraries.insert(path(entry)?);
        }
        Ok(())
    };
    let main = loop {
        let word = arguments
            .next()
            .context("Runtime argument file lacks main class")?;
        match word.as_str() {
            "-p" | "--module-path" | "-cp" | "-classpath" | "--class-path" => {
                add_paths(arguments.next().context("Missing runtime classpath")?)?
            }
            "--add-modules" | "--add-opens" | "--add-exports" | "--add-reads" => {
                let value = arguments.next().context("Missing JVM option value")?;
                ensure!(!value.starts_with('-'), "Missing JVM option value");
            }
            word if word.starts_with("-DlegacyClassPath=") => add_paths(&word[18..])?,
            word if word.starts_with("--module-path=") => add_paths(&word[14..])?,
            word if word.starts_with("--class-path=") => add_paths(&word[13..])?,
            word if word.starts_with("-D") => {}
            word if word.starts_with('-') || word.starts_with('@') => {
                anyhow::bail!("Unsupported JVM launch option")
            }
            word => break word.to_owned(),
        }
    };
    ensure!(
        !libraries.is_empty(),
        "Runtime has no declared launch libraries"
    );
    Ok((main, libraries))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn launch_arguments_distinguish_jvm_values_from_main_class() {
        let (main, libraries)=launch_arguments("--add-modules java.logging\n-p libraries/a.jar:libraries/b.jar\n-DlegacyClassPath=libraries/c.jar\nactual.Main --launchTarget forgeserver", ':').unwrap();
        assert_eq!(main, "actual.Main");
        assert_eq!(libraries.len(), 3);
        for text in [
            "-p",
            "-p ../outside.jar actual.Main",
            "-jar ignored.jar",
            "-p libraries/a.jar @outside.args",
            "--add-modules -p actual.Main",
            "-p \"quoted\" actual.Main",
        ] {
            assert!(launch_arguments(text, ':').is_err(), "accepted {text}");
        }
    }
}
