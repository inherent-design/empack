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
    cases: tuple[str, ...] = ()
    required_env: tuple[str, ...] = ()

    @property
    def filter(self):
        binaries = " | ".join(f"binary(={name})" for name in self.binaries) or "all()"
        if self.cases:
            return f"({binaries}) & (" + " | ".join(f"test(={name})" for name in self.cases) + ")"
        return binaries


CLI = (
    "cli_arguments", "cli_automation", "cli_build", "cli_dependencies", "cli_entry", "cli_exit_codes",
    "cli_init", "cli_interactive", "cli_manifest", "cli_pending_build", "cli_status", "journey_authoring",
    "journey_content_kinds", "journey_selection",
    "journey_import", "journey_local_exports", "journey_pending", "journey_pending_import", "journey_pending_instance",
    "journey_publisher", "journey_verified_import", "smoke",
)
SUITES = {
    "deterministic": Suite(),
    "cli": Suite(CLI),
    "import-corpus": Suite(("journey_import",)),
    "pinned-import": Suite(("live_pinned_import",), ignored=True, timeout=1800),
    "telemetry": Suite(("cli_telemetry",), ignored=True, telemetry=True),
    "live-catalog": Suite(("live_cli_init", "runtime_catalog_smoke"), ignored=True),
    "live-provider": Suite(("live_provider_cli", "provider_catalog_smoke"), ignored=True,
                           required_env=("EMPACK_KEY_CURSEFORGE",)),
    "live-provider-cli": Suite(("live_provider_cli",), ignored=True, cases=(
        "live_cli_equivalent_provider_selectors_converge",
        "live_cli_pty_search_requires_choice_and_approval",
        "live_cli_selected_update_preserves_exact_pin_and_authoring",
        "live_cli_direct_url_retains_observed_content",
        "live_cli_runtime_migration_preserves_pins_and_verifies_consumer_outputs")),
    "live-modrinth": Suite(("provider_catalog_smoke",), ignored=True, cases=(
        "modrinth_mod_identity_and_exact_bytes", "modrinth_resource_pack_identity_and_exact_bytes",
        "modrinth_compatible_selection_and_bytes", "modrinth_mixed_project_selects_datapack_bytes",
        "modrinth_required_closure_and_all_selected_bytes")),
    "live-curseforge": Suite(("provider_catalog_smoke", "live_provider_cli"), ignored=True, cases=(
        "curseforge_identity_and_exact_bytes", "curseforge_compatible_selection_and_bytes",
        "live_cli_curseforge_selectors_converge"),
        required_env=("EMPACK_KEY_CURSEFORGE",)),
    "server-cli": Suite(("live_server_cli",), ignored=True, java=True),
    "live-runtime": Suite(("runtime_server_smoke",), ignored=True, java=True, timeout=3600,
                          required_env=("EMPACK_TEST_JAVA8_HOME",)),
    "runtime-current": Suite(("runtime_server_smoke",), ignored=True, java=True, timeout=1800,
                             cases=("runtime_managed_server_vanilla", "runtime_managed_server_neoforge",
                                    "runtime_server_fabric_classpath")),
    "live-assistance": Suite(("live_manual_assistance",), ignored=True, unix_only=True),
    "import-survey": Suite(("live_import_survey",), ignored=True, timeout=3600),
}


TEST_FEATURES = {"minimal": ("--no-default-features",), "all": ("--all-features",)}


def nextest_command(action, suite, config, test_features):
    """One reviewed discovery/execution selection, including its feature graph."""
    if action not in {"list", "run"} or test_features not in TEST_FEATURES:
        raise ValueError("Unknown test action or feature graph")
    command = ["cargo", "nextest", action, "--config-file", str(config),
               "--user-config-file", "none", "--profile", "verification",
               "--locked", "--workspace", *TEST_FEATURES[test_features],
               "-E", suite.filter, "--ignore-default-filter", "--run-ignored",
               "ignored-only" if suite.ignored else "default"]
    if action == "list":
        return command + ["--message-format", "json"]
    command += ["--no-tests", "fail", "--retries", "0", "--no-fail-fast"]
    if suite.ignored:
        command += ["--test-threads", "2"]
    return command
