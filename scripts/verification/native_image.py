"""Architecture and native linker reports for captured shipping executables.

This reads headers, not a general executable validator. The package CLI suite
separately establishes startup and behavior on the recorded native host.
"""
from datetime import datetime, timezone
import json
from pathlib import Path, PurePosixPath, PureWindowsPath
import platform
import re
import shutil
import struct

from .compiler import compiler_environment
from .evidence import sha256, write_json
from .process import run

# ELF gABI e_machine; Apple mach-o/loader.h and mach/machine.h; Microsoft PE format.
IMAGES = {
    "x86_64-unknown-linux-gnu": ("elf", 62, "x86_64", "Linux"),
    "aarch64-unknown-linux-gnu": ("elf", 183, "aarch64", "Linux"),
    "x86_64-apple-darwin": ("mach-o", 0x01000007, "x86_64", "Darwin"),
    "aarch64-apple-darwin": ("mach-o", 0x0100000C, "aarch64", "Darwin"),
    "x86_64-pc-windows-msvc": ("pe", 0x8664, "x86_64", "Windows"),
}
HEADER_LIMIT = 1 << 20
REPORT_LIMIT = 4 << 20


class InspectionUnavailable(RuntimeError):
    """A missing native inspection prerequisite cannot supply package evidence."""


def architecture(header, target):
    """Admit only the five thin, little-endian 64-bit release image profiles."""
    try:
        format, expected, name, _ = IMAGES[target]
        if format == "elf":
            if (len(header) < 64 or header[:7] != b"\x7fELF\x02\x01\x01"
                    or struct.unpack_from("<H", header, 52)[0] != 64):
                raise ValueError("Expected a complete ELF64 little-endian header")
            kind, machine, version = struct.unpack_from("<HHI", header, 16)
            if kind not in (2, 3) or version != 1:
                raise ValueError("Expected an executable ELF image")
        elif format == "mach-o":
            if len(header) < 32 or header[:4] != b"\xcf\xfa\xed\xfe":
                raise ValueError("Expected a thin Mach-O64 little-endian header")
            machine, _, kind = struct.unpack_from("<III", header, 4)
            if kind != 2:
                raise ValueError("Expected an executable Mach-O image")
        else:
            if len(header) < 64 or header[:2] != b"MZ":
                raise ValueError("Expected a PE image with a DOS header")
            offset = struct.unpack_from("<I", header, 0x3C)[0]
            if not 64 <= offset <= min(len(header), HEADER_LIMIT) - 26 or header[offset:offset + 4] != b"PE\0\0":
                raise ValueError("PE header offset or signature is invalid")
            machine, sections, _, _, _, size, kind = struct.unpack_from("<HHIIIHH", header, offset + 4)
            if (not 1 <= sections <= 96 or size < 112 or offset + 24 + size > len(header)
                    or struct.unpack_from("<H", header, offset + 24)[0] != 0x20B
                    or not kind & 2 or kind & 0x2000):
                raise ValueError("Expected a bounded PE32+ executable image")
        if machine != expected:
            raise ValueError("Executable architecture differs from the selected release target")
        return dict(format=format, architecture=name, machine=machine, bits=64,
                    byte_order="little", image_type=kind)
    except (KeyError, struct.error) as error:
        raise ValueError("Unknown target or truncated native executable header") from error


def inspect_architecture(binary, target):
    with Path(binary).open("rb") as stream:
        return architecture(stream.read(HEADER_LIMIT), target)


def report_arguments(target, tool, binary):
    format = IMAGES[target][0]
    if format == "elf":
        return [tool, "--dynamic", "--program-headers", "--version-info", "--wide", binary]
    if format == "mach-o":
        return [tool, "-L", "-l", binary]
    return [tool, "/HEADERS", "/DEPENDENTS", binary]


def _read(path, limit):
    if path.is_symlink() or path.parent.is_symlink() or not path.is_file():
        raise ValueError("Missing or redirected native image evidence")
    with path.open("rb") as stream:
        data = stream.read(limit + 1)
    if not data or len(data) > limit or b"[output truncated at admitted log limit]" in data:
        raise ValueError("Native image evidence is empty, oversized or truncated")
    return data.decode("utf-8")


def _version(text, windows):
    if windows:
        match = re.search(r"^Microsoft .*COFF/PE Dumper Version .+$", text, re.MULTILINE)
        if not match:
            raise ValueError("DUMPBIN did not identify its native tool version")
        return match[0].strip()
    lines = [line.strip() for line in text.splitlines() if line.strip()]
    if not lines:
        raise ValueError("Native inspector did not report its version")
    return lines[0]


def capture(binary, target, digest, directory, environment):
    binary, directory = Path(binary), Path(directory)
    if not binary.is_absolute() or binary.is_symlink() or sha256(binary) != digest:
        raise ValueError("Native inspection requires the exact captured executable")
    image = inspect_architecture(binary, target)
    if platform.system() != IMAGES[target][3]:
        raise InspectionUnavailable("Native linker inspection requires the selected target's operating system")
    environment = dict(compiler_environment(environment), LC_ALL="C", LANG="C", VSLANG="1033")
    name = {"elf": "readelf", "mach-o": "otool", "pe": "dumpbin.exe"}[image["format"]]
    found = shutil.which(name, path=environment.get("PATH"))
    if not found:
        raise InspectionUnavailable(f"Native package inspection requires installed {name}")
    tool = Path(found).absolute()
    tool_digest = sha256(tool)
    directory.mkdir()
    command = report_arguments(target, str(tool), str(binary))
    commands = dict(dependencies=command)
    if image["format"] != "pe":
        commands["version"] = [str(tool), "--version"]
    for label, arguments in commands.items():
        code, _, expired = run(arguments, cwd=directory, env=environment,
                               log=directory / f"{label}.log", timeout=30)
        if code or expired:
            raise RuntimeError(f"Native {label} inspection failed (exit {code}, timeout={expired}); see {label}.log")
        _read(directory / f"{label}.log", REPORT_LIMIT)
    windows = image["format"] == "pe"
    version = _version(_read(directory / ("dependencies.log" if windows else "version.log"), REPORT_LIMIT), windows)
    if sha256(binary) != digest or sha256(tool) != tool_digest:
        raise ValueError("Native executable or inspector changed during observation")
    record = dict(schema=1, kind="native-image-observation", qualification=False, target=target,
                  created_at=datetime.now(timezone.utc).isoformat(), image=image,
                  executable=dict(path=str(binary), sha256=digest, length=binary.stat().st_size),
                  tool=dict(path=str(tool), sha256=tool_digest, version=version), commands=commands,
                  host=dict(system=platform.system(), architecture=platform.machine(), release=platform.release()),
                  reports={f"{label}.log": sha256(directory / f"{label}.log") for label in commands})
    write_json(directory / "image.json", record)
    inspect(directory / "image.json", target, record["executable"], image)
    return sha256(directory / "image.json")


def inspect(path, target, executable, image):
    """Recheck reported bytes; image facts are supplied by the checked archive reader."""
    path = Path(path)
    record = json.loads(_read(path, 1 << 20))
    try:
        windows = IMAGES[target][0] == "pe"
        paths = PureWindowsPath if windows else PurePosixPath
        tool, binary = record["tool"], record["executable"]
        name = {"elf": "readelf", "mach-o": "otool", "pe": "dumpbin.exe"}[image["format"]]
        if (record["schema"] != 1 or record["kind"] != "native-image-observation" or record["qualification"] is not False
                or record["target"] != target or record["image"] != image
                or record["host"]["system"] != IMAGES[target][3] or not record["host"]["architecture"]
                or not record["host"]["release"] or not datetime.fromisoformat(record["created_at"]).tzinfo
                or not paths(tool["path"]).is_absolute() or paths(tool["path"]).name.lower() != name
                or not paths(binary["path"]).is_absolute() or paths(binary["path"]).name != ("empack.exe" if windows else "empack")
                or {key: binary[key] for key in ("sha256", "length")} != {key: executable[key] for key in ("sha256", "length")}
                or not re.fullmatch(r"[a-f0-9]{64}", tool["sha256"])):
            raise ValueError("Native image observation differs from the checked package")
        commands = dict(dependencies=report_arguments(target, tool["path"], binary["path"]))
        if not windows:
            commands["version"] = [tool["path"], "--version"]
        if (record["commands"] != commands or set(record["reports"]) != {f"{label}.log" for label in commands}
                or {p.name for p in path.parent.iterdir()} != {path.name, *record["reports"]}):
            raise ValueError("Native inspector command or report membership changed")
        for name, digest in record["reports"].items():
            _read(path.parent / name, REPORT_LIMIT)
            if sha256(path.parent / name) != digest:
                raise ValueError("Native dependency report changed")
        text = _read(path.parent / ("dependencies.log" if windows else "version.log"), REPORT_LIMIT)
        if tool["version"] != _version(text, windows):
            raise ValueError("Native tool version differs from its report")
        return dict(target=target, executable_sha256=binary["sha256"], image=image,
                    tool=tool, reports=record["reports"], host=record["host"],
                    observation_sha256=sha256(path), created_at=record["created_at"])
    except (KeyError, TypeError, AttributeError, StopIteration) as error:
        raise ValueError("Malformed native image observation") from error
