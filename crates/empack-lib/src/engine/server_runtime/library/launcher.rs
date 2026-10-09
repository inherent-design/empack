use super::*;
use crate::engine::{io::copy_bounded, staging::PrivateFile};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, SeekFrom, Write},
};

/// Produce then independently read the exact JAR inventory; JAR resources are not native paths.
pub(super) fn assemble(
    paths: &[PortableRelPath],
    libraries: &[AcquiredContent],
    contract: &LauncherContract,
    limits: ArchiveLimits,
    maximum: u64,
    cancel: &Cancellation,
) -> Result<(AcquiredContent, String)> {
    let launch_main = &contract.launch_main;
    let shaded = contract.shaded;
    let (group, artifact) = contract.kind.coordinate();
    let prefix = format!("libraries/{}/{artifact}/", group.replace('.', "/"));
    let loader = libraries
        .iter()
        .zip(paths)
        .find(|(_, path)| path.as_str().starts_with(&prefix))
        .context("Library runtime loader library is missing")?
        .0;
    let mut jar = JarReader::open(loader, limits, cancel)?;
    let main = match &contract.declared_main {
        Some(main) => main.clone(),
        None => jar
            .main_attributes()?
            .remove("main-class")
            .context("Loader JAR has no main class")?,
    };
    for class in [&main, launch_main] {
        let class_path = class_path(class)?;
        let mut entry = jar
            .archive
            .by_name(&class_path)
            .context("Library runtime loader omits a declared launcher class")?;
        ensure!(
            !entry.encrypted() && !entry.is_dir() && entry.size() <= 16 << 20,
            "Invalid runtime launcher class entry"
        );
        let mut magic = [0; 4];
        entry.read_exact(&mut magic)?;
        ensure!(
            magic == [0xca, 0xfe, 0xba, 0xbe],
            "Invalid runtime launcher class"
        );
        copy_bounded(&mut entry, &mut io::sink(), 16 << 20, cancel)?;
    }
    drop(jar);
    let mut manifest = fold("Manifest-Version", "1.0")?;
    manifest.extend(fold("Main-Class", &main)?);
    if !shaded {
        manifest.extend(fold(
            "Class-Path",
            &paths
                .iter()
                .map(|path| path.as_str())
                .collect::<Vec<_>>()
                .join(" "),
        )?);
    }
    manifest.extend(b"\r\n");
    let mut private = PrivateFile::new()?;
    let mut output = zip::ZipWriter::new(BoundedFile {
        file: private.file(),
        maximum,
    });
    let mut expected = BTreeMap::new();
    let mut total = 0u64;
    let mut emit = |output: &mut zip::ZipWriter<BoundedFile<'_>>,
                    name: &str,
                    source: &mut dyn Read|
     -> Result<()> {
        ensure!(
            expected.len() < limits.entries,
            "Library runtime launcher entry count exceeds limit"
        );
        output.start_file(
            name,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )?;
        let (hash, size) = copy_bounded(
            source,
            output,
            limits
                .file_bytes
                .min(limits.total_bytes.saturating_sub(total)),
            cancel,
        )?;
        total = total
            .checked_add(size)
            .context("Library runtime launcher byte count overflow")?;
        expected.insert(name.to_owned(), (hash, size));
        Ok(())
    };
    emit(
        &mut output,
        "META-INF/MANIFEST.MF",
        &mut manifest.as_slice(),
    )?;
    emit(
        &mut output,
        &format!("{}-server-launch.properties", contract.kind.name()),
        &mut format!("launch.mainClass={launch_main}\n").as_bytes(),
    )?;
    let mut added = BTreeSet::from([
        "META-INF/MANIFEST.MF".to_owned(),
        format!("{}-server-launch.properties", contract.kind.name()),
    ]);
    let mut services: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut service_bytes = 0usize;
    if shaded {
        for library in libraries {
            cancel.check()?;
            let mut jar = JarReader::open(library, limits, cancel)?;
            for index in 0..jar.archive.len() {
                let mut entry = jar.archive.by_index(index)?;
                if entry.is_dir() {
                    continue;
                }
                ensure!(
                    !entry.encrypted(),
                    "Library runtime library contains encrypted data"
                );
                let name = entry.name().to_owned();
                if name
                    .strip_prefix("META-INF/services/")
                    .is_some_and(|name| !name.contains('/'))
                {
                    ensure!(
                        entry.size() <= 64 << 10,
                        "Library runtime service declaration exceeds limit"
                    );
                    let mut bytes = String::new();
                    entry.take((64 << 10) + 1).read_to_string(&mut bytes)?;
                    ensure!(
                        bytes.len() <= 64 << 10,
                        "Library runtime service declaration exceeds limit"
                    );
                    service_bytes = service_bytes
                        .checked_add(bytes.len())
                        .context("Library runtime service size overflow")?;
                    ensure!(
                        service_bytes <= 4 << 20,
                        "Library runtime service declarations exceed limit"
                    );
                    let definitions = services.entry(name).or_default();
                    for line in bytes.lines() {
                        let value = line
                            .split('#')
                            .next()
                            .unwrap_or_default()
                            .trim_matches(|ch: char| ch <= ' ');
                        if !value.is_empty() && !definitions.iter().any(|item| item == value) {
                            class_path(value)?;
                            definitions.push(value.to_owned());
                        }
                    }
                } else if !signature(&name) && added.insert(name.clone()) {
                    ensure!(
                        entry.size() <= limits.file_bytes,
                        "Library runtime library entry exceeds limit"
                    );
                    emit(&mut output, &name, &mut entry)?;
                }
            }
        }
        for (name, values) in services {
            emit(
                &mut output,
                &name,
                &mut (values.join("\n") + "\n").as_bytes(),
            )?;
        }
    }
    output.finish()?;
    private.file().rewind()?;
    // Verify the actual published container against the independently accumulated input inventory.
    let generated_length = private.file().metadata()?.len();
    preflight_zip(private.file(), generated_length, limits, cancel)?;
    private.file().rewind()?;
    let mut archive = zip::ZipArchive::new(private.file())?;
    let mut seen = BTreeSet::new();
    ensure!(
        archive.len() == expected.len(),
        "Generated library launcher inventory differs"
    );
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        ensure!(
            seen.insert(entry.name().to_owned()) && !entry.encrypted() && !entry.is_dir(),
            "Invalid generated Fabric launcher entry"
        );
        let &(hash, size) = expected
            .get(entry.name())
            .context("Unexpected generated launcher entry")?;
        ensure!(
            entry.size() == size,
            "Generated launcher entry size differs"
        );
        let actual = copy_bounded(&mut entry, &mut io::sink(), size, cancel)?;
        ensure!(
            actual == (hash, size),
            "Generated library launcher bytes differ"
        );
    }
    drop(archive);
    private.file().rewind()?;
    let length = private.file().metadata()?.len();
    let content = verify_stream(
        private.file(),
        &ExpectedContent {
            digests: None,
            size: Some(length),
            accepted_observation: None,
        },
        maximum,
        SourceEvidencePolicy::Compatibility,
        InitialObservation::Accepted,
        cancel,
    )?;
    Ok((content, main))
}
fn fold(name: &str, value: &str) -> Result<Vec<u8>> {
    ensure!(
        !value.bytes().any(|byte| matches!(byte, 0 | b'\r' | b'\n')),
        "Invalid generated JAR attribute"
    );
    let bytes = format!("{name}: {value}").into_bytes();
    let mut result = Vec::new();
    for (index, chunk) in bytes.chunks(70).enumerate() {
        if index > 0 {
            result.push(b' ');
        }
        result.extend(chunk);
        result.extend(b"\r\n");
    }
    Ok(result)
}
fn signature(name: &str) -> bool {
    name.strip_prefix("META-INF/").is_some_and(|name| {
        !name.contains('/')
            && name
                .rsplit_once('.')
                .is_some_and(|(_, ext)| matches!(ext, "SF" | "DSA" | "RSA" | "EC"))
    })
}
struct BoundedFile<'a> {
    file: &'a mut std::fs::File,
    maximum: u64,
}
impl Write for BoundedFile<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let position = self.file.stream_position()?;
        if position
            .checked_add(bytes.len() as u64)
            .is_none_or(|end| end > self.maximum)
        {
            return Err(io::Error::other(
                "Library runtime launcher exceeds reserved bytes",
            ));
        }
        self.file.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}
impl Seek for BoundedFile<'_> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.file.seek(position)
    }
}
