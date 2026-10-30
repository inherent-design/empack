"""Select configured native compiler tools without changing the host environment."""
import os


def compiler_environment(environment):
    """Git Bash's link.exe is not the linker for an initialized x64 MSVC toolset.

    Call only for compilation/discovery commands. Explicit compiler flags and
    caller-owned homes are unchanged; runtime-only commands keep their own policy.
    """
    result = dict(environment)
    if os.name != "nt":
        return result
    installation = next((value for name, value in result.items()
                         if name.upper() == "VCTOOLSINSTALLDIR"), None)
    if not installation:
        return result
    directory = os.path.join(installation, "bin", "Hostx64", "x64")
    if not all(os.path.isfile(os.path.join(directory, tool)) for tool in ("cl.exe", "link.exe")):
        raise RuntimeError("Configured x64 MSVC compiler tools are missing")
    path = next((value for name, value in result.items() if name.upper() == "PATH"), "")
    if path != directory and not path.startswith(directory + os.pathsep):
        path = directory + (os.pathsep + path if path else "")
    # A plain Python dict can contain differently cased spellings, unlike the
    # native Windows environment. Emit one spelling for the child process.
    result = {name: value for name, value in result.items() if name.upper() != "PATH"}
    result["PATH"] = path
    return result
