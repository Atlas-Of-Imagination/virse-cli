"""验证独立安装所需的文件边界和 base 引用，不访问线上服务。"""

import shutil
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import package_skills as package


class SkillPackageTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.patch = patch.object(package, 'ROOT', self.root)
        self.patch.start()
        self.addCleanup(self.patch.stop)
        self.base = self.write('skills/virse-base/SKILL.md', '# Base\n').parent
        self.write('skills/virse-base/references/context/objects.md', '# Objects\n')
        self.business = self.write(
            'skills/virse-example/SKILL.md',
            '依赖 `virse-base`。\n[交付](references/shared/delivery.md)\n',
        ).parent
        self.write(
            'shared/delivery.md',
            '读 `virse-base/references/context/objects.md`，再读[细节](detail.md)。\n',
        )
        self.write('shared/detail.md', '# Detail\n')

    def write(self, relative, content):
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)
        return path

    def test_domain_closure_keeps_base_only_in_base(self):
        package.synchronize(self.business, check=False)
        package.synchronize(self.business, check=True)
        package.validate_base_dependencies([self.base, self.business])
        bundled = self.business / 'references/shared'
        self.assertEqual(
            {p.name for p in bundled.iterdir()}, {'delivery.md', 'detail.md'}
        )
        # 修改唯一的 base 文件不需要同步业务技能内的副本。
        self.write('skills/virse-base/references/context/objects.md', '# Updated\n')
        package.synchronize(self.business, check=True)

    def test_missing_base_is_rejected(self):
        shutil.rmtree(self.base)
        with self.assertRaisesRegex(ValueError, 'missing required virse-base'):
            package.validate_base_dependencies([self.business])

    def test_missing_base_reference_is_rejected(self):
        package.synchronize(self.business, check=False)
        (self.base / 'references/context/objects.md').unlink()
        with self.assertRaisesRegex(ValueError, 'missing base reference'):
            package.validate_base_dependencies([self.base, self.business])

    def test_reference_cannot_escape_base(self):
        self.write('secret.md', 'outside base')
        self.write(
            'skills/virse-example/references/escape.md',
            '`virse-base/references/../../../secret.md`',
        )
        with self.assertRaisesRegex(ValueError, 'missing base reference'):
            package.validate_base_dependencies([self.base, self.business])

    def test_craft_is_independent_and_cannot_acquire_base_references(self):
        craft = self.write('skills/virse-image-craft/SKILL.md', '# Prompt craft\n').parent
        package.validate_base_dependencies([self.base, craft])
        self.write(
            'skills/virse-image-craft/references/unexpected.md',
            '`virse-base/references/context/objects.md`',
        )
        with self.assertRaisesRegex(ValueError, 'independent craft Skill'):
            package.validate_base_dependencies([self.base, craft])

    def test_stale_domain_copy_is_rejected_then_resynchronized(self):
        package.synchronize(self.business, check=False)
        self.write('shared/detail.md', '# Changed\n')
        with self.assertRaisesRegex(ValueError, 'out of sync'):
            package.synchronize(self.business, check=True)
        package.synchronize(self.business, check=False)
        self.assertEqual(
            (self.business / 'references/shared/detail.md').read_text(), '# Changed\n'
        )


if __name__ == '__main__':
    unittest.main()
