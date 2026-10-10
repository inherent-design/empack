use super::*;
use crate::engine::{
    documents::DocumentCodec, mrpack::tests::project, project::ProjectReader,
    publication::RecoveryReader, snapshot::SnapshotLimits,
};
use std::{fs, path::Path};

fn path(value: &str) -> PortableRelPath {
    PortableRelPath::parse(value, PathSyntax::ProjectContent).unwrap()
}
fn fixture(root: &Path) {
    let project = project(false, false);
    fs::write(
        root.join("empack.yml"),
        DocumentCodec.encode_intent(project.intent()).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("empack.lock"),
        DocumentCodec.encode_lock(&project).unwrap(),
    )
    .unwrap();
}
fn put(root: &Path, name: &str, bytes: &[u8]) {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}
fn capture(root: &Path, host: &Path) -> WorkspaceSnapshot {
    ProjectReader::new(RecoveryReader::new(host.join("private")))
        .capture_build(
            root,
            &[path("result.zip")],
            SnapshotLimits::default(),
            &Cancellation::default(),
        )
        .unwrap()
}
fn bytes(file: &RenderedTemplate) -> Vec<u8> {
    let mut result = vec![];
    file.content
        .lease()
        .open()
        .read_to_end(&mut result)
        .unwrap();
    result
}
#[test]
fn captured_templates_keep_nested_paths_side_precedence_and_user_inputs() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    fixture(root.path());
    put(
        root.path(),
        "templates/common/nested/settings.template",
        b"common {{NAME}}",
    );
    put(
        root.path(),
        "templates/client/nested/settings.template",
        b"client {{NAME}} {{MC_VERSION}} {{MODLOADER_NAME}} {{MODLOADER_VERSION}}",
    );
    put(
        root.path(),
        "templates/server/nested/settings.template",
        b"server {{VERSION}}",
    );
    put(root.path(), "templates/common/icon.bin", &[0, 255, 1]);
    put(
        root.path(),
        "templates/common/literal.txt",
        b"{{UNDEFINED}}",
    );
    put(root.path(), "templates/common/notes.txt", b"{{NAME}}");
    put(
        root.path(),
        "templates/common/run.sh.template",
        b"printf '%s' {{shell_quote NAME}}",
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            root.path().join("templates/common/run.sh.template"),
            fs::Permissions::from_mode(0o555),
        )
        .unwrap();
    }
    let workspace = capture(root.path(), host.path());
    let options = TemplateOptions {
        modes: BTreeMap::from([(path("templates/common/literal.txt"), TemplateMode::Copy)]),
        ..Default::default()
    };
    for (target, expected, source) in [
        (
            BuildTarget::Client,
            "client Test 1.20.1 fabric 0.16.0",
            "client",
        ),
        (
            BuildTarget::ClientFull,
            "client Test 1.20.1 fabric 0.16.0",
            "client",
        ),
        (BuildTarget::Server, "server alpha", "server"),
        (BuildTarget::ServerFull, "server alpha", "server"),
    ] {
        let rendered =
            prepare_templates(&workspace, target, &options, &Cancellation::default()).unwrap();
        assert_eq!(rendered.files().len(), 5);
        let settings = &rendered.files()[&path("nested/settings")];
        assert_eq!(bytes(settings), expected.as_bytes());
        assert_eq!(
            settings.input.source,
            path(&format!("templates/{source}/nested/settings.template"))
        );
        assert_eq!(
            settings.replaces,
            Some(path("templates/common/nested/settings.template"))
        );
        assert_eq!(bytes(&rendered.files()[&path("icon.bin")]), [0, 255, 1]);
        assert_eq!(
            bytes(&rendered.files()[&path("literal.txt")]),
            b"{{UNDEFINED}}"
        );
        assert_eq!(bytes(&rendered.files()[&path("notes.txt")]), b"Test");
        #[cfg(unix)]
        assert_eq!(
            rendered.files()[&path("run.sh")].permissions,
            FilePermissions {
                readonly: true,
                executable: true
            }
        );
    }
    workspace
        .root()
        .revalidate(workspace.observations(), &Cancellation::default())
        .unwrap();
    assert!(!root.path().join("dist").exists());
    assert!(!host.path().join("private").exists());
}
#[test]
fn template_failures_never_return_partial_outputs_or_modify_project() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    fixture(root.path());
    put(root.path(), "templates/common/a.template", b"ok");
    for invalid in [b"{{MISSING}}".as_slice(), b"{{#if NAME}}", &[255, 0]] {
        put(root.path(), "templates/common/z.template", invalid);
        let workspace = capture(root.path(), host.path());
        assert!(
            prepare_templates(
                &workspace,
                BuildTarget::Client,
                &TemplateOptions::default(),
                &Cancellation::default()
            )
            .is_err()
        );
        workspace
            .root()
            .revalidate(workspace.observations(), &Cancellation::default())
            .unwrap();
    }
    put(
        root.path(),
        "templates/common/z.template",
        b"{{NAME}}{{NAME}}{{NAME}}",
    );
    let workspace = capture(root.path(), host.path());
    for limits in [
        TemplateLimits {
            output_bytes: 8,
            ..Default::default()
        },
        TemplateLimits {
            total_bytes: 13,
            ..Default::default()
        },
        TemplateLimits {
            input_bytes: 3,
            ..Default::default()
        },
        TemplateLimits {
            entries: 1,
            ..Default::default()
        },
    ] {
        assert!(
            prepare_templates(
                &workspace,
                BuildTarget::Client,
                &TemplateOptions {
                    limits,
                    ..Default::default()
                },
                &Cancellation::default()
            )
            .is_err()
        );
    }
    let cancel = Cancellation::default();
    cancel.cancel();
    assert!(
        prepare_templates(
            &workspace,
            BuildTarget::Client,
            &TemplateOptions::default(),
            &cancel
        )
        .is_err()
    );
    assert!(!root.path().join("dist").exists());
}
#[test]
fn template_projection_rejects_ambiguous_output_and_stale_input() {
    let root = tempfile::tempdir().unwrap();
    let host = tempfile::tempdir().unwrap();
    fixture(root.path());
    put(root.path(), "templates/common/file.template", b"{{NAME}}");
    put(root.path(), "templates/common/file", b"ambiguous");
    let workspace = capture(root.path(), host.path());
    assert!(
        prepare_templates(
            &workspace,
            BuildTarget::Client,
            &TemplateOptions::default(),
            &Cancellation::default()
        )
        .is_err()
    );
    fs::remove_file(root.path().join("templates/common/file")).unwrap();
    for name in ["templates/client/FILE", "templates/client/file/nested"] {
        put(root.path(), name, b"ambiguous");
        let workspace = capture(root.path(), host.path());
        assert!(
            prepare_templates(
                &workspace,
                BuildTarget::Client,
                &TemplateOptions::default(),
                &Cancellation::default()
            )
            .is_err()
        );
        fs::remove_file(root.path().join(name)).unwrap();
    }
    let workspace = capture(root.path(), host.path());
    let options = TemplateOptions {
        modes: BTreeMap::from([(path("templates/unknown"), TemplateMode::Copy)]),
        ..Default::default()
    };
    assert!(
        prepare_templates(
            &workspace,
            BuildTarget::Client,
            &options,
            &Cancellation::default()
        )
        .is_err()
    );
    assert!(
        prepare_templates(
            &workspace,
            BuildTarget::Mrpack,
            &TemplateOptions::default(),
            &Cancellation::default()
        )
        .is_err()
    );
    put(root.path(), "templates/common/file.template", b"changed");
    assert!(
        prepare_templates(
            &workspace,
            BuildTarget::Client,
            &TemplateOptions::default(),
            &Cancellation::default()
        )
        .is_err()
    );
}

#[test]
fn embedded_defaults_share_escaping_and_enforce_output_budget() {
    let project = project(false, false);
    let name = "line\nkey=value\\suffix";
    let source = include_str!("../../../templates/server/server.properties.template");
    // Both checkout conventions are valid properties documents. Escaping must
    // keep metadata on one logical line without requiring a host newline style.
    let lf = source.replace("\r\n", "\n");
    for source in [lf.clone(), lf.replace('\n', "\r\n")] {
        let rendered = render_default(
            &project,
            &source,
            [("NAME".to_owned(), name.to_owned())],
            4096,
            &Cancellation::default(),
        )
        .unwrap();
        let rendered = String::from_utf8(rendered).unwrap();
        let expected = format!("server-name={}", encoding::properties_value(name));
        assert_eq!(rendered.lines().filter(|line| *line == expected).count(), 1);
        assert!(!rendered.contains("\nkey=value"));
    }
    assert!(render_default(&project, source, [], 4, &Cancellation::default(),).is_err());
}
