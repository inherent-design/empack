"""Exact Rust identities and platform policy, separate from execution evidence."""

BOUNDARIES = {"U", "H", "N", "X", "I", "L", "C", "P", "W", "M"}


def compare_rust_inventory(inventory, bindings, api_ids, required_binaries=(), test_features="all",
                           excluded_binaries=()):
    if bindings.get("schema") != 3 or bindings.get("selection") != "workspace-feature-graphs":
        raise ValueError("Expected workspace feature-graph Rust classification")
    if test_features not in {"all", "minimal"}:
        raise ValueError("Unclassified Rust test feature graph")
    targets = bindings["targets"]
    if not targets or not set(targets.values()) <= {"macos", "linux", "windows"}:
        raise ValueError("Unknown Rust platform policy")
    platforms = inventory["rust-build-meta"]["platforms"]
    # These suites execute on the host; cross compilation is a separate check.
    target = platforms["host"]["platform"]["triple"]
    if platforms["targets"]:
        raise ValueError("Cross-compiled inventory is not host execution discovery")
    if target not in targets:
        raise ValueError(f"Unclassified Rust target: {target}")
    platform = targets[target]
    selected_binaries = set(required_binaries)
    excluded = set(excluded_binaries)
    if selected_binaries & excluded:
        raise ValueError("Rust binary cannot be both required and excluded")
    classified, names = {}, {}
    for row in bindings["tests"]:
        identity = row["id"]
        if identity in classified or not identity.startswith(row["binary"] + "::"):
            raise ValueError(f"Duplicate or inconsistent Rust identity: {identity}")
        if row["boundary"] not in BOUNDARIES or row["api"] not in api_ids:
            raise ValueError(f"Unknown Rust boundary or responsibility: {identity}")
        hosts = row["platforms"]
        if not hosts or len(set(hosts)) != len(hosts) or not set(hosts) <= set(targets.values()):
            raise ValueError(f"Unknown or empty Rust platform membership: {identity}")
        if type(row["ignored"]) is not bool:
            raise ValueError(f"Missing Rust ignore policy: {identity}")
        if not row["binary_name"] or names.setdefault(row["binary"], row["binary_name"]) != row["binary_name"]:
            raise ValueError(f"Inconsistent Rust binary name: {identity}")
        graphs = row["test_features"]
        if not graphs or len(set(graphs)) != len(graphs) or not set(graphs) <= {"all", "minimal"}:
            raise ValueError(f"Unknown or empty Rust test feature membership: {identity}")
        classified[identity] = row
    if not classified:
        raise ValueError("Rust classification must be nonempty")
    if (selected_binaries | excluded) - set(names.values()):
        raise ValueError("Required binary is absent from Rust classification")
    expected = {identity: row for identity, row in classified.items()
                if platform in row["platforms"] and test_features in row["test_features"]
                and row["binary_name"] not in excluded
                and (not selected_binaries or row["binary_name"] in selected_binaries)}
    discovered = {}
    for binary, suite in inventory["rust-suites"].items():
        outside = (suite["binary-name"] in excluded
                   or selected_binaries and suite["binary-name"] not in selected_binaries)
        if suite["status"] == "skipped" and outside and not suite["testcases"]:
            continue
        if suite["status"] != "listed":
            raise ValueError(f"Required Rust binary was not listed: {binary}")
        if outside:
            raise ValueError(f"Unexpected binary in fixed Rust selection: {binary}")
        if names.get(binary) != suite["binary-name"]:
            raise ValueError(f"Unclassified or renamed Rust binary: {binary}")
        for name, case in suite["testcases"].items():
            identity = f"{binary}::{name}"
            if identity in discovered:
                raise ValueError(f"Duplicate discovered Rust identity: {identity}")
            discovered[identity] = case
    if not discovered or len(discovered) != inventory["test-count"]:
        raise ValueError("Rust discovery is empty or has an inconsistent count")
    if discovered.keys() != expected.keys():
        raise ValueError(f"Rust classification changed on {target}: "
                         f"unclassified={sorted(discovered.keys() - expected.keys())}; "
                         f"missing={sorted(expected.keys() - discovered.keys())}")
    for identity, case in discovered.items():
        if case["ignored"] != expected[identity]["ignored"]:
            raise ValueError(f"Rust ignore policy changed: {identity}")
    return discovered
