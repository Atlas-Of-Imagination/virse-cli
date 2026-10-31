#!/usr/bin/env python3
"""Exercise independent package export and reject writes through linked sources."""

import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ENTRY = '---\nname: example\ndescription: Example\n---\n<!-- virse:background:start -->\n<!-- virse:background:end -->\n[Guide](references/shared/guide.md)\n'


class Packaging(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name).resolve() / 'repo'
        (self.root / 'scripts').mkdir(parents=True)
        shutil.copyfile(ROOT / 'scripts/package_skills.py', self.root / 'scripts/package_skills.py')
        (self.root / 'shared').mkdir()
        (self.root / 'shared/product-background.md').write_text('Product background.\n')
        (self.root / 'shared/guide.md').write_text('# Guide\n[Detail](detail.md)\n')
        (self.root / 'shared/detail.md').write_text('# Detail\n')
        self.skill = self.root / 'skills/example'
        self.skill.mkdir(parents=True)
        (self.skill / 'SKILL.md').write_text(ENTRY)

    def tearDown(self):
        self.temp.cleanup()

    def run_script(self, *args):
        return subprocess.run([sys.executable, str(self.root / 'scripts/package_skills.py'), *args], capture_output=True, text=True)

    def test_export_remains_usable_without_source(self):
        output = Path(self.temp.name) / 'export'
        result = self.run_script('--output', str(output))
        self.assertEqual(result.returncode, 0, result.stderr)
        shutil.rmtree(self.root)
        self.assertIn('Product background.', (output / 'example/SKILL.md').read_text())
        self.assertEqual((output / 'example/references/shared/detail.md').read_text(), '# Detail\n')
        self.assertFalse(any(p.is_symlink() for p in output.rglob('*')))

    def test_linked_entry_cannot_modify_external_file(self):
        outside = Path(self.temp.name) / 'outside.md'
        outside.write_text(ENTRY)
        (self.skill / 'SKILL.md').unlink()
        (self.skill / 'SKILL.md').symlink_to(outside)
        result = self.run_script()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(outside.read_text(), ENTRY)

    def test_linked_skill_root_cannot_modify_external_directory(self):
        outside = Path(self.temp.name) / 'outside-skill'
        shutil.move(str(self.skill), outside)
        self.skill.symlink_to(outside, target_is_directory=True)
        result = self.run_script()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual((outside / 'SKILL.md').read_text(), ENTRY)
        self.assertFalse((outside / 'references').exists())

    def test_hardlinked_entry_cannot_modify_external_file(self):
        outside = Path(self.temp.name) / 'outside.md'
        outside.write_text(ENTRY)
        (self.skill / 'SKILL.md').unlink()
        (self.skill / 'SKILL.md').hardlink_to(outside)
        result = self.run_script()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(outside.read_text(), ENTRY)

    def test_check_and_existing_output_leave_source_unchanged(self):
        result = self.run_script('--check')
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual((self.skill / 'SKILL.md').read_text(), ENTRY)
        result = self.run_script('--output', str(self.root))
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual((self.skill / 'SKILL.md').read_text(), ENTRY)


if __name__ == '__main__':
    unittest.main()
