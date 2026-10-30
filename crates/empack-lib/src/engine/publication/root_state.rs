//! Stable associations retain the physical directory containing native coordination.
use super::*;
const ASSOCIATION: &str = "root-identity.json";
const ASSOCIATION_LIMIT: u64 = 1024;
const LOOKUP_ENTRIES: usize = 4096;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Association {
    schema: u32,
    persistent: [u8; 32],
    original: Binding,
}
fn read(directory: &Dir) -> Result<Option<Association>> {
    let mut file = match native::open_file(directory, ASSOCIATION) {
        Ok(file) => file,
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    copy_bounded(
        &mut file,
        &mut bytes,
        ASSOCIATION_LIMIT,
        &Cancellation::default(),
    )?;
    let association: Association =
        serde_json::from_slice(&bytes).context("Corrupt native root association")?;
    ensure!(
        association.schema == 1,
        "Unsupported native root association schema"
    );
    Ok(Some(association))
}
fn matches(association: &Association, name: &str, root: &ProjectReadRoot) -> Result<bool> {
    ensure!(
        binding_key(&association.original)? == name,
        "Native root association moved to another state directory"
    );
    let persistent = root
        .binding
        .persistent
        .context("Filesystem lacks persistent identity to verify its native state association")?;
    Ok(association.persistent == persistent)
}
impl Publisher {
    /// Read-only discovery never creates an association, lock or directory.
    pub(super) fn find_project_state(&self, root: &ProjectReadRoot) -> Result<Option<Dir>> {
        Ok(self
            .resolve_project_state(root)?
            .map(|(_, directory)| directory))
    }
    /// Retain only root identity, under publication coordination, before saving host work.
    /// This creates no project mutation or execution authority.
    pub(in crate::engine) fn retain_root_association(&self, root: &ProjectReadRoot) -> Result<()> {
        self.project_state(root)?;
        root.check_binding()
    }
    /// An association's opaque original key remains bound to this live root.
    pub(in crate::engine) fn bound_root_key(&self, root: &ProjectReadRoot) -> Result<String> {
        match self.resolve_project_state(root)? {
            Some((key, _)) => Ok(key),
            None => root_key(root),
        }
    }
    fn resolve_project_state(&self, root: &ProjectReadRoot) -> Result<Option<(String, Dir)>> {
        root.check_binding()?;
        let current = root_key(root)?;
        let (_, suffix) = current
            .strip_prefix("project-")
            .context("Invalid native root key")?
            .split_once('-')
            .context("Invalid native root key")?;
        let suffix = format!("-{suffix}");
        let mut selected = None;
        let mut unresolved = false;
        for (index, entry) in self.host.entries()?.enumerate() {
            ensure!(
                index < LOOKUP_ENTRIES,
                "Native state lookup exceeds entry limit"
            );
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if !name.starts_with("project-") || !name.ends_with(&suffix) {
                continue;
            }
            // Never follow links while searching another root's private state.
            let directory = self
                .host
                .open_dir_nofollow(name)
                .with_context(|| format!("Cannot inspect native state directory {name}"))?;
            native::reject_reparse(&directory.try_clone()?.into_std_file())?;
            match read(&directory)? {
                Some(association) => {
                    if !matches(&association, name, root)? {
                        ensure!(
                            name != current,
                            "Native state directory belongs to another persistent root"
                        );
                        continue;
                    }
                    ensure!(
                        selected.is_none(),
                        "Multiple native state directories claim this persistent root"
                    );
                    selected = Some((name.to_owned(), directory));
                }
                None if name == current => {
                    ensure!(
                        selected.is_none(),
                        "Multiple native state directories may belong to this root"
                    );
                    selected = Some((name.to_owned(), directory));
                }
                None => unresolved = true,
            }
        }
        ensure!(
            !unresolved,
            "Legacy native state may belong to this root but lacks persistent identity evidence; preserve it for explicit migration"
        );
        root.check_binding()?;
        Ok(selected)
    }
    pub(super) fn associate_project_state(
        &self,
        state: &Dir,
        root: &ProjectReadRoot,
    ) -> Result<()> {
        if let Some(association) = read(state)? {
            ensure!(
                association.persistent
                    == root
                        .binding
                        .persistent
                        .context("Filesystem lacks persistent object identity")?,
                "Native state association belongs to another root"
            );
            return Ok(());
        }
        let _lock = lock(state)?;
        // Another cooperating publisher may have established the same association.
        if let Some(association) = read(state)? {
            ensure!(
                association.persistent
                    == root
                        .binding
                        .persistent
                        .context("Filesystem lacks persistent object identity")?,
                "Native state association belongs to another root"
            );
            return Ok(());
        }
        root.check_binding()?;
        let record = Association {
            schema: 1,
            persistent: root.binding.persistent.context("Filesystem lacks persistent object identity; native state association is unsupported")?,
            original: root.binding.into(),
        };
        write_record(state, ASSOCIATION, &record)?;
        root.check_binding()
    }
}
