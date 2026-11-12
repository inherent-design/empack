"""Execute the packaged JAR; fixture engines do not implement pack management."""
import argparse
import hashlib
import http.server
import io
import json
import os
from pathlib import Path
import signal
import shutil
import ssl
import struct
import subprocess
import sys
import tempfile
import threading
import time
import tarfile
import unittest
import warnings
import zipfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT.parent / "scripts"))
from verification.compiler import compiler_environment

JAR = ROOT / "target/empack-bootstrap.jar"
JAVA = str(Path(os.environ["JAVA_HOME"]) / "bin/java") if "JAVA_HOME" in os.environ else "java"
_fixture_home = None
_engine = None


def native_engine():
    global _fixture_home, _engine
    if _engine is None:
        _fixture_home = tempfile.TemporaryDirectory(prefix="empack bootstrap native fixture ")
        _engine = Path(_fixture_home.name) / ("engine.exe" if os.name == "nt" else "engine")
        subprocess.run(["rustc", "--edition=2024", str(ROOT / "tests/fixtures/engine.rs"),
                        "-o", str(_engine)], check=True, env=compiler_environment(os.environ))
    return _engine


def tearDownModule():
    if _fixture_home is not None:
        _fixture_home.cleanup()


class BootstrapTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if not JAR.is_file():
            raise RuntimeError("Build the bootstrap JAR explicitly before executing its tests")
        cls.engine = native_engine()

    def invoke(self, *args, **kwargs):
        return subprocess.run([JAVA, "-jar", str(JAR), *args], capture_output=True,
                              timeout=20, **kwargs)

    def test_packaged_help_and_version(self):
        help_result = self.invoke("--help")
        self.assertEqual(help_result.returncode, 0, help_result.stderr)
        self.assertIn(b"--engine-version", help_result.stdout)
        version = self.invoke("--version")
        self.assertEqual(version.returncode, 0, version.stderr)
        self.assertTrue(version.stdout.startswith(b"empack-bootstrap "))
        identity = JAR.with_suffix(".json")
        if identity.is_file():
            expected = json.loads(identity.read_text())["version"]
            self.assertEqual(version.stdout.decode().strip(), "empack-bootstrap " + expected)

    def test_packaged_base_classes_target_java8(self):
        with zipfile.ZipFile(JAR) as archive:
            classes = [n for n in archive.namelist()
                       if n.endswith(".class") and not n.startswith("META-INF/versions/")]
            self.assertTrue(classes)
            for name in classes:
                header = archive.read(name)[:8]
                self.assertEqual(header[:4], b"\xca\xfe\xba\xbe", name)
                self.assertLessEqual(struct.unpack(">H", header[6:8])[0], 52, name)

    def test_engine_argv_streams_unicode_and_status_are_preserved(self):
        args = ["--", "value with spaces", "日本語", "", "line\nbreak", "$(display only)"]
        data = b"console input\n\x00\xff"
        result = self.invoke("--engine-version", "0.8.5-beta", "--engine", str(self.engine),
                             "--", "--fixture-argv", *args, input=data)
        self.assertEqual(result.returncode, 7, result.stderr)
        self.assertEqual(result.stderr, b"fixture returned 7\n")
        count = struct.unpack(">I", result.stdout[:4])[0]
        self.assertEqual(count, len(args))
        offset, actual = 4, []
        for _ in range(count):
            length = struct.unpack(">I", result.stdout[offset:offset + 4])[0]
            offset += 4
            actual.append(result.stdout[offset:offset + length].decode("utf-8"))
            offset += length
        self.assertEqual(result.stdout[offset:], data)
        # Keep the literal-argv assertion below. This independent, nonshipping
        # probe distinguishes JVM ingress from the shipping Java/native handoff;
        # observing a file route must not turn a broken literal route into a pass.
        observations = [{"route": "literal", "argv": actual, "status": result.returncode}]
        with tempfile.TemporaryDirectory(prefix="empack argument ingress ") as temporary:
            invocation = Path(temporary) / "invocation space.json"
            fixture_arguments = [
                "--engine-version", "0.8.5-beta", "--engine", str(self.engine),
                "--", "--fixture-argv", *args]
            invocation.write_text(json.dumps(fixture_arguments, ensure_ascii=False), encoding="utf-8")
            classpath = os.pathsep.join([str(JAR), str(ROOT / "target/test-classes")])
            command = [JAVA, "-cp", classpath, "design.inherent.empack.bootstrap.FixtureMain"]
            cli = ["arguments-from-cli", *fixture_arguments]
            file = ["arguments-from-file", str(invocation)]
            variants = [("jvm-cli", cli, None), ("utf8-file", file, None)]
            if os.name != "nt":
                c_locale = dict(os.environ, LANG="C", LC_ALL="C")
                variants.extend([("jvm-cli-C-locale", cli, c_locale),
                                 ("utf8-file-C-locale", file, c_locale)])
            for route, arguments, environment in variants:
                probe = subprocess.run([*command, *arguments], env=environment, input=data,
                                       capture_output=True, timeout=20)
                observation = {"route": route, "status": probe.returncode}
                try:
                    decoded = next(line for line in probe.stderr.splitlines()
                                   if line.startswith(b"UTF8_ARGUMENTS="))
                    observation["jvm_arguments"] = json.loads(decoded.removeprefix(b"UTF8_ARGUMENTS="))
                    count = struct.unpack(">I", probe.stdout[:4])[0]
                    position, values = 4, []
                    if count != len(args):
                        raise ValueError("unexpected argument count")
                    for _ in range(count):
                        length = struct.unpack(">I", probe.stdout[position:position + 4])[0]
                        position += 4
                        values.append(probe.stdout[position:position + length].decode("utf-8"))
                        position += length
                    observation.update(argv=values, input_preserved=probe.stdout[position:] == data)
                except (ValueError, struct.error, UnicodeError, StopIteration):
                    observation["error"] = "probe did not return the fixture argument record"
                observations.append(observation)
        print(json.dumps({"scope": "argument-file feasibility; C-locale observations are supplemental",
                          "observations": observations}), flush=True)
        self.assertEqual(actual, args, "Literal JVM transport differs from the original Unicode arguments")
        self.assertEqual(observations[2].get("argv"), args)
        self.assertEqual(observations[2]["status"], 7)
        self.assertTrue(observations[2].get("input_preserved"))

    def test_newer_local_engine_does_not_replace_requested_version(self):
        result = self.invoke("--engine-version", "0.8.4-beta", "--engine", str(self.engine),
                             "--", "--fixture-argv")
        self.assertEqual(result.returncode, 1)
        self.assertIn(b"does not report requested version 0.8.4-beta", result.stderr)
        self.assertEqual(result.stdout, b"")

    def test_development_engine_requires_explicit_opt_in(self):
        environment = dict(os.environ, EMPACK_FIXTURE_VERSION="0.0.0-dev")
        args = ["--engine-version", "0.8.5-beta", "--engine", str(self.engine)]
        denied = self.invoke(*args, "--", "--fixture-argv", env=environment)
        self.assertEqual(denied.returncode, 1)
        accepted = self.invoke(*args, "--development-engine", "--", "--fixture-argv",
                               env=environment, input=b"")
        self.assertEqual(accepted.returncode, 7, accepted.stderr)

    def test_missing_cancellation_capability_is_refused_before_delegation(self):
        result = self.invoke("--engine-version", "0.8.5-beta", "--engine", str(self.engine),
                             "--", "--fixture-argv",
                             env=dict(os.environ, EMPACK_FIXTURE_PROTOCOL="missing"))
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, b"")
        self.assertIn(b"bootstrap cancellation protocol 1", result.stderr)
        self.assertIn(b"0.8.5-beta", result.stderr)

    def test_versions_and_duplicate_options_fail_before_engine_execution(self):
        for version in ["latest", ">=0.8.5", "v0.8.5-beta", "0.8.5-beta\n", "0.08.5",
                        "0.8.5-rc.01", "../0.8.5", "0.0.0-dev"]:
            with self.subTest(version=version):
                result = self.invoke("--engine-version", version, "--engine", str(self.engine),
                                     "--", "--fixture-argv")
                self.assertEqual(result.returncode, 2)
                self.assertEqual(result.stdout, b"")
        repeated = self.invoke("--engine-version", "0.8.5-beta", "--engine-version", "0.8.5-beta",
                               "--engine", str(self.engine), "--", "--fixture-argv")
        self.assertEqual(repeated.returncode, 2)

    @unittest.skipIf(os.name == "nt", "Native executable symlink fixture is Unix-specific")
    def test_explicit_engine_symlink_is_rejected(self):
        with tempfile.TemporaryDirectory() as folder:
            link = Path(folder) / "engine"
            link.symlink_to(self.engine)
            result = self.invoke("--engine-version", "0.8.5-beta", "--engine", str(link),
                                 "--", "--fixture-argv")
            self.assertEqual(result.returncode, 1)
            self.assertIn(b"executable regular file", result.stderr)

    def test_jvm_termination_cancels_and_waits_for_native_cleanup(self):
        # Windows kill uses TerminateProcess; it must still close the invocation
        # connection and reach native cancellation without a Java shutdown hook.
        for termination in ((None,) if os.name == "nt" else (signal.SIGTERM, signal.SIGKILL)):
            with self.subTest(termination=termination), tempfile.TemporaryDirectory() as folder:
                ready = Path(folder) / "ready"
                process = subprocess.Popen([
                    JAVA, "-jar", str(JAR), "--engine-version", "0.8.5-beta", "--engine",
                    str(self.engine), "--", "--fixture-wait", str(ready),
                ], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                child = None
                try:
                    deadline = time.monotonic() + 10
                    while not ready.exists() and process.poll() is None and time.monotonic() < deadline:
                        time.sleep(0.02)
                    self.assertTrue(ready.exists(), "Native engine did not complete the cancellation handshake")
                    child = int(ready.read_text())
                    if termination is None:
                        process.kill()
                    else:
                        process.send_signal(termination)
                    stdout, stderr = process.communicate(timeout=10)
                    self.assertEqual(stdout, b"")
                    self.assertIn(b"fixture retired; recovery evidence cleared", stderr)
                    if termination == signal.SIGTERM:
                        self.assertEqual(process.returncode, 130, stderr)
                        self.assertIn(b"waiting for cleanup", stderr)
                    # Reparented children can remain briefly visible as zombies. Verify
                    # the absence of a live owned process rather than mistaking reaping
                    # latency for a still-running engine.
                    if os.name != "nt":
                        state = subprocess.run(["ps", "-o", "stat=", "-p", str(child)],
                                               capture_output=True, text=True, check=False).stdout.strip()
                        self.assertTrue(not state or state.startswith("Z"), state)
                finally:
                    if process.poll() is None:
                        process.kill()
                        process.communicate(timeout=10)

    @unittest.skipIf(os.name == "nt", "Unix JVM shutdown signal fixture")
    def test_shutdown_during_native_startup_still_delivers_cancellation(self):
        with tempfile.TemporaryDirectory() as folder:
            ready = Path(folder) / "starting"
            process = subprocess.Popen([
                JAVA, "-jar", str(JAR), "--engine-version", "0.8.5-beta", "--engine", str(self.engine),
                "--", "--fixture-late-wait", str(ready),
            ], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            try:
                deadline = time.monotonic() + 10
                while not ready.exists() and process.poll() is None and time.monotonic() < deadline:
                    time.sleep(0.01)
                self.assertTrue(ready.exists())
                process.terminate()
                stdout, stderr = process.communicate(timeout=10)
                self.assertEqual(process.returncode, 130, stderr)
                self.assertEqual(stdout, b"")
                self.assertIn(b"fixture retired; recovery evidence cleared", stderr)
            finally:
                if process.poll() is None:
                    process.kill()
                    process.communicate(timeout=10)


class ArchiveTests(unittest.TestCase):
    """Container/target fixtures are data, not runnable or gameplay-qualified engines."""
    @staticmethod
    def header(target):
        if "windows" in target:
            result = bytearray(134)
            result[:2] = b"MZ"
            struct.pack_into("<I", result, 60, 128)
            result[128:134] = b"PE\0\0d\x86"
        else:
            result = bytearray(64)
            struct.pack_into("<II", result, 0, 0xfeedfacf, 0x0100000c)
        return bytes(result)

    def unpack(self, archive, folder, target):
        classpath = os.pathsep.join([str(JAR), str(ROOT / "target/test-classes")])
        return subprocess.run([JAVA, "-cp", classpath, "design.inherent.empack.bootstrap.FixtureMain",
                               "unpack", target, str(archive), str(folder)],
                              capture_output=True, timeout=20)

    @staticmethod
    def tar(entries):
        output = io.BytesIO()
        with tarfile.open(fileobj=output, mode="w:gz") as archive:
            for name, content, kind in entries:
                entry = tarfile.TarInfo(name)
                entry.type = kind
                entry.size = len(content) if kind == tarfile.REGTYPE else 0
                if kind == tarfile.SYMTYPE:
                    entry.linkname = "../outside"
                archive.addfile(entry, io.BytesIO(content) if kind == tarfile.REGTYPE else None)
        return output.getvalue()

    def test_tar_integrity_paths_and_entry_kinds(self):
        target = "aarch64-apple-darwin"
        payload = self.header(target)
        good = self.tar([(".", b"", tarfile.DIRTYPE), ("./empack", payload, tarfile.REGTYPE)])
        corrupt_crc = bytearray(good)
        corrupt_crc[-8] ^= 1
        samples = {
            "good": good,
            "truncated": good[:-1],
            "gzip-crc": bytes(corrupt_crc),
            "traversal": self.tar([("../outside", payload, tarfile.REGTYPE)]),
            "absolute": self.tar([("/empack", payload, tarfile.REGTYPE)]),
            "symlink": self.tar([("empack", b"", tarfile.SYMTYPE)]),
            "duplicate": self.tar([("empack", payload, tarfile.REGTYPE)] * 2),
            "wrong-target": self.tar([("empack", b"not an executable" * 8, tarfile.REGTYPE)]),
        }
        for label, archive_bytes in samples.items():
            with self.subTest(label=label), tempfile.TemporaryDirectory() as temporary:
                base = Path(temporary)
                archive, output = base / "archive.tar.gz", base / "output"
                archive.write_bytes(archive_bytes)
                output.mkdir()
                result = self.unpack(archive, output, target)
                if label == "good":
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stdout.strip(), hashlib.sha256(payload).hexdigest().encode())
                else:
                    self.assertNotEqual(result.returncode, 0, label)
                self.assertFalse((base / "outside").exists())


    def test_zip_integrity_paths_and_duplicate_destinations(self):
        target = "x86_64-pc-windows-msvc"
        payload = self.header(target)
        def encode(names):
            output = io.BytesIO()
            with warnings.catch_warnings():
                warnings.simplefilter("ignore", UserWarning)
                with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_STORED) as archive:
                    for name in names:
                        archive.writestr(name, payload)
            return output.getvalue()
        good = encode(["empack.exe"])
        corrupt = bytearray(good)
        with zipfile.ZipFile(io.BytesIO(good)) as archive:
            entry = archive.getinfo("empack.exe")
            offset = entry.header_offset + 30 + len(entry.filename.encode()) + len(entry.extra)
        corrupt[offset + len(payload) - 1] ^= 1
        samples = {"good": good, "crc": bytes(corrupt), "truncated": good[:-1],
                   "duplicate": encode(["empack.exe", "empack.exe"]),
                   "traversal": encode(["../outside"])}
        for label, archive_bytes in samples.items():
            with self.subTest(label=label), tempfile.TemporaryDirectory() as temporary:
                base = Path(temporary)
                archive, output = base / "archive.zip", base / "output"
                archive.write_bytes(archive_bytes)
                output.mkdir()
                result = self.unpack(archive, output, target)
                self.assertEqual(result.returncode == 0, label == "good", result.stderr)
                if label == "good":
                    self.assertEqual(result.stdout.strip(), hashlib.sha256(payload).hexdigest().encode())
                self.assertFalse((base / "outside").exists())


class CacheTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.engine = native_engine()
        cls.certificates = tempfile.TemporaryDirectory(prefix="empack-bootstrap-tls-")
        folder = Path(cls.certificates.name)
        openssl = shutil.which("openssl")
        keytool = Path(JAVA).with_name("keytool.exe" if os.name == "nt" else "keytool")
        if not keytool.is_file():
            keytool = shutil.which("keytool")
        if not openssl or not keytool:
            raise RuntimeError("Bootstrap HTTPS fixtures require OpenSSL and Java keytool")
        cls.certificate, cls.key, cls.trust = folder / "certificate.pem", folder / "key.pem", folder / "trust.jks"
        subprocess.run([openssl, "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "2",
                        "-subj", "/CN=localhost", "-addext", "subjectAltName=DNS:localhost",
                        "-keyout", str(cls.key), "-out", str(cls.certificate)], check=True, capture_output=True)
        subprocess.run([str(keytool), "-importcert", "-noprompt", "-alias", "fixture", "-file",
                        str(cls.certificate), "-keystore", str(cls.trust), "-storetype", "JKS",
                        "-storepass", "fixture-password"], check=True, capture_output=True)

    @classmethod
    def tearDownClass(cls):
        cls.certificates.cleanup()

    def setUp(self):
        self.home = tempfile.TemporaryDirectory(prefix="empack-bootstrap-cache-")
        self.root = Path(self.home.name).resolve()
        self.cache = self.root / "cache"
        self.mode = "good"
        self.asset_requests = 0
        self.blocked = threading.Event()
        self.resume = threading.Event()
        import platform
        cpu = "aarch64" if platform.machine().lower() in ("aarch64", "arm64") else "x86_64"
        if os.name == "nt":
            self.target, self.extension, self.executable = "x86_64-pc-windows-msvc", "zip", "empack.exe"
        elif platform.system() == "Darwin":
            self.target, self.extension, self.executable = cpu + "-apple-darwin", "tar.gz", "empack"
        else:
            self.target, self.extension, self.executable = cpu + "-unknown-linux-gnu", "tar.gz", "empack"
        buffer = io.BytesIO()
        if self.extension == "zip":
            with zipfile.ZipFile(buffer, "w", compression=zipfile.ZIP_DEFLATED) as archive:
                archive.write(self.engine, self.executable)
        else:
            with tarfile.open(fileobj=buffer, mode="w:gz") as archive:
                archive.add(self.engine, arcname="./" + self.executable, recursive=False)
        self.payload = buffer.getvalue()
        fixture = self
        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def do_GET(self):
                try:
                    if self.path == "/delayed-headers":
                        fixture.blocked.set()
                        fixture.resume.wait(timeout=12)
                        self.send_response(200)
                        self.end_headers()
                    elif self.path == "/trickling-body":
                        self.send_response(200)
                        self.end_headers()
                        fixture.blocked.set()
                        for _ in range(30):
                            self.wfile.write(b".")
                            self.wfile.flush()
                            if fixture.resume.wait(timeout=0.2):
                                break
                    elif self.path.startswith("/api/releases/tags/v"):
                        version = self.path.removeprefix("/api/releases/tags/v")
                        asset = {"name": f"empack-{version}-{fixture.target}.{fixture.extension}",
                                 "size": len(fixture.payload),
                                 "digest": "sha256:" + hashlib.sha256(fixture.payload).hexdigest()}
                        if fixture.mode == "wrong-hash":
                            asset["digest"] = "sha256:" + "0" * 64
                        if fixture.mode == "missing-digest":
                            asset["digest"] = None
                        assets = None if fixture.mode == "invalid-assets" else [] if fixture.mode == "missing-asset" else [asset]
                        body = json.dumps({"tag_name": "v" + version, "draft": False, "assets": assets}).encode()
                        self.send_response(200)
                        self.send_header("Content-Length", str(len(body)))
                        self.end_headers()
                        self.wfile.write(body)
                    elif self.path.startswith("/assets/v"):
                        fixture.asset_requests += 1
                        if fixture.mode == "redirect-http":
                            self.send_response(302)
                            self.send_header("Location", f"http://localhost:{fixture.server.server_port}/unsafe")
                            self.end_headers()
                            return
                        self.send_response(200)
                        self.send_header("Content-Length", str(len(fixture.payload)))
                        self.end_headers()
                        if fixture.mode == "block-first" and fixture.asset_requests == 1:
                            self.wfile.write(fixture.payload[:16384])
                            self.wfile.flush()
                            fixture.blocked.set()
                            fixture.resume.wait(timeout=20)
                            self.wfile.write(fixture.payload[16384:])
                        elif fixture.mode == "truncated":
                            self.wfile.write(fixture.payload[:len(fixture.payload) // 2])
                        else:
                            self.wfile.write(fixture.payload)
                    else:
                        self.send_error(404)
                except (BrokenPipeError, ConnectionResetError, ssl.SSLError):
                    pass  # Expected when an interrupted JVM closes its own fixture connection.

        self.server = http.server.ThreadingHTTPServer(("localhost", 0), Handler)
        self.server.daemon_threads = True
        tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        tls.load_cert_chain(self.certificate, self.key)
        self.server.socket = tls.wrap_socket(self.server.socket, server_side=True)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.origin = f"https://localhost:{self.server.server_port}/"

    def tearDown(self):
        self.resume.set()
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=5)
        self.home.cleanup()

    def command(self, version="0.8.5-beta", *, offline=False, trusted=True):
        properties = [f"-Djavax.net.ssl.trustStore={self.trust}",
                      "-Djavax.net.ssl.trustStorePassword=fixture-password"] if trusted else []
        classpath = os.pathsep.join([str(JAR), str(ROOT / "target/test-classes")])
        return [JAVA, *properties, "-cp", classpath, "design.inherent.empack.bootstrap.FixtureMain",
                "resolve", self.origin, str(self.cache), version, str(offline).lower()]

    def invoke(self, version="0.8.5-beta", **kwargs):
        environment = dict(os.environ, EMPACK_FIXTURE_VERSION="0.8.6-beta" if self.mode == "wrong-version" else version)
        if self.mode == "missing-protocol":
            environment["EMPACK_FIXTURE_PROTOCOL"] = "missing"
        return subprocess.run(self.command(version, **kwargs), env=environment, capture_output=True, timeout=40)

    def test_verified_first_use_and_offline_reuse(self):
        first = self.invoke()
        self.assertEqual(first.returncode, 0, first.stderr)
        engine = Path(first.stdout.decode().strip())
        self.assertEqual(engine.read_bytes(), self.engine.read_bytes())
        self.assertEqual(self.asset_requests, 1)
        cached = self.invoke(offline=True)
        self.assertEqual(cached.returncode, 0, cached.stderr)
        self.assertEqual(first.stdout, cached.stdout)
        self.assertEqual(self.asset_requests, 1)

    def test_deadline_and_cancellation_bound_headers_and_trickling_bodies(self):
        classpath = os.pathsep.join([str(JAR), str(ROOT / "target/test-classes")])
        for path, mode, expected in (("delayed-headers", "deadline", b"deadline exceeded"),
                                     ("trickling-body", "deadline", b"deadline exceeded"),
                                     ("delayed-headers", "interrupt", b"interrupted")):
            with self.subTest(path=path, mode=mode):
                start = time.monotonic()
                result = subprocess.run([JAVA, f"-Djavax.net.ssl.trustStore={self.trust}",
                                         "-Djavax.net.ssl.trustStorePassword=fixture-password",
                                         "-cp", classpath, "design.inherent.empack.bootstrap.FixtureMain",
                                         "fetch", self.origin + path, "1800", mode],
                                        capture_output=True, timeout=5)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn(expected, result.stdout)
                self.assertLess(time.monotonic() - start, 4)

    def test_untrusted_tls_is_not_bypassed(self):
        result = self.invoke(trusted=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.asset_requests, 0)
        self.assertFalse(list(self.cache.rglob("ready.properties")))

    def test_failed_acquisition_never_publishes_an_engine(self):
        for mode in ("wrong-hash", "truncated", "redirect-http", "missing-digest", "invalid-assets",
                     "missing-asset", "wrong-version", "missing-protocol"):
            with self.subTest(mode=mode):
                self.mode = mode
                result = self.invoke()
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(list(self.cache.rglob("ready.properties")))
                self.assertFalse(list(self.cache.rglob("private-*")))

    def test_concurrent_first_use_downloads_one_immutable_object(self):
        environment = dict(os.environ, EMPACK_FIXTURE_VERSION="0.8.5-beta")
        processes = [subprocess.Popen(self.command(), env=environment, stdout=subprocess.PIPE,
                                      stderr=subprocess.PIPE) for _ in range(2)]
        try:
            outcomes = [(p, *p.communicate(timeout=40)) for p in processes]
            for process, stdout, stderr in outcomes:
                self.assertEqual(process.returncode, 0, stderr)
            self.assertEqual(outcomes[0][1], outcomes[1][1])
            self.assertEqual(self.asset_requests, 1)
        finally:
            for process in processes:
                if process.poll() is None:
                    process.kill()
                    process.communicate(timeout=10)

    def test_interrupted_download_recovers_without_trusting_partial_bytes(self):
        self.mode = "block-first"
        process = subprocess.Popen(self.command(), stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            self.assertTrue(self.blocked.wait(timeout=15), "Download did not enter the interrupted fixture")
            process.kill()
            process.communicate(timeout=10)
            self.resume.set()
            self.mode = "good"
            result = self.invoke()
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(self.asset_requests, 2)
            self.assertFalse(list(self.cache.rglob("private-*")))
            self.assertFalse(list(self.cache.rglob("pending.properties")))
        finally:
            self.resume.set()
            if process.poll() is None:
                process.kill()
                process.communicate(timeout=10)

    def test_corrupt_registered_engine_is_refused_without_overwrite(self):
        first = self.invoke()
        self.assertEqual(first.returncode, 0, first.stderr)
        engine = Path(first.stdout.decode().strip())
        engine.chmod(0o700)
        engine.write_bytes(b"corrupt")
        result = self.invoke(offline=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b"corrupt", result.stderr)
        self.assertEqual(engine.read_bytes(), b"corrupt")
        self.assertEqual(self.asset_requests, 1)

    def test_case_distinct_prereleases_have_separate_cache_slots(self):
        first = self.invoke("0.8.5-beta.A")
        second = self.invoke("0.8.5-beta.a")
        self.assertEqual(first.returncode, 0, first.stderr)
        self.assertEqual(second.returncode, 0, second.stderr)
        self.assertNotEqual(first.stdout, second.stdout)

    def test_restart_after_object_commit_recovers_without_redownload(self):
        first = self.invoke()
        self.assertEqual(first.returncode, 0, first.stderr)
        engine = Path(first.stdout.decode().strip())
        slot = next(self.cache.rglob("ready.properties")).parent
        (slot / "ready.properties").unlink()
        identifier = "12345678-1234-1234-1234-123456789abc"
        (slot / "pending.properties").write_text(
            "kind=empack-engine-download/1\ncandidate=private-" + identifier + "\n")
        (slot / ("metadata-" + identifier + ".tmp")).write_bytes(b"unfinished metadata")
        result = self.invoke()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(Path(result.stdout.decode().strip()), engine)
        self.assertEqual(self.asset_requests, 1)
        self.assertTrue((slot / "ready.properties").is_file())
        self.assertFalse((slot / "pending.properties").exists())
        self.assertFalse(list(slot.glob("metadata-*.tmp")))

    def test_missing_backing_is_not_silently_replaced(self):
        first = self.invoke()
        self.assertEqual(first.returncode, 0, first.stderr)
        engine = Path(first.stdout.decode().strip())
        shutil.rmtree(engine.parent)
        result = self.invoke()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b"backing is missing", result.stderr)
        self.assertEqual(self.asset_requests, 1)

    def test_cache_redirect_cannot_authorize_engine_execution(self):
        outside = self.root / "outside"
        outside.mkdir()
        marker = outside / "keep"
        marker.write_bytes(b"not cache data")
        if os.name == "nt":
            subprocess.run(["cmd", "/d", "/c", "mklink", "/J", str(self.cache), str(outside)],
                           check=True, capture_output=True)
        else:
            self.cache.symlink_to(outside, target_is_directory=True)
        result = self.invoke()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b"must not traverse links or filesystem aliases", result.stderr)
        self.assertEqual(self.asset_requests, 0)
        self.assertEqual(marker.read_bytes(), b"not cache data")

    def test_shared_writable_cache_is_refused(self):
        self.cache.mkdir()
        if os.name == "nt":
            # Use the well-known Everyone SID, not an English account name.
            # Only this disposable directory is changed; no elevation is needed.
            subprocess.run(["icacls", str(self.cache), "/grant", "*S-1-1-0:(OI)(CI)M"],
                           check=True, capture_output=True, timeout=10)
            expected = b"ACL admits a writer outside the user-home trust boundary"
        else:
            self.cache.chmod(0o777)
            expected = b"permits writes by another user"
        result = self.invoke()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(expected, result.stderr)
        self.assertEqual(self.asset_requests, 0)



if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--jar", type=Path, default=JAR)
    parser.add_argument("--java", default=JAVA)
    args, extra = parser.parse_known_args()
    JAR, JAVA = args.jar.resolve(), args.java
    unittest.main(argv=[__file__, *extra])
