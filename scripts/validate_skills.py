#!/usr/bin/env python3
"""只读校验 Skill 元数据、目录内引用和按名称定位的 base 依赖。"""

import argparse
import re
from pathlib import Path
from urllib.parse import unquote, urlsplit

import yaml


SKILLS = Path(__file__).resolve().parents[1] / 'skills'
LINK = re.compile(r'!?\[[^\]]*\]\(([^\s)]+)(?:\s+"[^"]*")?\)')
BASE_REFERENCE = re.compile(r'`virse-base/(references/[^`#]+)(?:#[^`]*)?`')
INDEPENDENT = {'virse-image-craft', 'virse-video-craft'}


def within(path, parent):
    return path.resolve().is_relative_to(parent.resolve())


def validate_frontmatter(skill):
    entry = skill / 'SKILL.md'
    text = entry.read_text(encoding='utf-8')
    match = re.match(r'\A---\n(.*?)\n---(?:\n|$)', text, re.S)
    if not match:
        raise ValueError(f'{entry}: missing YAML frontmatter')
    try:
        data = yaml.safe_load(match[1])
    except yaml.YAMLError as error:
        raise ValueError(f'{entry}: invalid YAML frontmatter: {error}') from error
    if not isinstance(data, dict):
        raise ValueError(f'{entry}: frontmatter must be a mapping')
    name = data.get('name')
    if (not isinstance(name, str) or len(name) > 64
            or not re.fullmatch(r'[a-z0-9]+(?:-[a-z0-9]+)*', name)
            or name != skill.name):
        raise ValueError(f'{entry}: invalid name or directory mismatch')
    description = data.get('description')
    if not isinstance(description, str) or not description.strip() or len(description) > 1024:
        raise ValueError(f'{entry}: description must be a nonempty string of at most 1024 characters')
    return text


def validate_skills(directory=SKILLS):
    skills = sorted(p.parent for p in directory.glob('*/SKILL.md'))
    if not skills:
        raise ValueError(f'{directory}: no Skills found')
    base = directory / 'virse-base'
    for skill in skills:
        # 仓库技能不使用符号链接，避免安装复制时丢失引用的文件。
        for path in [skill, *skill.rglob('*')]:
            if path.is_symlink():
                raise ValueError(f'{path}: symlink is not supported in Skill sources')
        entry = validate_frontmatter(skill)
        if skill != base and skill.name not in INDEPENDENT:
            if '依赖 `virse-base`' not in entry:
                raise ValueError(f'{skill.name}: declare the virse-base dependency')
            if not (base / 'SKILL.md').is_file():
                raise ValueError(f'{skill.name}: missing required virse-base/SKILL.md')
        if skill.name in INDEPENDENT and '依赖 `virse-base`' in entry:
            raise ValueError(f'{skill.name}: independent craft Skill depends on base')
        for path in skill.rglob('*.md'):
            text = path.read_text(encoding='utf-8')
            for link in LINK.findall(text):
                parsed = urlsplit(link.strip('<>'))
                if parsed.scheme or parsed.netloc or not parsed.path:
                    continue
                target = path.parent / unquote(parsed.path)
                if not within(target, skill) or not target.is_file():
                    raise ValueError(f'{path}: broken or external local reference {link}')
            for reference in BASE_REFERENCE.findall(text):
                # 入口中的路径格式说明不是文件引用。
                if reference == 'references/...':
                    continue
                if skill.name in INDEPENDENT:
                    raise ValueError(f'{path}: independent craft Skill depends on base')
                target = base / reference
                if not (base / 'SKILL.md').is_file() or not within(target, base) or not target.is_file():
                    raise ValueError(f'{path}: missing or external base reference {reference}')
    return len(skills)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', nargs='?', type=Path, default=SKILLS,
                        help='Skill container directory; defaults to this repository\'s skills/')
    args = parser.parse_args()
    try:
        count = validate_skills(args.directory)
    except (ValueError, OSError) as error:
        parser.exit(1, str(error) + '\n')
    print(f'{count} Skills: metadata, local references and base dependencies verified')


if __name__ == '__main__':
    main()
