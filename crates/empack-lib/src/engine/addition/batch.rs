//! One resolved group for explicitly selected provider and direct-file requests.
use super::*;
use crate::engine::{
    dependency_content::{DependencyContent, DependencyContents},
    mrpack::LockedFileKey,
    providers::ProviderAddition,
    resources::ResourceRequest,
    runtime::{RetainedOutput, WorkScope},
};
use anyhow::{Context, ensure};
use empack_core::model::ResolutionLock;
use std::collections::BTreeMap;

pub struct ResolvedAdditionBatch {
    group: AdditionGroup,
    content: DependencyContents,
}
impl ResolvedAdditionBatch {
    pub fn group(&self) -> &AdditionGroup {
        &self.group
    }
    pub fn content(&self) -> &DependencyContents {
        &self.content
    }
    /// Every source has already resolved under one captured project revision. Combining groups
    /// grants no writer and cannot silently choose between colliding logical records.
    pub async fn combine(
        scope: &mut WorkScope,
        current: ResolvedProject,
        provider: Option<Box<ProviderAddition>>,
        files: Option<FileAddition>,
    ) -> Result<RetainedOutput<Self>> {
        Self::combine_with_content(scope, current, provider, None, files).await
    }
    /// Retain verified provider payloads alongside direct files in the same publication batch.
    pub async fn combine_with_content(
        scope: &mut WorkScope,
        current: ResolvedProject,
        provider: Option<Box<ProviderAddition>>,
        provider_content: Option<crate::engine::providers::ProviderContent>,
        files: Option<FileAddition>,
    ) -> Result<RetainedOutput<Self>> {
        let retained = ResourceRequest {
            memory_bytes: 64 << 20,
            ..Default::default()
        };
        let worker = scope.spawn_blocking(
            ResourceRequest {
                jobs: 1,
                ..retained
            },
            retained,
            move |cancel| {
                ensure!(
                    provider.is_some() || files.is_some(),
                    "Addition batch is empty"
                );
                let mut content = BTreeMap::new();
                if let Some(provider) = &provider {
                    for (key, dependency) in &provider.project().lock().dependencies {
                        cancel.check()?;
                        for file in dependency.files.as_slice() {
                            content.insert(
                                LockedFileKey {
                                    dependency: key.clone(),
                                    slot: file.slot.clone(),
                                },
                                DependencyContent::Reference,
                            );
                        }
                    }
                }
                if let Some(acquired) = &provider_content {
                    ensure!(acquired.complete(), "Provider content still requires input");
                    ensure!(
                        content.keys().eq(acquired.content().keys()),
                        "Provider content coverage differs from its resolved group"
                    );
                    content = acquired.content().clone();
                }
                if let Some(files) = &files {
                    for (key, file) in files.content() {
                        ensure!(
                            content.insert(key.clone(), file.clone()).is_none(),
                            "Addition groups share a logical content slot"
                        );
                    }
                }
                let group = match (&provider, &files) {
                    (Some(provider), None) => provider.group().clone(),
                    (None, Some(files)) => files.group().clone(),
                    (Some(provider), Some(files)) => {
                        let mut intent = current.intent().clone();
                        intent.roots.clear();
                        let mut lock = ResolutionLock {
                            intent_revision: current.lock().intent_revision,
                            resolver: "empack-batch-addition-v0.5".into(),
                            dependencies: BTreeMap::new(),
                            required_edges: BTreeMap::new(),
                            coverage: BTreeMap::new(),
                            runtime: current.lock().runtime.clone(),
                        };
                        for project in [provider.project(), files.project()] {
                            cancel.check()?;
                            ensure!(
                                project.lock().runtime == lock.runtime,
                                "Addition groups resolved different runtimes"
                            );
                            for (key, root) in &project.intent().roots {
                                ensure!(
                                    intent.roots.insert(key.clone(), root.clone()).is_none(),
                                    "Addition groups repeat a logical root"
                                );
                            }
                            for (key, dependency) in &project.lock().dependencies {
                                ensure!(
                                    lock.dependencies
                                        .insert(key.clone(), dependency.clone())
                                        .is_none(),
                                    "Addition groups repeat a logical installation"
                                );
                            }
                            lock.required_edges
                                .extend(project.lock().required_edges.clone());
                            lock.coverage.extend(project.lock().coverage.clone());
                        }
                        let source = DocumentCodec.decode_intent(
                            &DocumentCodec.encode_intent(&intent)?,
                            "addition batch",
                        )?;
                        lock.intent_revision = source.semantic_revision();
                        let resolved =
                            ResolvedProject::validate(intent, lock, source.semantic_revision())
                                .context("Incompatible addition groups")?;
                        AdditionGroup::from_resolved(&resolved)?
                    }
                    (None, None) => unreachable!("validated nonempty batch"),
                };
                Ok::<_, anyhow::Error>(Self { group, content })
            },
        )?;
        scope.accept(worker.wait().await?)?.transpose()
    }
}
