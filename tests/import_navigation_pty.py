#!/usr/bin/env python3
"""Exercise real Esc navigation: python3 tests/import_navigation_pty.py [binary]."""
import json
import os
from pathlib import Path
import select
import signal
import sys
import tempfile
import time
import zipfile

BINARY = str(Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/safeselect").resolve())


class Terminal:
    def __init__(self, root, arguments):
        self.pid, self.fd = os.forkpty()
        if self.pid == 0:
            os.chdir(root)
            os.environ.update(
                TERM="xterm-256color", NO_COLOR="1",
                SAFESELECT_CONFIG_DIR=str(root / "global"),
            )
            os.execv(BINARY, [BINARY] + arguments)
        self.data = b""
        self.transcript = b""

    def expect(self, needle):
        deadline = time.monotonic() + 10
        while needle.encode() not in self.data:
            if time.monotonic() > deadline:
                raise AssertionError(f"Timeout waiting for {needle}: {self.data[-400:]!r}")
            if select.select([self.fd], [], [], 0.1)[0]:
                try:
                    chunk = os.read(self.fd, 65536)
                except OSError:
                    chunk = b""
                if not chunk:
                    raise AssertionError(f"Process exited before {needle}: {self.data[-400:]!r}")
                self.data += chunk
                self.transcript += chunk
        result, self.data = self.data, b""
        return result

    def send(self, keys):
        os.write(self.fd, keys)

    def finish(self):
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            pid, status = os.waitpid(self.pid, os.WNOHANG)
            if pid:
                self.pid = None
                assert os.waitstatus_to_exitcode(status) == 0, status
                return
            if select.select([self.fd], [], [], 0.1)[0]:
                try:
                    self.data += os.read(self.fd, 65536)
                except OSError:
                    pass
        raise AssertionError("Process did not exit")

    def close(self):
        if self.pid is not None:
            try:
                os.kill(self.pid, signal.SIGKILL)
                os.waitpid(self.pid, 0)
            except ProcessLookupError:
                pass
        os.close(self.fd)


def fixture(root, provider):
    if provider == "compose":
        (root / "compose.yaml").write_text(
            "services:\n  db:\n    image: postgres:17\n    environment:\n"
            "      POSTGRES_USER: reader\n      POSTGRES_DB: app\n"
        )
        return ["import-compose", "--path", str(root)]
    if provider == "compass":
        path = root / "compass.json"
        path.write_text(json.dumps({"connections": [{
            "name": "dev", "connectionString": "mongodb://reader@db.example/app",
        }]}))
        return ["import-compass", "--path", str(path)]
    path = root / "export.zip"
    with zipfile.ZipFile(path, "w") as archive:
        archive.writestr("workspace/data-sources.json", json.dumps({"connections": [{
            "name": "dev", "driver": "postgres", "username": "reader",
            "configuration": {"url": "jdbc:postgresql://db.example:5432/app"},
        }]}))
    return ["import-dbeaver", str(path)]


def exercise(provider, save, interrupt=False, hidden=False):
    with tempfile.TemporaryDirectory(prefix="safeselect-pty-") as temporary:
        root = Path(temporary)
        terminal = Terminal(root, fixture(root, provider))
        try:
            terminal.expect("Select ")
            terminal.send(b" \r")
            terminal.expect("Environment name")
            terminal.send(b"edited\r")
            terminal.expect("How do you want")
            if hidden:
                terminal.send(b"\r")
                terminal.expect("Where do you want")
                terminal.send(b"\x1b[B\r" if sys.platform == "darwin" else b"\r")
                terminal.expect("Use the password only")
                terminal.send(b"y\r")
                terminal.expect("Password environment variable:")
                terminal.send(b"NAVIGATION_HIDDEN_PASSWORD\r")
                terminal.expect("Password (hidden):")
                terminal.send(b"synthetic-hidden-password\r")
                terminal.expect("Save this import batch?")
                terminal.send(b"n\r")
                terminal.expect("Import cancelled")
                terminal.finish()
                assert not (root / ".safeselect").exists()
                assert b"synthetic-hidden-password" not in terminal.transcript
            elif interrupt:
                terminal.send(b"\x03")
                terminal.expect("Import cancelled")
                terminal.finish()
                assert not (root / ".safeselect").exists()
            elif not save:
                terminal.send(b"\x1b")
                terminal.expect("Environment name")
                terminal.send(b"\x1b")
                terminal.expect("Select ")
                terminal.send(b"\x1b")
                terminal.expect("Import cancelled")
                terminal.finish()
                assert not (root / ".safeselect").exists()
            else:
                terminal.send(b"\x1b[B\x1b[B\r")
                terminal.expect("Password environment variable:")
                terminal.send(b"BEFORE_DB_PASSWORD\r")
                terminal.expect("Save this import batch?")
                assert not (root / ".safeselect").exists()
                terminal.send(b"\x1b")
                terminal.expect("Password environment variable:")
                terminal.send(b"AFTER_DB_PASSWORD\r")
                terminal.expect("Save this import batch?")
                terminal.send(b"y\r")
                terminal.expect("Check the imported connections now?")
                terminal.send(b"\x1b")
                terminal.finish()
                content = (root / ".safeselect/environments/edited.toml").read_text()
                assert "AFTER_DB_PASSWORD" in content
                assert "BEFORE_DB_PASSWORD" not in content
        finally:
            terminal.close()
    scenario = "hidden password cancellation" if hidden else "Ctrl+C cancellation" if interrupt else "revised save" if save else "back to selection and cancel"
    print(provider, scenario, "PASS")


if __name__ == "__main__":
    for provider in ("compose", "compass", "dbeaver"):
        for save in (False, True):
            exercise(provider, save)
        exercise(provider, False, interrupt=True)
        exercise(provider, False, hidden=True)
