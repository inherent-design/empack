#!/usr/bin/env python3
"""Check selected Markdown files; optional snapshot checks require explicit Git refs."""
import argparse
from pathlib import Path, PurePosixPath
import posixpath
import re
import subprocess
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args], text=True).strip()


def prose(text, name):
    lines = []
    fence = None
    for line in text.splitlines():
        mark = re.match(r"^ {0,3}(`{3,}|~{3,})(.*)$", line)
        if mark:
            token, rest = mark.groups()
            if fence is None:
                fence = token
            elif token[0] == fence[0] and len(token) >= len(fence) and not rest.strip():
                fence = None
            continue
        if fence is None:
            lines.append(line)
    if fence:
        raise ValueError(f"{name}: unclosed fenced block")
    return "\n".join(lines)


def anchors(text):
    found, counts = set(), {}
    for title in re.findall(r"^ {0,3}#{1,6}\s+(.+?)\s*#*\s*$", text, re.MULTILINE):
        slug = re.sub(r"[^\w\- ]", "", title.lower()).replace(" ", "-")
        count = counts.get(slug, 0)
        counts[slug] = count + 1
        found.add(slug + (f"-{count}" if count else ""))
    return found


def validate(paths, available, read, strict_prose=False):
    for name in paths:
        body = prose(read(name), name)
        if strict_prose and re.search("[\u2013\u2014]", body):
            raise ValueError(f"{name}: replace prose en/em dashes")
        for target in re.findall(r"\[[^\]\n]*\]\(([^)\n]+)\)", body):
            target = target.strip()
            if target.startswith('<'):
                target = target[1:target.index('>')]
            else:
                target = target.split(' "', 1)[0].split(" '", 1)[0]
            link = urlsplit(target)
            if link.scheme or link.netloc:
                continue
            file = unquote(link.path)
            dest = posixpath.normpath(posixpath.join(posixpath.dirname(name), file)) if file else name
            if dest.startswith(('../', '/')) or dest not in available:
                raise ValueError(f"{name}: missing local target {target}")
            if link.fragment:
                if not dest.endswith('.md'):
                    raise ValueError(f"{name}: unsupported non-Markdown anchor {target}")
                if unquote(link.fragment) not in anchors(prose(read(dest), dest)):
                    raise ValueError(f"{name}: missing anchor {target}")
    return len(paths)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('paths', nargs='*', default=['docs'])
    parser.add_argument('--root', type=Path, default=ROOT)
    parser.add_argument('--ref', help='Git snapshot to read instead of working files')
    parser.add_argument('--base', help='Required ancestor for snapshot scope/diff checks')
    parser.add_argument('--docs-only', action='store_true')
    parser.add_argument('--strict-prose', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    for path in args.paths:
        if PurePosixPath(path).is_absolute() or '..' in PurePosixPath(path).parts:
            raise ValueError(f'Paths must be repository-relative: {path}')

    if (args.base and not args.ref) or (args.docs_only and not args.base):
        parser.error('--base requires --ref; --docs-only requires --base')
    if args.ref:
        head = git(root, 'rev-parse', '--verify', args.ref + '^{commit}')
        available = set(git(root, 'ls-tree', '-r', '--name-only', head).splitlines())
        read = lambda name: git(root, 'show', f'{head}:{name}')
        if args.base:
            base = git(root, 'rev-parse', '--verify', args.base + '^{commit}')
            subprocess.run(['git', '-C', str(root), 'merge-base', '--is-ancestor', base, head], check=True)
            subprocess.run(['git', '-C', str(root), 'diff', '--check', base, head], check=True)
            changed = git(root, 'diff', '--name-only', base, head).splitlines()
            if args.docs_only and any(not p.startswith('docs/') or not p.endswith('.md') for p in changed):
                raise ValueError('Snapshot change includes files outside docs/*.md')
    else:
        # Only walk selected directories; local links can target other repository files.
        available = set()
        for path in args.paths:
            item = root / path
            available.update(p.relative_to(root).as_posix() for p in
                             ([item] if item.is_file() else item.rglob('*.md')))
        class WorkingPaths(set):
            def __contains__(self, name):
                return (root / name).exists()
        available = WorkingPaths(available)
        read = lambda name: (root / name).read_text(encoding='utf-8')
    selected = set()
    for path in args.paths:
        matches = {p for p in available if p.endswith('.md') and (p == path or p.startswith(path.rstrip('/') + '/'))}
        if not matches:
            raise ValueError(f'No Markdown files found at {path}')
        selected.update(matches)
    count = validate(sorted(selected), available, read, args.strict_prose)
    print(f'{count} Markdown files passed links, anchors and fenced-block checks')


if __name__ == '__main__':
    try:
        main()
    except (ValueError, subprocess.CalledProcessError) as error:
        raise SystemExit(str(error))
