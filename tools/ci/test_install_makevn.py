import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.bin = self.root / "fake-bin"
        self.bin.mkdir()
        self.script = self.root / "install-makevn.sh"
        shutil.copy(Path(__file__).with_name("install-makevn.sh"), self.script)
        layout = self.root / "makevn-1.0.0" / "bin"
        layout.mkdir(parents=True)
        self.executable(layout / "makevn", '#!/bin/sh\necho "makevn 1.0.0"\n')
        self.archive = self.root / "fixture.tar.gz"
        with tarfile.open(self.archive, "w:gz") as stream:
            stream.add(layout.parent, arcname="makevn-1.0.0")
        checksum = hashlib.sha256(self.archive.read_bytes()).hexdigest()
        self.lock = {"version": "v1.0.0", "sha256": {
            "x86_64-unknown-linux-gnu": checksum,
            "aarch64-apple-darwin": checksum,
            "x86_64-apple-darwin": checksum}}
        self.write_lock()
        self.executable(self.bin / "uname", '''#!/bin/sh
if [ "$1" = -s ]; then echo "${FAKE_OS:-Linux}"; else echo "${FAKE_ARCH:-x86_64}"; fi
''')
        self.executable(self.bin / "curl", '''#!/bin/sh
printf '%s\n' "$@" > "$ARGS_LOG"
[ "${DOWNLOAD_FAIL:-0}" = 0 ] || exit 22
while [ "$1" != --output ]; do shift; done
cp "$ARCHIVE" "$2"
''')
        self.env = {**os.environ, "PATH": str(self.bin) + os.pathsep + os.environ["PATH"],
                    "RUNNER_TEMP": str(self.root), "GITHUB_PATH": str(self.root / "github-path"),
                    "ARCHIVE": str(self.archive), "ARGS_LOG": str(self.root / "curl-args")}

    def executable(self, path, content):
        path.write_text(content)
        path.chmod(0o755)

    def write_lock(self):
        (self.root / "makevn.lock.json").write_text(json.dumps(self.lock))

    def run_installer(self):
        return subprocess.run(["bash", str(self.script)], env=self.env, text=True, capture_output=True)

    def test_verifies_archive_and_adds_only_verified_install_to_path(self):
        result = self.run_installer()
        self.assertEqual(result.returncode, 0, result.stderr)
        path = Path((self.root / "github-path").read_text().strip())
        self.assertTrue((path / "makevn").is_file())
        args = (self.root / "curl-args").read_text().splitlines()
        self.assertIn("--retry", args)
        self.assertIn("--retry-max-time", args)
        self.assertIn("--max-time", args)
        self.assertNotIn("latest", " ".join(args))
        self.assertNotIn("api.github.com", " ".join(args))

    def test_corrupted_archive_never_installed_or_added_to_path(self):
        self.archive.write_bytes(b"corrupt")
        self.assertNotEqual(self.run_installer().returncode, 0)
        self.assertFalse((self.root / "github-path").exists())
        self.assertFalse(list(self.root.glob("makevn-installed.*")))

    def test_missing_download_is_not_ignored(self):
        self.env["DOWNLOAD_FAIL"] = "1"
        self.assertNotEqual(self.run_installer().returncode, 0)
        self.assertFalse((self.root / "github-path").exists())

    def test_cross_compile_still_installs_runner_native_makevn(self):
        self.env["CARGO_BUILD_TARGET"] = "aarch64-unknown-linux-gnu"
        self.assertEqual(self.run_installer().returncode, 0)
        self.assertIn("x86_64-unknown-linux-gnu", (self.root / "curl-args").read_text())

    def test_apple_silicon_host(self):
        self.env.update(FAKE_OS="Darwin", FAKE_ARCH="arm64")
        self.assertEqual(self.run_installer().returncode, 0)
        self.assertIn("aarch64-apple-darwin", (self.root / "curl-args").read_text())

    def test_unsupported_host_fails_before_network(self):
        self.env.update(FAKE_OS="Linux", FAKE_ARCH="aarch64")
        self.assertNotEqual(self.run_installer().returncode, 0)
        self.assertFalse((self.root / "curl-args").exists())


if __name__ == "__main__":
    unittest.main()
