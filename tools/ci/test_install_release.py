import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


class ReleaseInstallerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.fake_bin = self.root / "bin"
        self.fake_bin.mkdir()
        self.prefix = self.root / "prefix"
        self.temp_root = self.root / "tmp"
        self.temp_root.mkdir()
        self.archive = b"synthetic release archive"
        self.archive_sha = hashlib.sha256(self.archive).hexdigest()
        self.script = self.root / "install-release.sh"
        release_script = Path(__file__).resolve().parents[2] / "packaging/install/install-release.sh"
        shutil.copy(release_script, self.script)
        self.script.chmod(0o755)

        self.executable(self.fake_bin / "uname", '''#!/bin/sh
if [ "$1" = "-s" ]; then echo "${FAKE_OS:-Darwin}"; else echo "${FAKE_ARCH:-arm64}"; fi
''')
        self.executable(self.fake_bin / "ldd", '''#!/bin/sh
echo "${LDD_OUTPUT:-musl libc (synthetic)}"
''')
        self.executable(self.fake_bin / "curl", '''#!/bin/sh
printf '%s\n' "$*" >> "$CURL_LOG"
url=""
output=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        -o) output="$2"; shift 2 ;;
        https://*) url="$1"; shift ;;
        *) shift ;;
    esac
done
if [ "$url" = "https://api.github.com/repos/antonillos/safeselect/releases/latest" ]; then
    printf '{"tag_name":"v1.2.3"}\n'
elif [ "$CURL_MODE" = mismatch ]; then
    printf '%s  safeselect-v1.2.3-aarch64-apple-darwin.tar.gz\n' "$WRONG_SHA" > "$output"
elif printf '%s' "$url" | grep -q '\\.sha256$'; then
    printf '%s  safeselect-v1.2.3-aarch64-apple-darwin.tar.gz\n' "$ARCHIVE_SHA" > "$output"
else
    printf '%s' "$ARCHIVE_CONTENT" > "$output"
fi
''')
        self.executable(self.fake_bin / "tar", '''#!/bin/sh
printf '#!/bin/sh\necho synthetic safeselect\n' > safeselect
chmod +x safeselect
''')
        self.env = {
            **os.environ,
            "PATH": str(self.fake_bin) + os.pathsep + os.environ["PATH"],
            "PREFIX": str(self.prefix),
            "HOME": str(self.root),
            "TMPDIR": str(self.temp_root),
            "CURL_LOG": str(self.root / "curl.log"),
            "ARCHIVE_CONTENT": self.archive.decode(),
            "ARCHIVE_SHA": self.archive_sha,
            "WRONG_SHA": "0" * 64,
        }

    @staticmethod
    def executable(path, content):
        path.write_text(content)
        path.chmod(0o755)

    def run_installer(self, **extra_env):
        env = {**self.env, **extra_env}
        return subprocess.run([str(self.script)], env=env, text=True,
                              capture_output=True, timeout=10)

    def test_downloads_versioned_asset_and_verifies_checksum(self):
        result = self.run_installer()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((self.prefix / "bin/safeselect").is_file())
        self.assertIn("Verified SHA-256 checksum.", result.stdout)
        requests = (self.root / "curl.log").read_text()
        self.assertIn("safeselect-v1.2.3-aarch64-apple-darwin.tar.gz", requests)
        self.assertIn("safeselect-v1.2.3-aarch64-apple-darwin.tar.gz.sha256", requests)
        self.assertNotIn("safeselect-aarch64-apple-darwin.tar.gz", requests)

    def test_checksum_mismatch_never_installs_binary(self):
        result = self.run_installer(CURL_MODE="mismatch")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("SHA-256 checksum mismatch", result.stderr)
        self.assertFalse((self.prefix / "bin/safeselect").exists())
        self.assertEqual(list(self.temp_root.iterdir()), [])

    def test_explicit_version_accepts_optional_v_prefix(self):
        result = self.run_installer(SAFESELECT_VERSION="v1.2.3")
        self.assertEqual(result.returncode, 0, result.stderr)
        requests = (self.root / "curl.log").read_text()
        self.assertNotIn("/vv1.2.3/", requests)

    def test_musl_linux_fails_before_downloading_glibc_binary(self):
        result = self.run_installer(FAKE_OS="Linux", FAKE_ARCH="x86_64")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("musl", result.stderr)
        self.assertFalse((self.root / "curl.log").exists())

    def test_glibc_linux_is_accepted_when_musl_tools_are_installed(self):
        result = self.run_installer(FAKE_OS="Linux", FAKE_ARCH="x86_64",
                                    LDD_OUTPUT="ldd (GNU libc) 2.39")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((self.prefix / "bin/safeselect").is_file())


if __name__ == "__main__":
    unittest.main()
