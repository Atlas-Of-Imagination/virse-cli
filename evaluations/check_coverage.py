#!/usr/bin/env python3
"""Check atomic capability ownership and concrete reference destinations."""

import argparse
import json
import os
import re
import subprocess
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


# Declared by clap, but the current handler explicitly rejects this action.
# Keep the source reason visible rather than silently dropping an unknown leaf.
UNAVAILABLE = {'virse canvas edge name': 'src/canvas_edit.rs:202 rejects edge names'}


def command_leaves(binary):
    binary = binary.expanduser().resolve(strict=True)
    env = {k: v for k, v in os.environ.items() if not k.startswith('VIRSE_')}
    pending, leaves = [()], set()
    while pending:
        parts = pending.pop()
        if len(parts) > 8:
            raise ValueError('unexpected command nesting')
        result = subprocess.run([str(binary), *parts, '--help'], env=env, capture_output=True, text=True, timeout=10)
        if result.returncode:
            raise ValueError(f'cannot read command help: {" ".join(parts)}: {result.stderr.strip()}')
        children, in_commands = [], False
        for line in result.stdout.splitlines():
            if line == 'Commands:':
                in_commands = True
                continue
            if in_commands and line and not line.startswith(' '):
                in_commands = False
            if in_commands:
                match = re.match(r'^  ([a-z0-9][a-z0-9-]*)(?:\s|$)', line)
                if match and match[1] != 'help':
                    children.append(match[1])
        if children:
            pending.extend((*parts, child) for child in children)
        elif parts:
            leaves.add('virse ' + ' '.join(parts))
        else:
            raise ValueError('CLI root help has no command catalog')
    return leaves


def check_action_values(binary, capabilities):
    catalog = {row['id']: row for row in capabilities}
    cases = json.loads((ROOT / 'evaluations/command-actions.json').read_text())
    checked = {}
    for command, case in cases.items():
        for value, atom in case['values'].items():
            if atom not in catalog or command not in catalog[atom]['commands']:
                raise ValueError(f'{command} {value}: action has no matching atomic owner')
        env = {k: v for k, v in os.environ.items() if not k.startswith('VIRSE_')}
        result = subprocess.run([str(binary.expanduser().resolve(strict=True)), *command.split()[1:], '--help'],
                                env=env, capture_output=True, text=True, timeout=10)
        pattern = r'  <' + re.escape(case['argument']) + r'>[^\n]*\[possible values:\s*([^\]]+)\]'
        match = re.search(pattern, result.stdout)
        if result.returncode or not match:
            raise ValueError(f'cannot discover {command} {case["argument"]} choices')
        actual = {x.strip() for x in match[1].split(',')}
        expected = set(case['values'])
        if actual != expected:
            raise ValueError(json.dumps(dict(command=command, uncovered_actions=sorted(actual - expected),
                                            stale_actions=sorted(expected - actual)), ensure_ascii=False))
        checked[command] = len(actual)
    return checked


def check(binary=None):
    capabilities = json.loads((ROOT / 'evaluations/capabilities.json').read_text())
    expected = {row['id'] for row in capabilities}
    if len(expected) != len(capabilities):
        raise ValueError('duplicate capability registry ID')
    found, owners = {}, Counter()
    for file in sorted((ROOT / 'evaluations').glob('*-coverage.json')):
        skill = file.name.removesuffix('-coverage.json')
        if not (ROOT / 'skills' / skill / 'SKILL.md').is_file():
            raise ValueError(f'{file.name}: unknown Skill')
        for row in json.loads(file.read_text()):
            id = row['atom_id']
            if id not in expected or id in found:
                raise ValueError(f'{file.name}: unknown or duplicate capability {id}')
            path = (ROOT / row['reference']).resolve()
            if not path.is_relative_to(ROOT) or not path.is_file():
                raise ValueError(f'{id}: missing reference {row["reference"]}')
            headings = [line.lstrip('#').strip() for line in path.read_text().splitlines() if line.startswith('#')]
            if row['heading'] not in headings or not row['scenario'].strip():
                raise ValueError(f'{id}: missing reference heading or scenario')
            if path.is_relative_to(ROOT / 'shared'):
                packaged = ROOT / 'skills' / skill / 'references/shared' / path.relative_to(ROOT / 'shared')
                if not packaged.is_file() or packaged.read_bytes() != path.read_bytes():
                    raise ValueError(f'{id}: shared destination absent from standalone Skill')
            elif not path.is_relative_to(ROOT / 'skills' / skill):
                raise ValueError(f'{id}: owner relies on a sibling Skill')
            found[id] = skill
            owners[skill] += 1
    missing = sorted(expected - found.keys())
    if missing:
        raise ValueError('missing capabilities: ' + ', '.join(missing))
    contract_sections = 0
    for capability in capabilities:
        for section in capability.get('contract_sections', []):
            path = (ROOT / section['reference']).resolve()
            owner = found[capability['id']]
            if not path.is_relative_to(ROOT / 'skills' / owner) or not path.is_file():
                raise ValueError(f'{capability["id"]}: contract section has no owned reference')
            headings = [line.lstrip('#').strip() for line in path.read_text().splitlines() if line.startswith('#')]
            if section['heading'] not in headings or not section['name'].strip():
                raise ValueError(f'{capability["id"]}: contract section lacks concrete HOW')
            contract_sections += 1
    commands = {c for row in capabilities for c in row['commands']}
    result = dict(capabilities=len(found), commands=len(commands), contract_sections=contract_sections, owners=dict(owners))
    if binary:
        declared = command_leaves(binary)
        unavailable = {c: why for c, why in UNAVAILABLE.items() if c in declared}
        effective = declared - unavailable.keys()
        missing, stale = sorted(effective - commands), sorted(commands - effective)
        if missing or stale:
            raise ValueError(json.dumps(dict(uncovered_commands=missing, stale_commands=stale), ensure_ascii=False))
        result['live_cli'] = dict(declared=len(declared), effective=len(effective), excluded=unavailable)
        result['live_cli']['action_variants'] = check_action_values(binary, capabilities)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--json', action='store_true')
    parser.add_argument('--binary', type=Path, help='Also compare the real CLI help tree; never executes a business operation')
    args = parser.parse_args()
    try:
        result = check(args.binary)
    except (KeyError, ValueError, OSError, subprocess.TimeoutExpired) as error:
        parser.exit(1, str(error) + '\n')
    print(json.dumps(result, ensure_ascii=False, indent=2) if args.json else f'{result["capabilities"]} capabilities: unique owner and concrete packaged reference verified')


if __name__ == '__main__':
    main()
