//! Provider constraints through host inspection and recorded synchronization.
use super::*;
use crate::application::{cli::ManifestCommand, engine_host::cli::manifest};
use empack_core::model::{ProviderSelector, SourceIntent};

#[tokio::test]
async fn host_manifest_provider_constraints_match_recorded_synchronization() {
    for change in ["display", "pin", "selector", "automatic"] {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path()).await;
        let project = root.path().join("project");
        let mut server = Server::new_async().await;
        records(&mut server).await;
        add(
            root.path(),
            &server,
            vec![input("renderer", Some("my-alias"))],
            true,
            false,
        )
        .await
        .unwrap();
        let selected = read(&project);
        let mut intent = selected.intent().clone();
        let dependency = intent
            .roots
            .get_mut(&DependencyKey::parse("my-alias").unwrap())
            .unwrap();
        match change {
            "display" => intent.metadata.description = Some("Author display edit".into()),
            "pin" => {
                dependency.selection = DependencySelection::ProviderPin(
                    PinSelector::ModrinthVersion(ModrinthVersionId::parse("RootVer2").unwrap()),
                )
            }
            "selector" => {
                dependency.source = SourceIntent::ProviderSelector(
                    ProviderSelector::new(ProviderKind::Modrinth, "different-project").unwrap(),
                )
            }
            "automatic" => dependency.selection = DependencySelection::Automatic,
            _ => unreachable!(),
        }
        // Component setup uses the codec; expected facts below are literal
        // provider fixture identities, not another call to the validator.
        let author = DocumentCodec.encode_intent(&intent).unwrap();
        fs::write(project.join("empack.yml"), &author).unwrap();
        let before = snapshot(root.path());
        let (inspection, messages) = session(root.path(), true, false).record_messages();
        manifest::inspect(&inspection, ManifestCommand::Diff { json: true })
            .await
            .unwrap();
        assert_eq!(
            snapshot(root.path()),
            before,
            "{change}: inspection mutated storage"
        );
        let report: Value = {
            let messages = messages.lock().unwrap();
            assert_eq!(messages.len(), 1);
            serde_json::from_str(&messages[0]).unwrap()
        };
        assert_eq!(report["lock_status"], "stale", "{change}: {report}");
        assert_eq!(report["runtime_satisfied"], true);
        let rows = report["dependencies"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        let row = rows.iter().find(|row| row["key"] == "my-alias").unwrap();
        let satisfied = matches!(change, "display" | "automatic");
        assert_eq!(row["selection_satisfied"], satisfied, "{change}: {row}");
        assert_eq!(
            row["canonical_project"],
            json!({"provider":"modrinth", "project":"Root0001"})
        );
        assert_eq!(
            row["locked_selection"],
            json!({"provider":"modrinth", "id":"RootVer1"})
        );
        if change == "selector" {
            assert_eq!(row["source_binding_current"], false);
            assert_eq!(row["reason"], "Locked identity differs from source intent");
        }
        if change == "pin" {
            assert_eq!(
                row["effective"]["selection"]["pin"],
                json!({"provider":"modrinth", "id":"RootVer2"})
            );
            assert_eq!(row["reason"], "Lock does not satisfy requested pin");
        }
        if change == "automatic" {
            assert_eq!(row["release_policy"], "prefer-stable");
            assert_eq!(row["effective"]["selection"], json!({"mode":"automatic"}));
        }
        let retained = rows
            .iter()
            .find(|row| row["key"] == "required-library")
            .unwrap();
        assert_eq!(retained["explicit"], false);
        assert!(retained["effective"].is_null());
        assert_eq!(
            retained["locked_selection"],
            json!({"provider":"modrinth", "id":"NeedVer1"})
        );
        assert_eq!(
            retained["reason"],
            "Retained lock entry; absence from roots is not deletion authority"
        );
        let outcome =
            synchronize(&session(root.path(), true, false), sync_request(&selected)).await;
        if satisfied {
            outcome.unwrap();
            let updated = read(&project);
            assert_eq!(updated.lock().dependencies, selected.lock().dependencies);
            assert_eq!(
                updated.lock().required_edges,
                selected.lock().required_edges
            );
            assert_eq!(fs::read(project.join("empack.yml")).unwrap(), author);
        } else {
            let error = outcome.unwrap_err();
            assert!(
                format!("{error:#}").contains("requires resolution"),
                "{error:#}"
            );
            assert_eq!(
                snapshot(root.path()),
                before,
                "{change}: refused sync changed input/storage"
            );
        }
    }
}
