import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


class UninstallScriptTests(unittest.TestCase):
    def test_removes_binary_without_agent_configuration_files(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            script = root / "uninstall.sh"
            shutil.copy(Path(__file__).parents[2] / "uninstall.sh", script)
            prefix = root / "prefix"
            binary = prefix / "bin/safeselect"
            binary.parent.mkdir(parents=True)
            binary.write_text("fixture")
            result = subprocess.run(
                ["bash", str(script)], input="y\n", text=True, capture_output=True,
                env={**os.environ, "HOME": str(root), "PREFIX": str(prefix)},
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertFalse(binary.exists())
            self.assertIn("Uninstall complete.", result.stdout)


if __name__ == "__main__":
    unittest.main()
