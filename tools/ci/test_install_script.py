import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


class InstallScriptTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        shutil.copy(Path(__file__).parents[2] / "install.sh", self.root / "install.sh")
        (self.root / "Cargo.toml").write_text('version = "1.0.0"\n')
        self.env = {**os.environ, "PATH": str(self.bin) + os.pathsep + "/usr/bin:/bin",
                    "FAKE_BIN": str(self.bin), "PREFIX": str(self.root / ".local")}

    def executable(self, name, content):
        path = self.bin / name
        path.write_text(content)
        path.chmod(0o755)

    def run_installer(self, *args):
        return subprocess.run(["bash", str(self.root / "install.sh"), *args],
                              cwd=self.root, env=self.env, text=True, capture_output=True)

    def add_build_stubs(self):
        self.executable("cargo", '''#!/bin/sh
mkdir -p target/release
printf '#!/bin/sh\\n' > target/release/safeselect
chmod +x target/release/safeselect
''')

    def test_missing_makevn_fails_without_install_opt_in(self):
        result = self.run_installer()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("makevn is required", result.stderr)
        self.assertIn("--install-makevn", result.stderr)

    def test_help_describes_opt_in_bootstrap(self):
        result = self.run_installer("--help")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("--install-makevn", result.stdout)

    def test_bootstrap_prefers_homebrew_and_rechecks_path(self):
        self.add_build_stubs()
        self.executable("brew", '''#!/bin/sh
[ "$1" = install ] && [ "$2" = antonillos/tap/makevn ] || exit 1
cat > "$FAKE_BIN/makevn" <<'EOF'
#!/bin/sh
mkdir -p sidecar/target
: > sidecar/target/safeselect-sidecar-1.0.0.jar
EOF
chmod +x "$FAKE_BIN/makevn"
''')
        result = self.run_installer("--install-makevn")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Installing makevn with Homebrew", result.stdout)
        self.assertTrue((self.root / ".local/bin/safeselect").is_file())

    def test_bootstrap_uses_asdf_when_homebrew_is_unavailable(self):
        self.add_build_stubs()
        log = self.root / "asdf.log"
        self.env["ASDF_LOG"] = str(log)
        self.executable("asdf", '''#!/bin/sh
printf '%s\\n' "$*" >> "$ASDF_LOG"
case "$1 $2" in
  "plugin list") exit 1 ;;
  "plugin add") exit 0 ;;
  "latest makevn") echo 1.0.0 ;;
  "list makevn") exit 1 ;;
  "install makevn"|"set -u"|"reshim makevn") ;;
  *) exit 1 ;;
esac
if [ "$1 $2" = "reshim makevn" ]; then
  cat > "$FAKE_BIN/makevn" <<'EOF'
#!/bin/sh
mkdir -p sidecar/target
: > sidecar/target/safeselect-sidecar-1.0.0.jar
EOF
  chmod +x "$FAKE_BIN/makevn"
fi
''')
        result = self.run_installer("--install-makevn")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Installing makevn with asdf", result.stdout)
        calls = log.read_text()
        self.assertIn("plugin add makevn https://github.com/antonillos/asdf-makevn.git", calls)
        self.assertIn("install makevn 1.0.0", calls)


if __name__ == "__main__":
    unittest.main()
