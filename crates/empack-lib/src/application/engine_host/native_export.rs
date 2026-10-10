//! Native consumer build selection; the engine captures inputs and owns publication.
use super::*;
use crate::engine::api::{NativeExportRequest, OperationPreview};
use empack_core::{
    model::DistributionArchive,
    path::{PathSyntax, PortableRelPath},
};
pub(super) async fn build(
    session: &dyn Session,
    args: &crate::application::BuildArgs,
) -> Result<()> {
    ensure!(
        args.targets == ["empack"],
        "Select the native consumer alone until combined consumer recipes are enabled"
    );
    ensure!(
        args.delivery.is_none()
            && args.environment.is_none()
            && args.updates.is_none()
            && !args.continue_build
            && !args.clean
            && !args.optional_defaults
            && args.optional_choices.is_empty()
            && !args.allow_optional_metadata_loss
            && args.downloads_dir.is_none()
            && !args.open_downloads
            && args.wait_downloads.is_none()
            && args.associate_downloads.is_empty(),
        "Native export preserves choices and requires exact materialized content; download/continuation and legacy cleanup options do not apply"
    );
    let (invocation, root) = project_path(session)?;
    let request = NativeExportRequest {
        artifact: PortableRelPath::parse("release.empack", PathSyntax::ArtifactName)?,
        archive: args.format.map(|format| match format {
            crate::application::cli::CliArchiveFormat::Zip => DistributionArchive::Zip,
            crate::application::cli::CliArchiveFormat::TarGz => DistributionArchive::TarGz,
            crate::application::cli::CliArchiveFormat::SevenZ => DistributionArchive::SevenZip,
        }),
    };
    let engine = engine(session.config().app_config(), &invocation)?;
    let result = async {
        let Preparation::Ready(prepared) =
            cancellable(session, engine.prepare(root, request)).await?
        else {
            anyhow::bail!("Native export requires input");
        };
        let OperationPreview::NativeExport(view) = prepared.view() else {
            anyhow::bail!("Unexpected native export preview");
        };
        session.display().status().info(&format!(
            "Release {}: {} file placements in dist/{}",
            view.release,
            view.files,
            view.artifact.as_str()
        ));
        apply(session, &engine, prepared, "Native release", |receipt| {
            let ExecutionReceipt::NativeExport(receipt) = receipt else {
                anyhow::bail!("Unexpected native export receipt");
            };
            Ok(format!(
                "Published dist/{}; release SHA-256 {}",
                receipt.preview.artifact.as_str(),
                receipt.preview.release
            ))
        })
        .await
    }
    .await;
    engine.shutdown().await;
    result
}
