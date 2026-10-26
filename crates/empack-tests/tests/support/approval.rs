//! Public resume approval without interpreting persisted records.
use crate::files;
use std::path::Path;

/// Fresh terminal approval must not inherit the earlier save's --yes decision.
pub fn decline_resume(invocation: &Path, root: &Path, state: &Path, args: &[&str], prompt: &str) {
    let cache = state.with_extension("cache");
    let paths = [invocation, root, state, &cache];
    let before: Vec<_> = paths
        .iter()
        .map(|path| path.exists().then(|| files::snapshot(path)))
        .collect();
    let mut command = empack_tests::harness::process::offline_empack_cmd(invocation);
    command
        .arg("--workdir")
        .arg(root)
        .arg("--state-dir")
        .arg(state)
        .arg("--cache-dir")
        .arg(&cache)
        .args(args);
    empack_tests::harness::terminal::decline(command, prompt);
    for (path, expected) in paths.into_iter().zip(before) {
        assert_eq!(
            path.exists().then(|| files::snapshot(path)),
            expected,
            "declined resume changed {}",
            path.display()
        );
    }
}
