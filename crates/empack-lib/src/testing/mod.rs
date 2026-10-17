pub mod filesystem;

pub use filesystem::TempDirFixture;

/// Read both author documents in a trusted test fixture. This is not a native engine snapshot.
#[cfg(any(test, feature = "test-utils"))]
pub fn read_authoring(
    path: &std::path::Path,
) -> anyhow::Result<crate::engine::documents::DecodedIntent> {
    let binding_path = path.with_file_name("empack.bindings.yml");
    let bindings = match std::fs::read(binding_path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    crate::engine::documents::DocumentCodec.decode_authoring(
        &std::fs::read(path)?,
        bindings.as_deref(),
        "authoring fixture",
    )
}

/// Replace author inputs in a trusted fixture; production mutations use verified publication.
#[cfg(any(test, feature = "test-utils"))]
pub fn write_authoring(
    path: &std::path::Path,
    intent: &empack_core::model::ProjectIntent,
) -> anyhow::Result<()> {
    let documents = crate::engine::documents::DocumentCodec.encode_authoring(intent)?;
    std::fs::write(path, documents.intent)?;
    let binding_path = path.with_file_name("empack.bindings.yml");
    if let Some(bytes) = documents.bindings {
        std::fs::write(binding_path, bytes)?;
    } else {
        match std::fs::remove_file(binding_path) {
            Ok(()) => (),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
