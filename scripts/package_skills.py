#!/usr/bin/env python3
"""同步领域参考，校验 base 依赖，并导出可一起安装的 Skill 集合。"""

import argparse
import re
import shutil
from pathlib import Path
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]
LINK = re.compile(r'!?\[[^\]]*\]\(([^\s)]+)(?:\s+"[^"]*")?\)')
BASE_REFERENCE = re.compile(r'`virse-base/(references/[^`#]+)(?:#[^`]*)?`')
INDEPENDENT = {'virse-image-craft', 'virse-video-craft'}


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


def validate_base_dependencies(skills):
    """跨技能参考按技能名定位；不依赖安装后的兄弟目录或全局路径。"""
    base = ROOT / 'skills/virse-base'
    if not (base / 'SKILL.md').is_file():
        raise ValueError('missing required virse-base/SKILL.md')
    for skill in skills:
        if skill != base and skill.name not in INDEPENDENT:
            if '依赖 `virse-base`' not in (skill / 'SKILL.md').read_text():
                raise ValueError(f'{skill.name}: declare the virse-base dependency')
        for path in skill.rglob('*.md'):
            for reference in BASE_REFERENCE.findall(path.read_text()):
                # 入口中的路径格式说明不是实际文件引用。
                if reference == 'references/...':
                    continue
                if skill.name in INDEPENDENT:
                    raise ValueError(f'{path}: independent craft Skill depends on base')
                target = base / reference
                if not within(target, base) or not target.is_file():
                    raise ValueError(f'{path}: missing base reference {reference}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true', help='Check without modifying source files')
    parser.add_argument('--output', type=Path, help='Export the Skill set, including required base, into a new directory')
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
        validate_base_dependencies(skills)
    except ValueError as error:
        parser.exit(1, str(error) + '\n')
    if args.output:
        output.mkdir(parents=True)
        for skill in skills:
            shutil.copytree(skill, output / skill.name)
    print(f'{len(skills)} Skills: domain references and base dependencies verified')


if __name__ == '__main__':
    main()
