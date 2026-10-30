"""离线验证安装器；模拟下载端，禁止访问真实 Release 或修改用户 PATH。"""

import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest
import zipfile


ROOT = Path(__file__).resolve().parents[1]
WINDOWS = os.name == "nt"


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="virse installer ")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.fixtures = self.root / "fixtures"
        self.fixtures.mkdir()
        self.dest = self.root / "bin with spaces"
        self.bin_name = "virse.exe" if WINDOWS else "virse"
        self.target = "x86_64-pc-windows-msvc" if WINDOWS else "x86_64-unknown-linux-musl"
        self.env = dict(os.environ)
        for key in ("VIRSE_VERSION", "VIRSE_TARGET", "VIRSE_INSTALL_DIR", "VIRSE_REPO"):
            self.env.pop(key, None)
        self.env.update(
            VIRSE_INSTALL_DIR=str(self.dest),
            VIRSE_TARGET=self.target,
            VIRSE_TEST_ROOT=str(self.root),
            VIRSE_TEST_SCRIPT=str(ROOT / ("install.ps1" if WINDOWS else "install.sh")),
        )
        (self.fixtures / "latest").write_text(json.dumps({"tag_name": "v1.2.3"}))
        if WINDOWS:
            wrapper = self.root / "mock-download.ps1"
            wrapper.write_text("""
function gh {
    if ($args[0] -eq 'auth') { $global:LASTEXITCODE = 1; return }
    throw 'Unexpected gh invocation in offline tests'
}
function Invoke-RestMethod { param($Uri); return @{tag_name='v1.2.3'} }
function Invoke-WebRequest {
    param($Uri, $OutFile, [switch]$UseBasicParsing)
    $name = ([Uri]$Uri).Segments[-1]
    Add-Content -LiteralPath (Join-Path $env:VIRSE_TEST_ROOT 'requests') -Value $name
    Copy-Item -LiteralPath (Join-Path $env:VIRSE_TEST_ROOT "fixtures/$name") -Destination $OutFile -ErrorAction Stop
}
& $env:VIRSE_TEST_SCRIPT -NoPathUpdate
""")
            self.command = [shutil.which("pwsh") or "powershell", "-NoProfile", "-File", str(wrapper)]
        else:
            mockbin = self.root / "mockbin"
            mockbin.mkdir()
            curl = mockbin / "curl"
            curl.write_text("""#!/bin/sh
set -eu
url=''
out=''
while [ "$#" -gt 0 ]; do
  case "$1" in
    https://*) url=$1; shift ;;
    -o) out=$2; shift 2 ;;
    *) shift ;;
  esac
done
name=${url##*/}
printf '%s\\n' "$name" >> "$VIRSE_TEST_ROOT/requests"
cp "$VIRSE_TEST_ROOT/fixtures/$name" "$out"
""")
            curl.chmod(0o755)
            gh = mockbin / "gh"
            gh.write_text('#!/bin/sh\nexit 1\n')
            gh.chmod(0o755)
            self.env["PATH"] = str(mockbin) + os.pathsep + self.env["PATH"]
            self.command = ["sh", str(ROOT / "install.sh")]
        self.make_archive()

    def make_archive(self, version="v1.2.3", payload=b"binary-v1", include_binary=True):
        stem = f"virse-{version}-{self.target}"
        self.archive = self.fixtures / (stem + (".zip" if WINDOWS else ".tar.gz"))
        # 故意放入文档，确认安装器只安装 bin；真实发行包另行检查只含 bin。
        files = {"README.md": b"must not be installed", "docs/guide.md": b"ignored"}
        if include_binary:
            files[self.bin_name] = payload
        if WINDOWS:
            with zipfile.ZipFile(self.archive, "w") as archive:
                for name, data in files.items():
                    archive.writestr(name, data)
        else:
            with tarfile.open(self.archive, "w:gz") as archive:
                for name, data in files.items():
                    member = tarfile.TarInfo(name)
                    member.size = len(data)
                    archive.addfile(member, io.BytesIO(data))
        self.checksum = self.fixtures / (stem + ".sha256")
        digest = hashlib.sha256(self.archive.read_bytes()).hexdigest()
        self.checksum.write_text(f"{digest}  {self.archive.name}\n")

    def run_installer(self, success=True):
        result = subprocess.run(self.command, env=self.env, capture_output=True, text=True)
        if success:
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        else:
            self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        return result

    def assert_only_binary(self, payload):
        self.assertEqual(sorted(p.name for p in self.dest.iterdir()), [self.bin_name])
        self.assertEqual((self.dest / self.bin_name).read_bytes(), payload)
        if not WINDOWS:
            self.assertTrue(os.access(self.dest / self.bin_name, os.X_OK))

    def test_latest_and_binary_only(self):
        self.run_installer()
        self.assert_only_binary(b"binary-v1")

    def test_explicit_version_and_upgrade(self):
        self.run_installer()
        self.env["VIRSE_VERSION"] = "1.2.4"
        self.make_archive("v1.2.4", b"binary-v2")
        self.run_installer()
        self.assert_only_binary(b"binary-v2")

    def test_corrupt_archive_preserves_installation(self):
        self.run_installer()
        self.archive.write_bytes(b"corrupt download")
        self.run_installer(success=False)
        self.assert_only_binary(b"binary-v1")

    def test_missing_checksum_preserves_installation(self):
        self.run_installer()
        self.checksum.unlink()
        self.run_installer(success=False)
        self.assert_only_binary(b"binary-v1")

    def test_missing_binary_preserves_installation(self):
        self.run_installer()
        self.make_archive(include_binary=False)
        self.run_installer(success=False)
        self.assert_only_binary(b"binary-v1")

    def test_unsupported_target(self):
        self.env["VIRSE_TARGET"] = "unsupported-target"
        self.run_installer(success=False)
        self.assertFalse(self.dest.exists())

    def test_authenticated_download(self):
        if WINDOWS:
            wrapper = self.root / 'mock-download.ps1'
            wrapper.write_text("""
function gh {
    $global:LASTEXITCODE = 0
    if ($args[0] -eq 'auth') { return }
    if ($args[1] -eq 'view') { return 'v1.2.3' }
    if ($args[1] -ne 'download') { throw 'Unexpected gh invocation' }
    $dir = $args[[Array]::IndexOf($args, '--dir') + 1]
    for ($i = 0; $i -lt $args.Count; $i++) {
        if ($args[$i] -eq '--pattern') {
            Copy-Item -LiteralPath (Join-Path $env:VIRSE_TEST_ROOT ('fixtures/' + $args[$i + 1])) -Destination $dir -ErrorAction Stop
        }
    }
}
function Invoke-WebRequest { throw 'Private releases must use gh' }
function Invoke-RestMethod { throw 'Private releases must use gh' }
& $env:VIRSE_TEST_SCRIPT -NoPathUpdate
""")
        else:
            gh = self.root / 'mockbin' / 'gh'
            gh.write_text("""#!/bin/sh
set -eu
case "$1 $2" in
  'auth status') exit 0 ;;
  'release view') echo v1.2.3; exit 0 ;;
  'release download') ;;
  *) exit 1 ;;
esac
patterns=''
out=''
while [ "$#" -gt 0 ]; do
  case "$1" in
    --pattern) patterns="$patterns $2"; shift 2 ;;
    --dir) out=$2; shift 2 ;;
    *) shift ;;
  esac
done
for name in $patterns; do cp "$VIRSE_TEST_ROOT/fixtures/$name" "$out/$name"; done
""")
            gh.chmod(0o755)
        self.run_installer()
        self.assert_only_binary(b'binary-v1')

    @unittest.skipIf(WINDOWS, "uname detection is specific to the shell installer")
    def test_platform_detection(self):
        self.env.pop("VIRSE_TARGET")
        uname = self.root / "mockbin" / "uname"
        uname.write_text('#!/bin/sh\ncase "$1" in -s) echo "$TEST_OS";; -m) echo "$TEST_ARCH";; esac\n')
        uname.chmod(0o755)
        for system, arch, target in [
            ("Linux", "x86_64", "x86_64-unknown-linux-musl"),
            ("Linux", "aarch64", "aarch64-unknown-linux-musl"),
            ("Darwin", "x86_64", "x86_64-apple-darwin"),
            ("Darwin", "arm64", "aarch64-apple-darwin"),
        ]:
            with self.subTest(target=target):
                self.target = target
                self.make_archive()
                self.env.update(TEST_OS=system, TEST_ARCH=arch)
                self.run_installer()
                self.assertIn(self.archive.name, (self.root / "requests").read_text())


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--archive", help="Also verify the actual release archive contains only its binary")
    options = parser.parse_args()
    if options.archive:
        archive_path = Path(options.archive)
        if archive_path.suffix == ".zip":
            with zipfile.ZipFile(archive_path) as archive:
                assert archive.namelist() == ["virse.exe"], archive.namelist()
        else:
            with tarfile.open(archive_path) as archive:
                assert archive.getnames() == ["virse"], archive.getnames()
                assert archive.getmembers()[0].isfile()
    unittest.main(argv=[__file__])
