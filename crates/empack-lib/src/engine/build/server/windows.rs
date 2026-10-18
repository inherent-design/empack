//! Windows console entry points preserve failures and support unattended callers.
const HEADER: &str =
    "@echo off\r\nsetlocal DisableDelayedExpansion\r\ncd /d \"%~dp0\" || goto directory_error\r\n";
const FINISH: &str = ":finish\r\nif not defined EMPACK_NO_PAUSE pause\r\nexit /b %_EMPACK_EXIT%\r\n:directory_error\r\necho Cannot open the server directory. Extract the entire ZIP before running this script.\r\nset \"_EMPACK_EXIT=1\"\r\ngoto finish\r\n";

pub(super) fn start_batch(managed: bool) -> String {
    let preparation = if managed {
        "set \"_EMPACK_SAVED_PAUSE=%EMPACK_NO_PAUSE%\"\r\nset \"EMPACK_NO_PAUSE=1\"\r\ncall install_pack.bat\r\nset \"_EMPACK_EXIT=%errorlevel%\"\r\nset \"EMPACK_NO_PAUSE=%_EMPACK_SAVED_PAUSE%\"\r\nif not \"%_EMPACK_EXIT%\"==\"0\" goto finish\r\n"
    } else {
        "cd game || goto directory_error\r\n"
    };
    let command = if managed {
        "empack --workdir \"%cd%\" --yes instance launch -- "
    } else {
        ""
    };
    format!(
        "{HEADER}{preparation}echo Starting server. Java comes from JAVA_HOME when set, otherwise PATH.\r\nif defined JAVA_HOME (\r\n  {command}\"%JAVA_HOME%\\bin\\java.exe\" -jar server.jar %*\r\n) else (\r\n  {command}java -jar server.jar %*\r\n)\r\nset \"_EMPACK_EXIT=%errorlevel%\"\r\nif not \"%_EMPACK_EXIT%\"==\"0\" echo Server failed. Check the output above and the required Java version; managed packs also need empack on PATH.\r\necho Server stopped. If Minecraft requested EULA acceptance, review game\\eula.txt before restarting.\r\n{FINISH}"
    )
}

pub(super) fn install_batch(release: Option<&str>) -> String {
    let command = match release {
        Some(id) => format!("empack --workdir \"%cd%\" --yes instance prepare \"%cd%\\.empack-consumer\\release.json\" --sha256 {id} --layout game --side server\r\nset \"_EMPACK_EXIT=%errorlevel%\"\r\nif not \"%_EMPACK_EXIT%\"==\"0\" (\r\n  echo Preparation failed. Install empack on PATH and check the preceding download or subscription error.\r\n  goto finish\r\n)\r\n"),
        None => "if not exist game\\ goto directory_error\r\necho This bundled server already includes its pack and runtime files; no download is needed.\r\n".into(),
    };
    format!(
        "{HEADER}{command}echo Server pack ready. Run start.bat to start it.\r\necho Review and accept the Minecraft EULA yourself before playing.\r\nset \"_EMPACK_EXIT=0\"\r\n{FINISH}"
    )
}
