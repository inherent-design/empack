#!/usr/bin/env python3
"""Execute the dated compatibility selection through the shared verification runner."""
import argparse
from datetime import datetime, timezone
from pathlib import Path
import subprocess
import sys

from verification.compatibility import scheduled_suites
from verification.evidence import write_json


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=Path("target/compatibility"))
    parser.add_argument("--plan-only", action="store_true")
    args = parser.parse_args()
    day = datetime.now(timezone.utc).date()
    suites = scheduled_suites(day)
    args.output.mkdir(parents=True, exist_ok=True)
    write_json(args.output / "selection.json", {
        "schema": 1, "kind": "scheduled-compatibility-selection", "qualification": False,
        "day_utc": day.isoformat(), "suites": suites,
        "execution": "NOT_RUN", "product_features": "telemetry", "product_profile": "release",
    })
    if args.plan_only:
        return 0
    return subprocess.call([
        sys.executable, str(Path(__file__).with_name("verify.py")), *suites,
        "--product-features", "telemetry", "--product-profile", "release",
        "--test-features", "all", "--output", str(args.output / "runs"),
    ])


if __name__ == "__main__":
    raise SystemExit(main())
