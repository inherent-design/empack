"""Literal ABI headers and fabricated inspector output; never runnable releases."""
from datetime import datetime, timezone
import hashlib
import io
import struct
import tarfile
import zipfile

from verification.evidence import write_json


def archive(path, target, payload=None):
    """Real containers holding inert ABI headers, never executable fixtures."""
    payload = header(target) if payload is None else payload
    if target.endswith("windows-msvc"):
        with zipfile.ZipFile(path, "w") as package:
            package.writestr("empack.exe", payload)
    else:
        with tarfile.open(path, "w:gz") as package:
            entry = tarfile.TarInfo("empack")
            entry.mode, entry.size = 0o755, len(payload)
            package.addfile(entry, io.BytesIO(payload))
    return hashlib.sha256(payload).hexdigest(), len(payload)


def header(target):
    if target.endswith("linux-gnu"):
        machine = 62 if target.startswith("x86_64") else 183
        return b"\x7fELF\x02\x01\x01" + b"\0" * 9 + struct.pack(
            "<HHIQQQIHHHHHH", 3, machine, 1, 0, 64, 0, 0, 64, 56, 0, 64, 0, 0)
    if target.endswith("apple-darwin"):
        cpu = 0x01000007 if target.startswith("x86_64") else 0x0100000C
        return struct.pack("<8I", 0xFEEDFACF, cpu, 0, 2, 0, 0, 0, 0)
    raw = bytearray(512)
    raw[:2] = b"MZ"
    struct.pack_into("<I", raw, 0x3C, 128)
    raw[128:132] = b"PE\0\0"
    struct.pack_into("<HHIIIHH", raw, 132, 0x8664, 1, 0, 0, 0, 240, 0x22)
    struct.pack_into("<H", raw, 152, 0x20B)
    return bytes(raw)


def facts(target):
    format, machine, cpu, kind = {
        "x86_64-unknown-linux-gnu": ("elf", 62, "x86_64", 3),
        "aarch64-unknown-linux-gnu": ("elf", 183, "aarch64", 3),
        "x86_64-apple-darwin": ("mach-o", 0x01000007, "x86_64", 2),
        "aarch64-apple-darwin": ("mach-o", 0x0100000C, "aarch64", 2),
        "x86_64-pc-windows-msvc": ("pe", 0x8664, "x86_64", 0x22),
    }[target]
    return dict(format=format, architecture=cpu, machine=machine, bits=64,
                byte_order="little", image_type=kind)


def reports(target):
    if target.endswith("linux-gnu"):
        return {"dependencies.log": "Dynamic section: (NEEDED) [libc.so.6]\n",
                "version.log": "GNU readelf (controlled fixture) 1\n"}
    if target.endswith("apple-darwin"):
        return {"dependencies.log": "empack:\n\t/usr/lib/libSystem.B.dylib\n",
                "version.log": "otool(1): controlled fixture\n"}
    return {"dependencies.log": "Microsoft (R) COFF/PE Dumper Version controlled-fixture\n"
                                "File Type: EXECUTABLE IMAGE\n KERNEL32.dll\n"}


def observation(directory, target, digest, length):
    """Do not call the production parser or producer to construct expected facts."""
    image = facts(target)
    windows = image["format"] == "pe"
    tool = {"elf": "/usr/bin/readelf", "mach-o": "/usr/bin/otool", "pe": r"C:\MSVC\dumpbin.exe"}[image["format"]]
    binary = r"C:\captured\empack.exe" if windows else "/captured/empack"
    arguments = {"elf": [tool, "--dynamic", "--program-headers", "--version-info", "--wide", binary],
                 "mach-o": [tool, "-L", "-l", binary], "pe": [tool, "/HEADERS", "/DEPENDENTS", binary]}[image["format"]]
    commands = dict(dependencies=arguments)
    if not windows:
        commands["version"] = [tool, "--version"]
    directory.mkdir()
    contents = reports(target)
    for name, text in contents.items():
        (directory / name).write_bytes(text.encode("utf-8"))
    version = contents["dependencies.log" if windows else "version.log"].splitlines()[0]
    record = dict(schema=1, kind="native-image-observation", qualification=False, target=target,
                  created_at=datetime.now(timezone.utc).isoformat(), image=image,
                  executable=dict(path=binary, sha256=digest, length=length),
                  tool=dict(path=tool, sha256="f" * 64, version=version), commands=commands,
                  host=dict(system={"elf": "Linux", "mach-o": "Darwin", "pe": "Windows"}[image["format"]],
                            architecture="controlled fixture", release="controlled fixture"),
                  reports={name: hashlib.sha256(text.encode("utf-8")).hexdigest() for name, text in contents.items()})
    write_json(directory / "image.json", record)
    return record
