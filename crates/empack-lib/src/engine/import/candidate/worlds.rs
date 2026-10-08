//! Imported provider archives use the same member interpretation as direct provider additions.
use super::*;
use crate::engine::{addition::world, content::AcquiredContent, mrpack::AcquiredBuildFile};

impl ImportCandidate {
    pub(in crate::engine) fn bound_content(
        &self,
        key: &DependencyKey,
        slot: &FileSlot,
        source: &ImportContentKey,
    ) -> (&AcquiredContent, FilePermissions) {
        if let Some(member) = self.members.get(&(key.clone(), slot.clone())) {
            (&member.content, member.permissions)
        } else {
            (
                &self.content.content()[source],
                self.content
                    .permissions()
                    .get(source)
                    .copied()
                    .unwrap_or(FilePermissions {
                        readonly: false,
                        executable: false,
                    }),
            )
        }
    }

    pub(super) async fn interpret_worlds(mut self, scope: &mut WorkScope) -> Result<Self> {
        let selected: Vec<_> = self
            .project
            .lock()
            .dependencies
            .iter()
            .filter(|(_, dependency)| {
                dependency.kind == ContentKind::World && dependency.selected.is_some()
            })
            .map(|(key, dependency)| (key.clone(), dependency.clone()))
            .collect();
        if selected.is_empty() {
            return Ok(self);
        }
        let limits = self.content.plan().limits();
        let mut intent = self.project.intent().clone();
        let mut lock = self.project.lock().clone();
        let mut expanded = 0u64;
        let mut records = self.bindings.len();
        for (key, dependency) in selected {
            ensure!(
                dependency.files.as_slice().len() == 1,
                "Imported provider world requires exactly one source archive"
            );
            let source = &dependency.files.as_slice()[0];
            let AcquisitionSpec::Provider {
                pin,
                slot,
                alternatives,
            } = &source.acquisition
            else {
                anyhow::bail!("Imported world lacks its original provider archive identity");
            };
            let binding = self
                .bindings
                .remove(&(key.clone(), slot.clone()))
                .context("Imported world lacks an acquired archive")?;
            let content = self.content.content()[&binding].clone();
            let archive = ProviderArchiveSource {
                pin: pin.clone(),
                slot: slot.clone(),
                alternatives: alternatives.clone(),
                expected: source.expected.clone(),
            };
            let members = world::read(
                scope,
                AcquiredBuildFile {
                    content,
                    permissions: FilePermissions {
                        readonly: false,
                        executable: false,
                    },
                },
                ArchiveLimits {
                    total_bytes: limits.archive.total_bytes.min(
                        limits
                            .total_bytes
                            .checked_sub(expanded)
                            .context("Imported world expansion exceeds total allowance")?,
                    ),
                    ..limits.archive
                },
                self.content.evidence,
            )
            .await?;
            let (members, permit) = members.into_parts();
            self._members.push(permit);
            records = records
                .checked_sub(1)
                .and_then(|n| n.checked_add(members.len()))
                .context("Imported world record count overflow")?;
            ensure!(
                records <= limits.records,
                "Imported world members exceed record allowance"
            );
            let mut files = Vec::new();
            for member in members {
                let bytes = member.file.content.lease().len();
                expanded = expanded
                    .checked_add(bytes)
                    .context("Imported world expansion overflow")?;
                let slot = FileSlot::parse(member.relative.as_str())?;
                let mut provenance = source.provenance.clone();
                provenance.declared_digests = None;
                files.push(ResolvedFile {
                    slot: slot.clone(),
                    acquisition: AcquisitionSpec::ProviderArchiveMember {
                        archive: archive.clone(),
                        member: member.member,
                    },
                    expected: ExpectedContent {
                        digests: None,
                        size: Some(bytes),
                        accepted_observation: Some(member.file.content.lease().id()),
                    },
                    provenance,
                    placements: NonEmpty::new(
                        source
                            .placements
                            .as_slice()
                            .iter()
                            .map(|base| {
                                Ok(Placement {
                                    destination: InstallDestination::parse(&format!(
                                        "{}/{}",
                                        base.destination.relative().as_str(),
                                        member.relative.as_str()
                                    ))?,
                                    layer: base.layer,
                                    requirements: base.requirements.clone(),
                                })
                            })
                            .collect::<Result<Vec<_>>>()?,
                    )?,
                });
                self.bindings
                    .insert((key.clone(), slot.clone()), binding.clone());
                self.members.insert((key.clone(), slot), member.file);
            }
            intent
                .roots
                .get_mut(&key)
                .context("Imported world lacks root intent")?
                .placement = PlacementIntent::ArchiveRoot(source.placements.clone());
            lock.dependencies.get_mut(&key).unwrap().files = NonEmpty::new(files)?;
        }
        let codec = DocumentCodec;
        let intent_bytes = codec.encode_intent(&intent)?;
        let revision = codec
            .decode_intent(&intent_bytes, "interpreted world import")?
            .semantic_revision();
        lock.intent_revision = revision;
        self.project = ResolvedProject::validate(intent, lock, revision)?;
        let mut bytes = intent_bytes.len() as u64 + codec.encode_lock(&self.project)?.len() as u64;
        let mut payload_bytes = 0u64;
        for ((key, slot), source) in &self.bindings {
            let (content, _) = self.bound_content(key, slot, source);
            let file = self.project.lock().dependencies[key]
                .files
                .as_slice()
                .iter()
                .find(|file| &file.slot == slot)
                .context("Imported member lacks locked role")?;
            payload_bytes = payload_bytes
                .checked_add(content.lease().len())
                .context("Import payload size overflow")?;
            bytes = bytes
                .checked_add(
                    content
                        .lease()
                        .len()
                        .checked_mul(file.placements.as_slice().len() as u64)
                        .context("Import staging size overflow")?,
                )
                .context("Import staging size overflow")?;
        }
        ensure!(
            payload_bytes <= limits.total_bytes,
            "Interpreted import exceeds total byte allowance"
        );
        self.publication_bytes = bytes;
        Ok(self)
    }
}
