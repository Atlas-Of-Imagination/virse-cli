#!/usr/bin/env python3
"""Synchronize shared Skill text; optionally export independent Skill folders."""

import argparse
import re
import shutil
from pathlib import Path
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]
START = '<!-- virse:background:start -->'
END = '<!-- virse:background:end -->'
LINK = re.compile(r'!?\[[^\]]*\]\(([^\s)]+)(?:\s+"[^"]*")?\)')


def local_links(text):
    for target in LINK.findall(text):
        parsed = urlsplit(target.strip('<>'))
        if parsed.scheme or parsed.netloc or not parsed.path:
            continue
        yield unquote(parsed.path)


def within(path, parent):
    return path.resolve().is_relative_to(parent.resolve())


def reject_linked_sources(directory):
    # Validate before reading or synchronizing: a linked SKILL.md must never be written.
    for path in [directory, *directory.rglob('*')]:
        if path.is_symlink():
            raise ValueError(f'{path}: symlinks cannot form a standalone package')
        if path.is_file() and path.stat().st_nlink > 1:
            raise ValueError(f'{path}: hard-linked source files cannot be synchronized')


def shared_files(skill):
    """Resolve only shared files reachable from this Skill's own Markdown."""
    destination = skill / 'references/shared'
    wanted = {}
    pending = []
    for path in skill.rglob('*.md'):
        if within(path, destination):
            continue
        for link in local_links(path.read_text()):
            target = (path.parent / link).resolve()
            if within(target, destination):
                pending.append(target.relative_to(destination))
    while pending:
        relative = pending.pop()
        if relative in wanted:
            continue
        source = ROOT / 'shared' / relative
        if not within(source, ROOT / 'shared') or not source.is_file() or source.is_symlink():
            raise ValueError(f'{skill.name}: missing shared source {relative}')
        wanted[relative] = source.read_bytes()
        if source.suffix == '.md':
            for link in local_links(source.read_text()):
                target = (source.parent / link).resolve()
                if not within(target, ROOT / 'shared'):
                    raise ValueError(f'{source}: shared reference escapes its folder: {link}')
                pending.append(target.relative_to(ROOT / 'shared'))
    return wanted


def synchronize(skill, check):
    reject_linked_sources(skill)
    entry = skill / 'SKILL.md'
    text = entry.read_text()
    if text.count(START) != 1 or text.count(END) != 1:
        raise ValueError(f'{entry}: expected one shared-background block')
    before, rest = text.split(START)
    _, after = rest.split(END)
    updated = before + START + '\n' + (ROOT / 'shared/product-background.md').read_text().strip() + '\n' + END + after
    if text != updated:
        if check:
            raise ValueError(f'{entry}: shared background out of sync')
        entry.write_text(updated)
    wanted = shared_files(skill)
    destination = skill / 'references/shared'
    actual = {p.relative_to(destination): p.read_bytes() for p in destination.rglob('*') if p.is_file()}
    if actual != wanted:
        if check:
            raise ValueError(f'{skill.name}: packaged shared references out of sync')
        if destination.exists():
            shutil.rmtree(destination)
        for relative, data in wanted.items():
            path = destination / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    for path in skill.rglob('*.md'):
        for link in local_links(path.read_text()):
            target = (path.parent / link).resolve()
            if not within(target, skill) or not target.is_file():
                raise ValueError(f'{path}: broken or external local reference {link}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true', help='Check without modifying source files')
    parser.add_argument('--output', type=Path, help='Export independent folders into a new directory')
    args = parser.parse_args()
    skills_directory = ROOT / 'skills'
    skills = sorted(p.parent for p in skills_directory.glob('*/SKILL.md'))
    if not skills:
        parser.error('no Skills found')
    if args.output:
        output = args.output.resolve()
        if output.exists() or within(output, ROOT / 'skills') or output == ROOT:
            parser.error('--output must be a new directory outside skills/')
    try:
        reject_linked_sources(skills_directory)
        reject_linked_sources(ROOT / 'shared')
        for skill in skills:
            synchronize(skill, args.check)
    except ValueError as error:
        parser.exit(1, str(error) + '\n')
    if args.output:
        output.mkdir(parents=True)
        for skill in skills:
            shutil.copytree(skill, output / skill.name)
    print(f'{len(skills)} Skills: background and standalone reference closure verified')


if __name__ == '__main__':
    main()
