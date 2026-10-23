"""Parse rustdoc's explicit test listing, never infer tests from Rust source."""
import re


def rustdoc_tests(output, *, allow_empty=False):
    discovered = []
    group = []
    summaries = 0
    for line in output.splitlines():
        if not line.strip():
            continue
        if line.endswith(": test"):
            identity = line.removesuffix(": test")
            if not re.fullmatch(r".+ - .*\(line [1-9][0-9]*\)", identity):
                raise ValueError(f"Unknown rustdoc test identity: {line}")
            group.append(identity)
        elif match := re.fullmatch(r"([0-9]+) tests?, ([0-9]+) benchmarks?", line):
            if int(match[1]) != len(group) or int(match[2]):
                raise ValueError("Rustdoc listing count mismatch or unexpected benchmarks")
            discovered.extend(group)
            group = []
            summaries += 1
        else:
            raise ValueError(f"Unknown rustdoc listing line: {line}")
    if group or not summaries:
        raise ValueError("Incomplete rustdoc listing")
    if len(set(discovered)) != len(discovered):
        raise ValueError("Duplicate rustdoc test identity")
    if not discovered and not allow_empty:
        raise ValueError("Rustdoc discovery must be nonempty")
    return set(discovered)
