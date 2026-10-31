"""Fixed phase breadcrumbs; these records never establish test acceptance."""
from contextlib import contextmanager
from datetime import datetime, timezone
import time

from .evidence import write_json


@contextmanager
def phase(directory, name):
    # Names are fixed runner labels. Do not record command arguments, environment,
    # child output or exception text: terminal logs have separate redaction rules.
    path = directory / f"{name}.progress.json"
    started = time.monotonic()
    record = dict(schema=1, kind="verification-progress", qualification=False,
                  phase=name, state="entered", started_at=datetime.now(timezone.utc).isoformat())

    def emit():
        available = True
        try:
            write_json(path, record)
        except OSError:
            available = False
        # Diagnostic storage/console failures must not replace the command's
        # actual outcome or prevent its caller from retaining that outcome.
        try:
            if not available:
                print(f"[verify] {name}: progress file unavailable", flush=True)
            print(f"[verify] {name}: {record['state']}", flush=True)
        except OSError:
            pass

    emit()
    try:
        yield
    except BaseException as error:
        record["state"] = "interrupted" if isinstance(error, KeyboardInterrupt) else "raised"
        raise
    else:
        # A returned command can still have a nonzero exit status. Its receipt,
        # not this breadcrumb, determines success, failure or missing evidence.
        record["state"] = "returned"
    finally:
        record.update(finished_at=datetime.now(timezone.utc).isoformat(),
                      elapsed_seconds=time.monotonic() - started)
        emit()
