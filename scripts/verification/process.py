"""Bounded command output and one attempt; never retry a timed-out mutation."""
import os
import re
import signal
import subprocess
import threading
from urllib.parse import urlsplit, urlunsplit

LOG_LIMIT = 4 * 1024 * 1024


def redact(text, environment):
    secrets = [value for name, value in environment.items()
               if len(value) >= 6 and re.search(r"KEY|TOKEN|SECRET|PASSWORD|CREDENTIAL|AUTH", name, re.I)]
    for value in sorted(secrets, key=len, reverse=True):
        text = text.replace(value, "<redacted>")

    def url(match):
        try:
            value = urlsplit(match.group())
            host = value.netloc.rsplit("@", 1)[-1]
            return urlunsplit((value.scheme, host, value.path,
                               "<redacted>" if value.query else "", ""))
        except ValueError:
            return "<redacted-url>"
    return re.sub(r"https?://[^\s<>\"']+", url, text)


def run(command, *, cwd, env, log, timeout, separate_stdout=False):
    captured = [bytearray(), bytearray()]
    truncated = [False, False]
    options = {"start_new_session": True} if os.name != "nt" else {
        "creationflags": subprocess.CREATE_NEW_PROCESS_GROUP}
    process = subprocess.Popen(command, cwd=cwd, env=env, stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE if separate_stdout else subprocess.STDOUT, **options)

    def drain(stream, index):
        while chunk := stream.read(65536):
            remaining = max(0, LOG_LIMIT - len(captured[index]))
            captured[index].extend(chunk[:remaining])
            truncated[index] |= len(chunk) > remaining

    readers = []
    for index, stream in enumerate((process.stdout, process.stderr)):
        if stream is not None:
            reader = threading.Thread(target=drain, args=(stream, index), daemon=True)
            reader.start()
            readers.append((reader, stream))
    timed_out = False
    interrupted = False
    try:
        process.wait(timeout=timeout)
    except (subprocess.TimeoutExpired, KeyboardInterrupt) as error:
        timed_out = True
        interrupted = isinstance(error, KeyboardInterrupt)
        if os.name == "nt":
            subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                           timeout=30, check=False)
        else:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        process.wait(timeout=30)
    for reader, stream in readers:
        reader.join(timeout=10)
        if reader.is_alive():
            raise RuntimeError("Command output did not retire after process completion")
        stream.close()
    output = []
    for index, buffer in enumerate(captured):
        raw = buffer.decode("utf-8", errors="replace")
        if truncated[index]:
            # Drop partial lines so a clipped secret cannot escape substitution.
            raw = raw.rsplit("\n", 1)[0] if "\n" in raw else ""
        clean = redact(raw, env)
        if truncated[index]:
            clean += "\n[output truncated at admitted log limit]\n"
        output.append(clean)
    log.write_text(output[1] + output[0], encoding="utf-8")
    if interrupted:
        raise KeyboardInterrupt
    return process.returncode, output[0], timed_out
