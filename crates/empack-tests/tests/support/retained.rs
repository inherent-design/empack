//! Native fault placement only; continuation identities and actions stay public.
use std::path::{Path, PathBuf};

/// Locate one known fixture payload in native retained storage, without decoding
/// private continuation records or deriving the expected bytes from stored data.
pub fn backing(state: &Path, expected: &[u8]) -> PathBuf {
    let matches: Vec<_> = crate::files::snapshot(state)
        .into_iter()
        .filter(|(path, bytes)| path.extension().is_some_and(|e| e == "blob") && bytes == expected)
        .map(|(path, _)| state.join(path))
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "fault must target exactly one known backing"
    );
    matches.into_iter().next().unwrap()
}
