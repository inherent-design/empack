//! Native capabilities and explicit, bounded Java diagnostics; no project or state ownership.
use super::*;
use crate::{
    application::cli::DoctorArgs,
    engine::diagnostics::{Diagnostic, DiagnosticPhase},
    terminal::display_line,
};
use empack_core::{java::JavaPolicy, model::GameVersion};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
enum JavaState {
    NotFound,
    Located,
    Checked,
    Failed,
}
#[derive(Serialize)]
struct JavaReport {
    program: Option<PathBuf>,
    state: JavaState,
    major: Option<u16>,
    problem: Option<String>,
    diagnostic: Option<Diagnostic>,
}
#[derive(Serialize)]
struct TargetJava {
    minecraft: String,
    recommended: Option<u16>,
    supported: Option<Vec<u16>>,
    actual_admitted: Option<bool>,
}
#[derive(Serialize)]
struct DoctorReport {
    schema: u32,
    version: &'static str,
    binary_os: &'static str,
    binary_arch: &'static str,
    executable: Option<PathBuf>,
    revision: Option<&'static str>,
    build_date: Option<&'static str>,
    native_archives: [&'static str; 3],
    network_checked: bool,
    java: JavaReport,
    target: Option<TargetJava>,
    complete: bool,
}
fn failure(report: &mut DoctorReport, error: &anyhow::Error) {
    report.complete = false;
    report.java.problem = Some(format!("{error:#}").chars().take(2048).collect());
    report.java.diagnostic = Some(Diagnostic::from_error(error, DiagnosticPhase::Verification));
}
/// Default inspection finds tools only; an explicit Java or Minecraft selection requests execution.
pub(in crate::application) async fn inspect(session: &dyn Session, args: DoctorArgs) -> Result<()> {
    ensure!(
        !(args.json && args.details),
        "Choose --json or --details, not both"
    );
    let selected_game = args
        .minecraft
        .as_deref()
        .map(GameVersion::parse)
        .transpose()?;
    let invocation = session.invocation().current_dir()?;
    let requested = args.java.is_some() || selected_game.is_some();
    ensure!(
        !session.config().app_config().dry_run || !requested,
        "Java probes require execution; omit --dry-run for doctor --java/--minecraft"
    );
    let program = args
        .java
        .as_ref()
        .map(|path| absolute(&invocation, path))
        .or_else(|| {
            session
                .process()
                .find_program("java")
                .map(|path| absolute(&invocation, Path::new(&path)))
        });
    let mut report = DoctorReport {
        schema: 1,
        version: env!("CARGO_PKG_VERSION"),
        binary_os: std::env::consts::OS,
        binary_arch: std::env::consts::ARCH,
        executable: std::env::current_exe().ok(),
        revision: option_env!("GIT_HASH"),
        build_date: option_env!("BUILD_DATE"),
        native_archives: ["zip", "tar.gz", "7z"],
        network_checked: false,
        java: JavaReport {
            state: if program.is_some() {
                JavaState::Located
            } else {
                JavaState::NotFound
            },
            program: program.clone(),
            major: None,
            problem: None,
            diagnostic: None,
        },
        target: selected_game.map(|game| {
            let policy = JavaPolicy::for_game(game.as_str());
            TargetJava {
                minecraft: game.as_str().into(),
                recommended: policy.recommended(),
                supported: policy.supported().map(<[u16]>::to_vec),
                actual_admitted: None,
            }
        }),
        complete: true,
    };
    let mut result = Ok(());
    if requested {
        let checked = match program {
            None => Err(anyhow::anyhow!(
                "Java was not found; select a local executable with doctor --java PATH"
            )),
            Some(program) => {
                scoped_with_feedback(
                    session,
                    ResourceGovernor::new(ResourceRequest {
                        jobs: 1,
                        memory_bytes: 8 << 20,
                        open_files: 16,
                        scratch_bytes: 0,
                    }),
                    args.quiet,
                    move |mut scope| async move {
                        crate::engine::java::inspect_scoped(&mut scope, program, invocation).await
                    },
                )
                .await
            }
        };
        match checked {
            Ok(major) => {
                report.java.state = JavaState::Checked;
                report.java.major = Some(major);
                if let Some(target) = &mut report.target {
                    let policy = JavaPolicy::for_game(&target.minecraft);
                    result = policy
                        .recommended()
                        .context(
                            "Java support is not established for the selected Minecraft version",
                        )
                        .and_then(|preferred| {
                            policy.check_actual(major, preferred).map_err(Into::into)
                        });
                    target.actual_admitted = target.supported.as_ref().map(|_| result.is_ok());
                }
            }
            Err(error) => {
                report.java.state = JavaState::Failed;
                result = Err(error);
            }
        }
        if let Err(error) = &result {
            failure(&mut report, error);
        }
    }
    let output = if args.json {
        crate::terminal::json_text(&report)?
    } else {
        render(&report, args.details)
    };
    session.display().status().message(&output);
    result
}
fn render(report: &DoctorReport, details: bool) -> String {
    let mut lines = vec![
        format!("empack {}", report.version),
        format!(
            "Binary platform: {}/{}",
            report.binary_os, report.binary_arch
        ),
    ];
    lines.push(match report.java.state {
        JavaState::NotFound => "Java: not found (optional for native authoring)".into(),
        JavaState::Located => format!(
            "Java: found at {} (version not checked)",
            display_line(&report.java.program.as_ref().unwrap().display().to_string())
        ),
        JavaState::Checked => format!("Java: reported major {}", report.java.major.unwrap()),
        JavaState::Failed => match &report.java.program {
            Some(path) => format!(
                "Java: check failed for {}",
                display_line(&path.display().to_string())
            ),
            None => "Java: not found for the requested check".into(),
        },
    });
    if let Some(target) = &report.target {
        lines.push(format!(
            "Minecraft {}: {}",
            display_line(&target.minecraft),
            match target.actual_admitted {
                Some(true) => "Java major admitted by empack's policy",
                Some(false) => "Java major is outside empack's policy",
                None => "Java support not verified",
            }
        ));
        if let Some(recommended) = target.recommended {
            lines.push(format!("Recommended Java: {recommended}"));
        }
        if let Some(majors) = &target.supported {
            lines.push(format!(
                "Supported majors: {}",
                majors
                    .iter()
                    .map(u16::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        lines.push(
            "Mods and loaders can have stricter requirements; startup was not tested.".into(),
        );
    }
    if let Some(problem) = &report.java.problem {
        for line in problem.lines() {
            lines.push(format!("  {}", display_line(line)));
        }
    }
    if details {
        if let Some(path) = report
            .java
            .program
            .as_ref()
            .filter(|_| matches!(report.java.state, JavaState::Checked))
        {
            lines.push(format!(
                "Java executable: {}",
                display_line(&path.display().to_string())
            ));
        }
        if let Some(path) = &report.executable {
            lines.push(format!(
                "Executable: {}",
                display_line(&path.display().to_string())
            ));
        }
        lines.push(format!(
            "Built from commit: {}",
            report.revision.unwrap_or("not recorded")
        ));
        lines.push(format!(
            "Build date: {}",
            report.build_date.unwrap_or("not recorded")
        ));
        lines
            .push("Pack operations: native engine; no external dependency manager required".into());
        lines.push("Archives: native ZIP, TAR.GZ and 7z output; bounded ZIP import".into());
        lines.push("Provider connectivity and credentials: not checked".into());
        if matches!(report.java.state, JavaState::Located) {
            lines.push("Use doctor --java PATH to probe a tool, or --minecraft VERSION to check the Java found on PATH against empack's policy.".into());
        }
    } else {
        lines.push("Call with --details for more information".into());
    }
    lines.join("\n")
}
