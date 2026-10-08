//! Stable import facts for continuation. Execution-only URLs are deliberately excluded.
use super::*;
use crate::engine::providers::{DependencyRelation, EnvironmentEvidence};
use empack_core::{digest::ContentId, identity::PinSelector, model::ContentKind};
use sha2::{Digest, Sha256};

struct Facts(Sha256);
impl Facts {
    fn bytes(&mut self, bytes: &[u8]) {
        self.0.update((bytes.len() as u64).to_le_bytes());
        self.0.update(bytes);
    }
    fn text(&mut self, text: &str) {
        self.bytes(text.as_bytes());
    }
    fn count(&mut self, count: usize) {
        self.bytes(&(count as u64).to_le_bytes());
    }
    fn optional(&mut self, value: Option<&str>) {
        self.bytes(&[u8::from(value.is_some())]);
        if let Some(value) = value {
            self.text(value);
        }
    }
    fn project(&mut self, value: &empack_core::identity::ProviderProjectId) {
        use empack_core::identity::ProviderProjectId;
        self.text(match value {
            ProviderProjectId::Modrinth(_) => "modrinth",
            ProviderProjectId::CurseForge(_) => "curseforge",
        });
        self.text(&value.to_string());
    }
    fn pin(&mut self, value: &PinSelector) {
        match value {
            PinSelector::ModrinthVersion(id) => {
                self.text("modrinth");
                self.text(id.as_str());
            }
            PinSelector::CurseForgeFile(id) => {
                self.text("curseforge");
                self.text(&id.to_string());
            }
        }
    }
    fn environment(&mut self, value: &EnvironmentEvidence) {
        self.optional(value.version.as_deref());
        self.optional(value.client.as_deref());
        self.optional(value.server.as_deref());
    }
    fn expected(&mut self, expected: &ExpectedContent) {
        self.count(
            expected
                .digests
                .as_ref()
                .map_or(0, |set| set.values().len()),
        );
        if let Some(set) = &expected.digests {
            for digest in set.values() {
                self.text(digest.algorithm().name());
                self.bytes(digest.bytes());
            }
        }
        self.bytes(&[u8::from(expected.size.is_some())]);
        if let Some(size) = expected.size {
            self.bytes(&size.to_le_bytes());
        }
        self.bytes(&[u8::from(expected.accepted_observation.is_some())]);
        if let Some(id) = &expected.accepted_observation {
            self.bytes(id.bytes());
        }
    }
    fn kinds(&mut self, kinds: &[ContentKind]) {
        self.count(kinds.len());
        for kind in kinds {
            self.text(crate::engine::documents::kind_name(*kind));
        }
    }
}
impl ImportContentPlan {
    /// Source bytes and canonical catalog facts must still match before saved associations
    /// can be reused. A refreshed signed URL alone does not stale an exact selection.
    pub fn resume_revision(&self) -> ContentId {
        let mut facts = Facts(Sha256::new());
        facts.text("empack/import-resume-facts/1");
        facts.bytes(self.imported.source_id().bytes());
        facts.count(self.needs.len());
        for need in &self.needs {
            facts.text(&need.key.selector());
            facts.expected(&need.expected);
            match &need.source {
                ImportedAcquisition::Downloads(_) => facts.text("download"),
                ImportedAcquisition::Embedded(member) => {
                    facts.text("embedded");
                    facts.text(member.as_str());
                }
            }
            facts.bytes(&[u8::from(need.permissions.is_some())]);
            if let Some(mode) = need.permissions {
                facts.bytes(&[u8::from(mode.readonly), u8::from(mode.executable)]);
            }
        }
        facts.count(self.providers.records().len());
        for (pin, record) in self.providers.records() {
            facts.project(&pin.project);
            facts.pin(&pin.selection);
            facts.project(&record.project.id);
            facts.text(&record.project.slug);
            facts.text(&record.project.title);
            facts.kinds(record.project.kinds.as_slice());
            facts.environment(&record.project.environment);
            facts.kinds(record.kinds.as_slice());
            facts.project(&record.pin.project);
            facts.pin(&record.pin.selection);
            facts.environment(&record.environment);
            for values in [&record.game_versions, &record.loaders] {
                facts.count(values.len());
                for value in values {
                    facts.text(value);
                }
            }
            facts.count(record.files.as_slice().len());
            for file in record.files.as_slice() {
                facts.text(&file.filename);
                facts.bytes(&[u8::from(file.primary)]);
                facts.optional(file.role.as_deref());
                facts.expected(&file.expected);
            }
            facts.text(match record.coverage {
                Coverage::CompleteForSelection => "complete",
                Coverage::Partial => "partial",
                Coverage::Unknown => "unknown",
            });
            facts.count(record.dependencies.len());
            for dependency in &record.dependencies {
                facts.bytes(&[u8::from(dependency.project.is_some())]);
                if let Some(project) = &dependency.project {
                    facts.project(project);
                }
                facts.bytes(&[u8::from(dependency.pin.is_some())]);
                if let Some(pin) = &dependency.pin {
                    facts.pin(pin);
                }
                facts.optional(dependency.filename.as_deref());
                facts.text(match dependency.relation {
                    DependencyRelation::Required => "required",
                    DependencyRelation::Optional => "optional",
                    DependencyRelation::Incompatible => "incompatible",
                    DependencyRelation::Embedded => "embedded",
                    DependencyRelation::Tool => "tool",
                    DependencyRelation::Include => "include",
                });
            }
        }
        ContentId::from_sha256(facts.0.finalize().into())
    }
}
