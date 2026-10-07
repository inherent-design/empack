//! Exact derivative index updates shared by dependency mutations.
use crate::{
    application::process_runtime::Cancellation,
    engine::{content::SourceEvidencePolicy, project::WorkspaceSnapshot, snapshot::Observation},
};
use anyhow::{Context, Result, ensure};
use empack_core::{
    digest::ExpectedDigest,
    files::ManagedPath,
    model::ContentLayer,
    path::{PathSyntax, PortableRelPath},
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
fn path(name: &str) -> Result<PortableRelPath> {
    Ok(PortableRelPath::parse(name, PathSyntax::ProjectContent)?)
}
/// Preserve remaining index entries and all extension fields. Rebind the pack's index digest
/// after deleting exact observed entries; never pass a manifest label to a backend command.
pub(in crate::engine) fn refresh_index(
    workspace: &WorkspaceSnapshot,
    removals: &BTreeSet<ManagedPath>,
    documents: &mut BTreeMap<ManagedPath, Vec<u8>>,
    cancel: &Cancellation,
) -> Result<()> {
    refresh_index_with_updates(workspace, removals, &BTreeMap::new(), documents, cancel)
}
/// Update existing direct entries without losing their extension fields or changing the default
/// algorithm for retained entries. Selected removed metadata remains an explicit removal.
pub(in crate::engine) fn refresh_index_with_updates(
    workspace: &WorkspaceSnapshot,
    removals: &BTreeSet<ManagedPath>,
    updates: &BTreeMap<PortableRelPath, ExpectedDigest>,
    documents: &mut BTreeMap<ManagedPath, Vec<u8>>,
    cancel: &Cancellation,
) -> Result<()> {
    let removed: BTreeSet<_> = removals
        .iter()
        .filter_map(|target| match target {
            ManagedPath::BackendDocument(path)
            | ManagedPath::Content {
                layer: ContentLayer::Common,
                path,
            } => Some(path.clone()),
            _ => None,
        })
        .collect();
    ensure!(
        updates.keys().all(|path| !removed.contains(path)),
        "Index target is both updated and removed"
    );
    if removed.is_empty() && updates.is_empty() {
        return Ok(());
    }
    let index_path = path("pack/index.toml")?;
    let pack_path = path("pack/pack.toml")?;
    let read = |path: &PortableRelPath| -> Result<Option<Vec<u8>>> {
        match workspace.observations().entries().get(path) {
            Some(Observation::File(_)) => workspace.read_document(path, cancel),
            Some(Observation::Directory { .. } | Observation::Ancestor(_)) => {
                anyhow::bail!("Backend document is a directory")
            }
            _ => Ok(None),
        }
    };
    let (index, pack) = (read(&index_path)?, read(&pack_path)?);
    let (Some(index), Some(pack)) = (&index, &pack) else {
        ensure!(
            index.is_none() && pack.is_none(),
            "Backend index and pack documents must both exist"
        );
        return Ok(());
    };
    let index_bytes = index;
    let mut index: toml::Value = toml::from_str(std::str::from_utf8(index_bytes)?)?;
    let mut pack: toml::Value = toml::from_str(std::str::from_utf8(pack)?)?;
    let no_hashes = pack
        .get("options")
        .and_then(|options| options.get("no-internal-hashes"))
        .map(|value| value.as_bool().context("Invalid internal-hash option"))
        .transpose()?
        .unwrap_or(false);
    let reference = pack
        .as_table_mut()
        .context("Pack document must be a table")?
        .entry("index")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .context("Pack index reference must be a table")?;
    let selected = reference
        .get("file")
        .map(|value| value.as_str().context("Index path must be text"))
        .transpose()?
        .unwrap_or("index.toml");
    ensure!(
        selected.is_empty() || selected == "index.toml",
        "Unexpected backend index path"
    );
    reference.insert("file".into(), toml::Value::String("index.toml".into()));
    let hash = reference
        .get("hash")
        .map(|value| value.as_str().context("Index digest must be text"))
        .transpose()?
        .filter(|value| !value.is_empty());
    let publish_hash = hash.is_some() && !no_hashes;
    if let Some(hash) = hash {
        let declaration = ExpectedDigest::parse(
            reference
                .get("hash-format")
                .and_then(toml::Value::as_str)
                .context("Index digest has no algorithm")?,
            hash,
        )?;
        crate::engine::content::verify_observation(
            &mut index_bytes.as_slice(),
            &empack_core::model::ExpectedContent {
                digests: Some(empack_core::digest::DigestSet::new(vec![declaration])?),
                size: Some(index_bytes.len() as u64),
                accepted_observation: None,
            },
            index_bytes.len() as u64,
            SourceEvidencePolicy::Compatibility,
            crate::engine::content::InitialObservation::RequireEvidence,
            cancel,
        )?;
    }
    let files = index
        .as_table_mut()
        .context("Index must be a table")?
        .entry("files")
        .or_insert_with(|| toml::Value::Array(vec![]))
        .as_array_mut()
        .context("Index file entries must be an array")?;
    let mut paths = crate::engine::layout::CollisionIndex::default();
    let mut retained = Vec::new();
    let mut names = BTreeSet::new();
    let mut aliases = BTreeSet::new();
    for mut entry in files.drain(..) {
        let name = path(
            entry
                .get("file")
                .and_then(toml::Value::as_str)
                .context("Index entry lacks a file")?,
        )?;
        let alias = entry
            .get("alias")
            .map(|value| value.as_str().context("Index alias must be text"))
            .transpose()?
            .unwrap_or("");
        ensure!(
            aliases.insert((name.clone(), alias.to_owned())),
            "Repeated index file alias"
        );
        if names.insert(name.clone()) {
            paths.insert_file(&name)?;
        }
        if !removed.contains(&name) {
            for target in removed
                .iter()
                .chain(updates.keys())
                .filter(|target| **target != name)
            {
                let mut collision = crate::engine::layout::CollisionIndex::default();
                collision.insert_file(target)?;
                collision
                    .insert_file(&name)
                    .context("Index aliases a selected removal target")?;
            }
            if let Some(digest) = updates.get(&name) {
                let entry = entry
                    .as_table_mut()
                    .context("Index entry must be a table")?;
                ensure!(
                    !entry
                        .get("metafile")
                        .map(|value| value.as_bool().context("Invalid metafile flag"))
                        .transpose()?
                        .unwrap_or(false),
                    "Direct content is indexed as metadata"
                );
                if no_hashes {
                    entry.remove("hash");
                } else {
                    entry.insert("hash".into(), toml::Value::String(digest.hex()));
                }
                entry.insert(
                    "hash-format".into(),
                    toml::Value::String(digest.algorithm().name().into()),
                );
            }
            retained.push(entry);
        }
    }
    *files = retained;
    let index = toml::to_string(&index)?.into_bytes();
    if publish_hash {
        reference.insert("hash-format".into(), toml::Value::String("sha256".into()));
        reference.insert(
            "hash".into(),
            toml::Value::String(ExpectedDigest::Sha256(Sha256::digest(&index).into()).hex()),
        );
    } else {
        reference.remove("hash");
    }
    documents.insert(ManagedPath::BackendDocument(path("index.toml")?), index);
    documents.insert(
        ManagedPath::BackendDocument(path("pack.toml")?),
        toml::to_string(&pack)?.into_bytes(),
    );
    Ok(())
}
