//! Strict recipe codec shared by authored intent and retained build requests.
use super::*;
use empack_core::distribution::{Consumer, Delivery, Recipe, UpdateAuthority};

pub(crate) fn decode(value: &Value) -> Result<Recipe> {
    fields(value, &["consumer", "delivery", "environment", "updates"])?;
    let consumer = match text(required(value, "consumer")?)? {
        "modrinth" => Consumer::Modrinth,
        "curseforge" => Consumer::CurseForge,
        "prism" => Consumer::Prism,
        "server" => Consumer::Server,
        "empack" => Consumer::Empack,
        _ => bail!("Unknown distribution consumer"),
    };
    let delivery = match text(required(value, "delivery")?)? {
        "references" => Delivery::References,
        "bundled" => Delivery::Bundled,
        _ => bail!("Unknown dependency delivery"),
    };
    let environment = match text(required(value, "environment")?)? {
        "client" => Environments::Client,
        "server" => Environments::Server,
        "both" => Environments::Both,
        _ => bail!("Unknown distribution environment"),
    };
    let authority = match value
        .get("updates")
        .map(text)
        .transpose()?
        .unwrap_or("snapshot")
    {
        "snapshot" => UpdateAuthority::Snapshot,
        "platform" => UpdateAuthority::Platform,
        "empack" => UpdateAuthority::Empack,
        _ => bail!("Unknown update authority"),
    };
    Ok(Recipe::new(consumer, delivery, environment)?.with_update_authority(authority)?)
}

pub(crate) fn encode(recipe: &Recipe) -> Value {
    json!({
        "consumer": match recipe.consumer() {
            Consumer::Modrinth => "modrinth", Consumer::CurseForge => "curseforge",
            Consumer::Prism => "prism", Consumer::Server => "server", Consumer::Empack => "empack",
        },
        "delivery": match recipe.delivery() { Delivery::References => "references", Delivery::Bundled => "bundled" },
        "environment": match recipe.environments() { Environments::Client => "client", Environments::Server => "server", Environments::Both => "both" },
        "updates": match recipe.update_authority() { UpdateAuthority::Snapshot => "snapshot", UpdateAuthority::Platform => "platform", UpdateAuthority::Empack => "empack" },
    })
}

pub(crate) fn archive(value: &str) -> Result<DistributionArchive> {
    Ok(match value {
        "zip" => DistributionArchive::Zip,
        "tar.gz" => DistributionArchive::TarGz,
        "7z" => DistributionArchive::SevenZip,
        _ => bail!("Unknown distribution archive"),
    })
}
pub(crate) fn archive_name(value: DistributionArchive) -> &'static str {
    match value {
        DistributionArchive::Zip => "zip",
        DistributionArchive::TarGz => "tar.gz",
        DistributionArchive::SevenZip => "7z",
    }
}
pub(crate) fn decode_profile(value: &Value) -> Result<BuildProfile> {
    fields(
        value,
        &["consumer", "delivery", "environment", "updates", "format"],
    )?;
    let format = archive(text(required(value, "format")?)?)?;
    let mut recipe = value.clone();
    recipe.as_object_mut().unwrap().remove("format");
    Ok(BuildProfile {
        recipe: decode(&recipe)?,
        archive: format,
    })
}
pub(crate) fn encode_profile(value: &BuildProfile) -> Value {
    let mut recipe = encode(&value.recipe);
    recipe["format"] = json!(archive_name(value.archive));
    recipe
}
