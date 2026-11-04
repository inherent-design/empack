"""Fixed weekly service checks and bounded rotation; not candidate qualification."""
from datetime import date


def weekly_index(day: date, count: int) -> int:
    if count <= 0:
        raise ValueError("A rotating selection must be nonempty")
    # Monday origin keeps an ISO week together across calendar-year boundaries.
    return ((day - date(2026, 1, 5)).days // 7) % count


def scheduled_suites(day: date) -> list[str]:
    return ["import-corpus", "pinned-import", "live-provider-cli", "live-modrinth", "live-curseforge",
            "live-catalog", "publisher-cli", "live-runtime" if day.day <= 7 else "runtime-current"]
