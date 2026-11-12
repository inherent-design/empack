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

pub(super) fn project(value: &empack_core::identity::ProviderProjectId) -> String {
    use empack_core::identity::ProviderProjectId;
    match value {
        ProviderProjectId::Modrinth(id) => format!("modrinth:{id}"),
        ProviderProjectId::CurseForge(id) => format!("curseforge:{id}"),
    }
}

pub(super) fn pin(value: &empack_core::model::ResolvedPin) -> String {
    use empack_core::identity::PinSelector;
    let selection = match &value.selection {
        PinSelector::ModrinthVersion(id) => id.to_string(),
        PinSelector::CurseForgeFile(id) => id.to_string(),
    };
    format!("{}@{selection}", project(&value.project))
}

pub(super) fn layer(value: empack_core::model::ContentLayer) -> &'static str {
    use empack_core::model::ContentLayer;
    match value {
        ContentLayer::Common => "common",
        ContentLayer::CommonOverride => "common overrides",
        ContentLayer::Client => "client",
        ContentLayer::Server => "server",
    }
}

pub(super) fn coverage(value: empack_core::model::Coverage) -> &'static str {
    use empack_core::model::Coverage;
    match value {
        Coverage::CompleteForSelection => "complete for this version",
        Coverage::Partial => "incomplete",
        Coverage::Unknown => "not recorded",
    }
}

pub(super) fn requirement(value: &empack_core::requirements::Requirement) -> String {
    use empack_core::requirements::Requirement;
    match value {
        Requirement::Unsupported => "not used".into(),
        Requirement::Required => "required".into(),
        Requirement::Optional(choice) => format!(
            "optional {} (default {})",
            crate::terminal::display_line(choice.key.as_str()),
            if choice.default_enabled { "on" } else { "off" }
        ),
    }
}
