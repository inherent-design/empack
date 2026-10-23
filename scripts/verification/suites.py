"""Fixed selections shared by local tasks and CI; not an extensible workflow DSL."""
from dataclasses import dataclass


@dataclass(frozen=True)
class Suite:
    binaries: tuple[str, ...] = ()
    ignored: bool = False
    telemetry: bool = False
    unix_only: bool = False
    java: bool = False
    timeout: int = 1800

    @property
    def filter(self):
        return " | ".join(f"binary(={name})" for name in self.binaries) or "all()"


CLI = (
    "cli_arguments", "cli_build", "cli_dependencies", "cli_entry", "cli_exit_codes",
    "cli_init", "cli_interactive", "cli_pending_build", "journey_authoring",
    "journey_import", "journey_local_exports", "journey_verified_import", "smoke",
)
SUITES = {
    "deterministic": Suite(),
    "cli": Suite(CLI),
    "telemetry": Suite(("cli_telemetry",), ignored=True, telemetry=True),
    "live-catalog": Suite(("live_cli_init", "runtime_catalog_smoke"), ignored=True),
    "live-provider": Suite(("live_provider_cli", "provider_catalog_smoke"), ignored=True),
    "server-cli": Suite(("live_server_cli",), ignored=True, java=True),
    "live-runtime": Suite(("runtime_server_smoke",), ignored=True, java=True, timeout=3600),
    "live-assistance": Suite(("live_manual_assistance",), ignored=True, unix_only=True),
    "import-survey": Suite(("live_import_survey",), ignored=True, timeout=3600),
}
