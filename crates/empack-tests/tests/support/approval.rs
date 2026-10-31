//! Public decisions without interpreting persisted records or granting via stdin.
use empack_tests::harness::workspace::snapshot;
use std::path::Path;

/// Closed/piped stdin cannot grant approval; preview and terminal refusal preserve
/// the entire workspace and host store, including saved continuation evidence.
pub fn preserve_without_approval(
    invocation: &Path,
    root: &Path,
    state: &Path,
    args: &[&str],
    prompt: &str,
) {
    let cache = state.with_extension("cache");
    let paths = [invocation, root, state, &cache];
    let before: Vec<_> = paths
        .iter()
        .map(|path| path.exists().then(|| snapshot(path)))
        .collect();
    let command = || {
        let mut command = empack_tests::harness::process::offline_empack_cmd(invocation);
        command
            .arg("--workdir")
            .arg(root)
            .arg("--state-dir")
            .arg(state)
            .arg("--cache-dir")
            .arg(&cache)
            .args(args);
        command
    };
    let unchanged = || {
        for (path, expected) in paths.iter().zip(&before) {
            assert_eq!(
                &path.exists().then(|| snapshot(path)),
                expected,
                "unapproved operation changed {}: {args:?}",
                path.display()
            );
        }
    };
    for input in ["", "y\n", "n\n"] {
        assert_cmd::Command::from_std(command())
            .timeout(std::time::Duration::from_secs(30))
            .write_stdin(input)
            .assert()
            .code(2)
            .stderr(predicates::str::contains(
                "Noninteractive execution requires --yes",
            ));
        unchanged();
    }
    assert_cmd::Command::from_std(command())
        .timeout(std::time::Duration::from_secs(30))
        .args(["--yes", "--dry-run"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Dry run complete"));
    unchanged();
    empack_tests::harness::terminal::decline(command(), prompt);
    unchanged();
}
