"""验证源码和选择安装的技能集合；只使用临时目录。"""

import shutil
import tempfile
import unittest
from pathlib import Path

from validate_skills import validate_skills


class SkillValidationTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.base = self.skill('virse-base', '[对象](references/objects.md)\n')
        self.write('virse-base/references/objects.md', '# Objects\n')
        self.business = self.skill(
            'virse-example', '依赖 `virse-base`。\n[交付](references/delivery.md)\n'
        )
        self.write('virse-example/references/delivery.md',
                   '读 `virse-base/references/objects.md`。\n')

    def write(self, relative, content):
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding='utf-8')
        return path

    def skill(self, name, body):
        return self.write(f'{name}/SKILL.md',
                          f'---\nname: {name}\ndescription: Test skill\n---\n\n{body}').parent

    def test_validation_is_read_only(self):
        before = {p.relative_to(self.root): p.read_bytes()
                  for p in self.root.rglob('*') if p.is_file()}
        self.assertEqual(validate_skills(self.root), 2)
        after = {p.relative_to(self.root): p.read_bytes()
                 for p in self.root.rglob('*') if p.is_file()}
        self.assertEqual(before, after)

    def test_missing_base_is_rejected(self):
        shutil.rmtree(self.base)
        with self.assertRaisesRegex(ValueError, 'missing required virse-base'):
            validate_skills(self.root)

    def test_missing_base_reference_is_rejected(self):
        self.write('virse-example/references/delivery.md',
                   '`virse-base/references/missing.md`')
        with self.assertRaisesRegex(ValueError, 'missing or external base reference'):
            validate_skills(self.root)

    def test_missing_dependency_declaration_is_rejected(self):
        self.skill('virse-example', '# No dependency declared\n')
        with self.assertRaisesRegex(ValueError, 'declare the virse-base dependency'):
            validate_skills(self.root)

    def test_references_cannot_escape_their_skill(self):
        for reference in ('`virse-base/references/../../virse-example/SKILL.md`',
                          '[outside](../../virse-base/SKILL.md)'):
            with self.subTest(reference=reference):
                self.write('virse-example/references/delivery.md', reference)
                with self.assertRaisesRegex(ValueError, 'external'):
                    validate_skills(self.root)

    def test_missing_local_file_is_rejected(self):
        (self.business / 'references/delivery.md').unlink()
        with self.assertRaisesRegex(ValueError, 'broken or external local reference'):
            validate_skills(self.root)

    def test_craft_can_be_installed_without_base(self):
        shutil.rmtree(self.base)
        shutil.rmtree(self.business)
        self.skill('virse-image-craft', '# Image craft\n')
        self.skill('virse-video-craft', '# Video craft\n')
        self.assertEqual(validate_skills(self.root), 2)
        self.write('virse-image-craft/references/unexpected.md',
                   '`virse-base/references/objects.md`')
        with self.assertRaisesRegex(ValueError, 'independent craft Skill'):
            validate_skills(self.root)

    def test_invalid_frontmatter_is_rejected(self):
        for frontmatter in ('name: [', '[]', 'name: other\ndescription: text',
                            'name: virse-base\ndescription: 123',
                            'name: virse-base',
                            'name: virse-base\ndescription: ' + 'x' * 1025):
            with self.subTest(frontmatter=frontmatter):
                self.write('virse-base/SKILL.md', f'---\n{frontmatter}\n---\n')
                with self.assertRaises(ValueError):
                    validate_skills(self.root)

    def test_folded_yaml_description_is_supported(self):
        self.write('virse-base/SKILL.md',
                   '---\nname: virse-base\ndescription: >-\n  First line\n  Second line\n---\n')
        self.assertEqual(validate_skills(self.root), 2)


if __name__ == '__main__':
    unittest.main()
