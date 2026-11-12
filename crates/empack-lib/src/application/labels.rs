//! Shared human names for existing domain values; wire spellings stay with their codecs.
use empack_core::model::{ContentKind, LoaderKind};

pub(super) fn loader(value: LoaderKind) -> &'static str {
    match value {
        LoaderKind::Vanilla => "Vanilla",
        LoaderKind::Fabric => "Fabric",
        LoaderKind::Quilt => "Quilt",
        LoaderKind::Forge => "Forge",
        LoaderKind::NeoForge => "NeoForge",
    }
}

pub(super) fn content(value: ContentKind) -> &'static str {
    match value {
        ContentKind::Mod => "mod",
        ContentKind::ResourcePack => "resource pack",
        ContentKind::ShaderPack => "shader pack",
        ContentKind::DataPack => "data pack",
        ContentKind::World => "world",
        ContentKind::Config => "configuration",
        ContentKind::OtherFile => "file",
    }
}
