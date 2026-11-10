//! Standalone entry points enforce resolved majors without requiring empack.
use crate::engine::release::ReleaseJava;

pub(super) fn enforce(script: String, java: &ReleaseJava, windows: bool) -> String {
    let majors = java
        .supported
        .as_deref()
        .unwrap_or(std::slice::from_ref(&java.preferred));
    let admitted = majors.iter().map(u16::to_string).collect::<Vec<_>>();
    let description = admitted.join(", ");
    if windows {
        let checks = admitted
            .iter()
            .map(|major| format!("if \"%_EMPACK_JAVA_MAJOR%\"==\"{major}\" goto java_ready\r\n"))
            .collect::<String>();
        let probe = format!(
            "set \"_EMPACK_JAVA=java\"\r\nif defined JAVA_HOME set \"_EMPACK_JAVA=%JAVA_HOME%\\bin\\java.exe\"\r\nset \"_EMPACK_JAVA_MAJOR=\"\r\nset \"_EMPACK_JAVA_DUPLICATE=\"\r\nset \"_EMPACK_JAVA_PROBE_OK=\"\r\nfor /f \"tokens=1,2,3\" %%A in ('\"\"%_EMPACK_JAVA%\" -XshowSettings:properties -version 2^>^&1 ^&^& echo EMPACK_JAVA_PROBE_OK\"') do (\r\n  if \"%%A\"==\"EMPACK_JAVA_PROBE_OK\" set \"_EMPACK_JAVA_PROBE_OK=1\"\r\n  if \"%%A\"==\"java.specification.version\" if \"%%B\"==\"=\" (\r\n    if defined _EMPACK_JAVA_MAJOR set \"_EMPACK_JAVA_DUPLICATE=1\"\r\n    set \"_EMPACK_JAVA_MAJOR=%%C\"\r\n  )\r\n)\r\nif not defined _EMPACK_JAVA_PROBE_OK goto java_error\r\nif defined _EMPACK_JAVA_DUPLICATE goto java_error\r\nif \"%_EMPACK_JAVA_MAJOR:~0,2%\"==\"1.\" set \"_EMPACK_JAVA_MAJOR=%_EMPACK_JAVA_MAJOR:~2%\"\r\n{checks}:java_error\r\necho Cannot establish a compatible Java. This server admits Java {description}; choose JAVA_HOME or PATH accordingly.\r\nset \"_EMPACK_EXIT=1\"\r\ngoto finish\r\n:java_ready\r\n"
        );
        script.replace(
            "echo Starting server.",
            &format!("{probe}echo Starting server."),
        )
    } else {
        let choices = admitted.join("|");
        let probe = format!(
            r#"if ! _EMPACK_JAVA_PROPERTIES="$("$JAVA_PATH" -XshowSettings:properties -version 2>&1)"; then
  printf '%s\n' 'Cannot inspect Java. Select a working JAVA_HOME or Java on PATH.' >&2
  exit 1
fi
_EMPACK_JAVA_MAJOR=""
while IFS= read -r _EMPACK_JAVA_LINE; do
  if [[ "$_EMPACK_JAVA_LINE" =~ ^[[:blank:]]*java\.specification\.version[[:blank:]]*=[[:blank:]]*(1\.)?([0-9]+)[[:blank:]]*$ ]]; then
    if [[ -n "$_EMPACK_JAVA_MAJOR" ]]; then
      printf '%s\n' 'Java returned ambiguous version properties.' >&2
      exit 1
    fi
    _EMPACK_JAVA_MAJOR="${{BASH_REMATCH[2]}}"
  fi
done <<< "$_EMPACK_JAVA_PROPERTIES"
case "$_EMPACK_JAVA_MAJOR" in
  {choices}) ;;
  *) printf '%s\n' 'This server admits Java {description}; select a compatible JAVA_HOME or Java on PATH.' >&2; exit 1 ;;
esac
"#
        );
        script.replace("cd -- game\n", &format!("cd -- game\n{probe}"))
    }
}
