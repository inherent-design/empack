#!/usr/bin/env python3
"""Exercise JAR -> native empack -> Java through a real Unix controlling terminal.

Use a prepared disposable Prism instance carrying .empack-bootstrap-fixture.
This checks console/ownership behavior, not Minecraft or Prism GUI acceptance.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import pty
import select
import signal
import subprocess
import termios
import time


CONSOLE = r'''
import java.io.*;
import java.lang.management.ManagementFactory;
import java.nio.file.*;
public class ConsoleFixture {
    public static void main(String[] args) throws Exception {
        String pid = ManagementFactory.getRuntimeMXBean().getName().split("@")[0];
        Files.write(Paths.get(System.getProperty("empack.fixture.pid")), pid.getBytes("UTF-8"));
        Runtime.getRuntime().addShutdownHook(new Thread(() -> System.out.println("FIXTURE_SHUTDOWN")));
        System.out.println("CONSOLE_READY");
        BufferedReader input = new BufferedReader(new InputStreamReader(System.in, "UTF-8"));
        String line;
        while ((line = input.readLine()) != null) {
            System.out.println("CONSOLE_INPUT=" + line);
            if (line.equals("stop")) return;
        }
    }
}
'''


def run_case(args, home, mode):
    result = home / (mode + ".json")
    runtime_pid = home / (mode + ".runtime-pid")
    jar_pid = home / (mode + ".jar-pid")
    host_pid, master = pty.fork()
    if host_pid == 0:
        try:
            before = termios.tcgetattr(0)
            group = os.tcgetpgrp(0)
            command = [str(args.java), "-jar", str(args.jar), "--engine-version", args.version,
                       "--engine", str(args.engine), *(["--development-engine"] if args.development_engine else []),
                       "--", "--workdir", str(args.instance), "--state-dir", str(args.state_dir), "--yes",
                       "instance", "launch", "--java-runtime", "--", str(args.java),
                       "-Dempack.fixture.pid=" + str(runtime_pid), "-cp", str(home), "ConsoleFixture"]
            child = subprocess.Popen(command)
            jar_pid.write_text(str(child.pid))
            status = child.wait()
            result.write_text(json.dumps({"mode": mode, "status": status,
                                          "foreground_restored": os.tcgetpgrp(0) == group,
                                          "settings_restored": termios.tcgetattr(0) == before}))
            os._exit(0)
        except BaseException:
            import traceback
            traceback.print_exc()
            os._exit(1)
    output = bytearray()
    finished = False
    sent = False
    deadline = time.monotonic() + 150
    try:
        while time.monotonic() < deadline:
            readable, _, _ = select.select([master], [], [], 0.1)
            if readable:
                try:
                    block = os.read(master, 16384)
                except OSError:
                    block = b""
                if block:
                    output.extend(block)
                    if len(output) > 1024 * 1024:
                        raise RuntimeError("Console fixture exceeded its output allowance")
            if b"CONSOLE_READY" in output and not sent:
                sent = True
                if mode == "input":
                    os.write(master, b"list\nstop\n")
                elif mode == "interrupt":
                    os.write(master, b"\x03")
                else:
                    os.kill(int(runtime_pid.read_text()), signal.SIGSTOP)
                    os.kill(int(jar_pid.read_text()), signal.SIGTERM)
            pid, state = os.waitpid(host_pid, os.WNOHANG)
            if pid:
                finished = True
                if os.waitstatus_to_exitcode(state) != 0:
                    raise RuntimeError("Terminal fixture host failed")
                break
        if not finished:
            raise TimeoutError("JAR/native console did not retire within 150 seconds")
        outcome = json.loads(result.read_text())
        if not sent or not outcome["foreground_restored"] or not outcome["settings_restored"]:
            raise AssertionError("Foreground ownership or terminal settings were not restored")
        expected = {"input": 0, "interrupt": 1, "cancel-stopped": 130}[mode]
        if outcome["status"] != expected:
            raise AssertionError(f"Expected classified status {expected}, got {outcome['status']}")
        if mode == "input" and b"CONSOLE_INPUT=list" not in output:
            raise AssertionError("Console input did not reach the runtime")
        if b"recovery required" in output.lower() or b"retirement is unconfirmed" in output.lower():
            raise AssertionError("Retired fixture left an uncertain runtime outcome")
        return outcome
    finally:
        (home / (mode + ".log")).write_bytes(output)
        if not finished:
            # These PIDs are created by this isolated fixture, never discovered by
            # executable name. Retain failure logs and let native cleanup run first.
            if jar_pid.exists():
                try:
                    pid = int(jar_pid.read_text())
                    command = subprocess.run(["ps", "-p", str(pid), "-o", "command="], capture_output=True, text=True).stdout
                    if str(args.jar) in command:
                        os.kill(pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
            if runtime_pid.exists():
                try:
                    pid = int(runtime_pid.read_text())
                    command = subprocess.run(["ps", "-p", str(pid), "-o", "command="], capture_output=True, text=True).stdout
                    if "-Dempack.fixture.pid=" + str(runtime_pid) in command:
                        os.killpg(pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            try:
                os.killpg(host_pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            os.waitpid(host_pid, 0)
        os.close(master)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("engine", "instance", "state-dir", "java", "javac", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--development-engine", action="store_true")
    args = parser.parse_args()
    if os.name != "posix":
        parser.error("This controlling-PTY smoke requires Unix")
    for name in ("engine", "instance", "state_dir", "java", "javac", "output"):
        setattr(args, name, getattr(args, name).resolve())
    if not (args.instance / ".empack-bootstrap-fixture").is_file():
        parser.error("Refusing an instance without the explicit disposable-fixture marker")
    args.jar = args.instance / ".empack-tools/empack-bootstrap.jar"
    args.output.mkdir(parents=True, exist_ok=False)
    (args.output / "ConsoleFixture.java").write_text(CONSOLE, encoding="utf-8")
    subprocess.run([str(args.javac), "--release", "8", str(args.output / "ConsoleFixture.java")], check=True)
    outcomes = [run_case(args, args.output, mode) for mode in ("input", "interrupt", "cancel-stopped")]
    record = {"jar_sha256": hashlib.sha256(args.jar.read_bytes()).hexdigest(),
              "engine_sha256": hashlib.sha256(args.engine.read_bytes()).hexdigest(), "outcomes": outcomes,
              "scope": "real PTY and native instance ownership; synthetic Java console, no gameplay"}
    (args.output / "result.json").write_text(json.dumps(record, indent=2) + "\n")
    print(json.dumps(record, indent=2))


if __name__ == "__main__":
    main()
