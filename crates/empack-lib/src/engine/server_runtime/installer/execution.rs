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
impl InstallerExecution {
    /// Reserve installer work and its retained output together with the host's live inputs.
    pub(crate) fn reservations(&self) -> Result<(ResourceRequest, ResourceRequest)> {
        let retained = ResourceRequest {
            scratch_bytes: self.output.total_bytes,
            open_files: self.output.entries as u64,
            ..ResourceRequest::default()
        };
        let request = ResourceRequest {
            jobs: 1,
            memory_bytes: (u64::from(self.heap_megabytes) + 128)
                .checked_mul(1 << 20)
                .context("Installer memory allowance overflow")?,
            scratch_bytes: self
                .output
                .total_bytes
                .checked_mul(2)
                .context("Installer scratch allowance overflow")?,
            open_files: retained
                .open_files
                .checked_add(16)
                .context("Installer file allowance overflow")?,
        };
        Ok((request, retained))
    }
}
#[derive(Debug, Clone)]
pub struct InstallerRuntimeEvidence {
    pub installer: ContentId,
    pub expected: ExpectedContent,
    pub checksum_document: ContentId,
    pub contract: InstallerContract,
    pub main_class: String,
    pub alternative_matches: BTreeMap<PortableRelPath, ExpectedDigest>,
    pub bundled_libraries: BTreeMap<PortableRelPath, ExpectedContent>,
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
        let (request, retained) = execution.reservations()?;
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
                plan.verify_outputs(vanilla, frozen, archive, policy, &cancel)
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
        let mut pool =
            crate::engine::content::ContentPool::owned(scope, output.total_bytes).await?;
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
            let content = pool.consolidate_owned(scope, content).await?;
            acquired.push((library.path.clone(), content));
        }
        Ok(acquired)
    }

    pub(super) fn verify_outputs(
        self,
        mut vanilla: PreparedServerRuntime,
        mut stage: FrozenStage,
        archive: ArchiveLimits,
        policy: SourceEvidencePolicy,
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
        let bundled_libraries = bundled_library_assertions(
            &vanilla
                .files
                .get(&path("server.jar")?)
                .context("Missing verified vanilla server")?
                .content,
            archive,
            cancel,
        )?;
        for (path, expected) in &bundled_libraries {
            if let Some(previous) = required.get_mut(path) {
                ensure!(
                    previous.size.is_none() || previous.size == expected.size,
                    "Conflicting bundled library size"
                );
                previous.size = expected.size;
                previous.digests = Some(DigestSet::new(
                    previous
                        .digests
                        .iter()
                        .flat_map(|set| set.values())
                        .chain(expected.digests.iter().flat_map(|set| set.values()))
                        .cloned()
                        .collect(),
                )?);
            } else {
                required.insert(path.clone(), expected.clone());
            }
        }
        // Only declared profile/bundler members and exact launch outputs belong in a runtime.
        let mut selected = BTreeSet::from([contract.minecraft_path.clone()]);
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
                selected.insert(unix_path.clone());
                selected.insert(windows_path.clone());
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
                policy,
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
                bundled_libraries,
            },
        )));
        Ok(vanilla)
    }
}
fn bundled_library_assertions(
    base: &AcquiredContent,
    limits: ArchiveLimits,
    cancel: &Cancellation,
) -> Result<BTreeMap<PortableRelPath, ExpectedContent>> {
    let mut jar = JarReader::open(base, limits, cancel)?;
    if !jar
        .archive
        .file_names()
        .any(|name| name == "META-INF/libraries.list")
    {
        return Ok(BTreeMap::new());
    }
    let bytes = member(&mut jar, "META-INF/libraries.list", 1 << 20, cancel)?;
    let mut libraries = BTreeMap::new();
    for line in std::str::from_utf8(&bytes)?.lines() {
        cancel.check()?;
        ensure!(
            libraries.len() < 2048,
            "Bundled library count exceeds limit"
        );
        let fields: Vec<_> = line.split('\t').collect();
        ensure!(fields.len() == 3, "Invalid bundled library record");
        let relative = maven_path(fields[1])?;
        ensure!(
            relative.as_str() == fields[2],
            "Bundled library destination differs from coordinate"
        );
        let expected = ExpectedDigest::parse("sha256", fields[0])?;
        let entry = jar
            .archive
            .by_name(&format!("META-INF/libraries/{}", relative.as_str()))
            .context("Bundler omits declared library")?;
        ensure!(
            !entry.is_dir() && !entry.encrypted() && entry.size() <= limits.file_bytes,
            "Invalid bundled library entry"
        );
        let previous = libraries.insert(
            path(&format!("libraries/{}", relative.as_str()))?,
            ExpectedContent {
                digests: Some(DigestSet::new(vec![expected])?),
                size: Some(entry.size()),
                accepted_observation: None,
            },
        );
        ensure!(previous.is_none(), "Duplicate bundled library declaration");
    }
    Ok(libraries)
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
    #[test]
    fn bundled_libraries_require_exact_declared_coordinates_and_hashes() {
        use std::io::{Cursor, Write};
        let hash = ExpectedDigest::Sha256(Sha256::digest(b"payload").into()).hex();
        for case in [
            "valid",
            "wrong path",
            "bad digest",
            "duplicate",
            "missing member",
        ] {
            let relative = "fixture/library/1/library-1.jar";
            let line = format!(
                "{}\tfixture:library:1\t{}\n",
                if case == "bad digest" { "bad" } else { &hash },
                if case == "wrong path" {
                    "../outside.jar"
                } else {
                    relative
                }
            );
            let document = if case == "duplicate" {
                format!("{line}{line}")
            } else {
                line
            };
            let mut jar = zip::ZipWriter::new(Cursor::new(Vec::new()));
            jar.start_file(
                "META-INF/libraries.list",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            jar.write_all(document.as_bytes()).unwrap();
            if case != "missing member" {
                jar.start_file(
                    format!("META-INF/libraries/{relative}"),
                    zip::write::SimpleFileOptions::default(),
                )
                .unwrap();
                jar.write_all(b"payload").unwrap();
            }
            let bytes = jar.finish().unwrap().into_inner();
            let cancel = Cancellation::default();
            let source = verify_stream(
                &mut bytes.as_slice(),
                &ExpectedContent {
                    digests: None,
                    size: None,
                    accepted_observation: None,
                },
                1 << 20,
                SourceEvidencePolicy::Compatibility,
                InitialObservation::Accepted,
                &cancel,
            )
            .unwrap();
            let result = bundled_library_assertions(&source, ArchiveLimits::default(), &cancel);
            assert_eq!(result.is_ok(), case == "valid", "{case}");
            if let Ok(files) = result {
                let expected = &files[&path(&format!("libraries/{relative}")).unwrap()];
                assert_eq!(expected.size, Some(7));
                assert_eq!(expected.digests.as_ref().unwrap().values()[0].hex(), hash);
                assert!(
                    verify_stream(
                        &mut b"changed".as_slice(),
                        expected,
                        7,
                        SourceEvidencePolicy::Compatibility,
                        InitialObservation::RequireEvidence,
                        &cancel
                    )
                    .is_err()
                );
            }
        }
    }
}
