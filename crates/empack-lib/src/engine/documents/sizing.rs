//! Allocation-free admission estimates for the codec's owned wire trees and round trips.
use super::*;

#[derive(Default)]
struct Size(u64);
impl Size {
    fn add(&mut self, bytes: u64) -> Result<()> {
        self.0 = self
            .0
            .checked_add(bytes)
            .context("Document memory estimate overflow")?;
        Ok(())
    }
    fn nodes(&mut self, count: usize) -> Result<()> {
        self.add(
            (count as u64)
                .checked_mul(512)
                .context("Document node estimate overflow")?,
        )
    }
    fn text(&mut self, text: &str) -> Result<()> {
        self.nodes(1)?;
        self.add(
            (text.len() as u64)
                .checked_mul(6)
                .context("Document text estimate overflow")?,
        )
    }
    fn optional(&mut self, value: Option<&str>) -> Result<()> {
        if let Some(value) = value {
            self.text(value)?;
        }
        Ok(())
    }
    fn requirements(&mut self, value: &Requirements) -> Result<()> {
        self.nodes(4)?;
        for side in [&value.client, &value.server] {
            if let Requirement::Optional(choice) = side {
                self.text(choice.key.as_str())?;
                self.optional(choice.description.as_deref())?;
            }
        }
        Ok(())
    }
    fn placements(&mut self, values: &[Placement]) -> Result<()> {
        for value in values {
            self.nodes(4)?;
            self.text(value.destination.relative().as_str())?;
            self.requirements(&value.requirements)?;
        }
        Ok(())
    }
    fn extension(&mut self, value: &ExtensionValue) -> Result<()> {
        self.nodes(1)?;
        match value {
            ExtensionValue::Null | ExtensionValue::Bool(_) => {}
            ExtensionValue::Number(value) | ExtensionValue::Text(value) => self.text(value)?,
            ExtensionValue::List(values) => {
                for value in values {
                    self.extension(value)?;
                }
            }
            ExtensionValue::Object(values) => {
                for (key, value) in values {
                    self.text(key)?;
                    self.extension(value)?;
                }
            }
        }
        Ok(())
    }
}

/// Count variable text and collection members before constructing codec DTOs. Per-node
/// overhead includes bounded identifiers, hashes and scalar fields; the multiplier covers
/// simultaneous wire trees, semantic round-trip checks and encoded buffers. This is an
/// admission estimate, not a replacement for the codec's independent document-size limit.
pub(super) fn encoding_memory(project: &ResolvedProject) -> Result<u64> {
    let mut size = Size::default();
    size.nodes(32)?;
    let intent = project.intent();
    size.text(&intent.metadata.name)?;
    size.text(&intent.metadata.version)?;
    size.optional(intent.metadata.author.as_deref())?;
    size.optional(intent.metadata.description.as_deref())?;
    size.text(intent.runtime.minecraft.as_str())?;
    size.optional(
        intent
            .runtime
            .loader_version
            .as_ref()
            .map(LoaderVersion::as_str),
    )?;
    for version in &intent.runtime.acceptable_versions {
        size.text(version.as_str())?;
    }
    for (key, value) in &intent.roots {
        size.nodes(16)?;
        size.text(key.as_str())?;
        size.requirements(&value.requirements)?;
        match &value.source {
            SourceIntent::Provider(_) => {}
            SourceIntent::Search { query, providers } => {
                size.text(query)?;
                size.nodes(providers.as_slice().len())?;
            }
            SourceIntent::Url(urls) => {
                for url in urls.as_slice() {
                    size.text(url)?;
                }
            }
            SourceIntent::Local(path) => size.text(path.as_str())?,
            SourceIntent::LocalFiles(files) => {
                for (slot, path) in files {
                    size.text(slot.as_str())?;
                    size.text(path.as_str())?;
                }
            }
        }
        match &value.placement {
            PlacementIntent::Automatic => {}
            PlacementIntent::Explicit(places) | PlacementIntent::ArchiveRoot(places) => {
                size.placements(places.as_slice())?
            }
            PlacementIntent::ByFile(files) => {
                for (slot, places) in files {
                    size.text(slot.as_str())?;
                    size.placements(places.as_slice())?;
                }
            }
        }
    }
    for path in intent.layout.values() {
        size.text(path.as_str())?;
    }
    size.nodes(intent.distribution.targets.as_slice().len())?;
    for (key, value) in &intent.extensions {
        size.text(key)?;
        size.extension(value)?;
    }
    let lock = project.lock();
    size.text(&lock.resolver)?;
    size.text(lock.runtime.minecraft.as_str())?;
    size.optional(
        lock.runtime
            .loader_version
            .as_ref()
            .map(LoaderVersion::as_str),
    )?;
    for version in &lock.acceptable_versions {
        size.text(version.as_str())?;
    }
    for (key, dependency) in &lock.dependencies {
        size.nodes(16)?;
        size.text(key.as_str())?;
        size.text(&dependency.title)?;
        match &dependency.identity {
            ResolvedIdentity::Provider(_) => {}
            ResolvedIdentity::Url(key) | ResolvedIdentity::Local(key) => size.text(key.as_str())?,
        }
        for file in dependency.files.as_slice() {
            size.nodes(32)?;
            size.text(file.slot.as_str())?;
            size.text(&file.provenance.source)?;
            size.optional(file.provenance.location.as_deref())?;
            for conversion in &file.provenance.conversions {
                size.text(conversion)?;
            }
            size.placements(file.placements.as_slice())?;
            match &file.acquisition {
                AcquisitionSpec::Provider {
                    slot, alternatives, ..
                } => {
                    size.text(slot.as_str())?;
                    for url in alternatives {
                        size.text(url)?;
                    }
                }
                AcquisitionSpec::ProviderArchiveMember { archive, member } => {
                    size.nodes(16)?;
                    size.text(archive.slot.as_str())?;
                    size.text(member.as_str())?;
                    for url in &archive.alternatives {
                        size.text(url)?;
                    }
                }
                AcquisitionSpec::Url(urls) => {
                    for url in urls.as_slice() {
                        size.text(url)?;
                    }
                }
                AcquisitionSpec::Local(path) => size.text(path.as_str())?,
                AcquisitionSpec::Embedded { archive, member } => {
                    size.text(archive.as_str())?;
                    size.text(member.as_str())?;
                }
                AcquisitionSpec::Manual { instructions, .. } => size.text(instructions)?,
            }
        }
    }
    for (key, edges) in &lock.required_edges {
        size.text(key.as_str())?;
        for edge in edges {
            size.text(edge.as_str())?;
        }
    }
    for key in lock.coverage.keys() {
        size.text(key.as_str())?;
    }
    size.0
        .checked_mul(8)
        .and_then(|bytes| bytes.checked_add(256 << 10))
        .context("Document memory estimate overflow")
}
