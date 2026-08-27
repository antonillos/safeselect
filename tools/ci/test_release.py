import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch
from io import BytesIO
from urllib.error import HTTPError

import release
import check_package_version
import publish_registry

VERSION = "v1.2.3"
SHA = "a" * 40


def fixtures(directory, targets=release.TARGETS, suffix=""):
    directory.mkdir(parents=True, exist_ok=True)
    for name in release.payloads(VERSION, targets):
        (directory / name).write_bytes((name + suffix).encode())
    release.complete_checksums(directory, release.payloads(VERSION, targets))


class Hosting:
    """Small stateful fake: exercise recovery after a real partial upload, not just call counts."""
    def __init__(self):
        self.files = {}
        self.exists = False
        self.draft = True
        self.prerelease = False
        self.tag = ""
        self.calls = []
        self.fail_after = None

    def info(self, *_):
        if not self.exists:
            return None
        return {"draft": self.draft, "prerelease": self.prerelease, "assets": [
            {"name": name, "state": "uploaded", "digest": "sha256:" + hashlib.sha256(data).hexdigest()}
            for name, data in self.files.items()]}

    def command(self, *args, **kwargs):
        self.calls.append(args)
        if args[:3] == ("git", "ls-remote", "--tags"):
            return f"{self.tag}\trefs/tags/{VERSION}" if self.tag else ""
        if args[:3] == ("git", "rev-parse", "HEAD"):
            return SHA
        operation = args[2]
        if operation == "create":
            assert "--draft" in args
            self.exists = True
            self.prerelease = "--prerelease" in args
        elif operation == "download":
            dest = Path(args[args.index("--dir") + 1])
            for index, arg in enumerate(args):
                if arg == "--pattern":
                    name = args[index + 1]
                    (dest / name).write_bytes(self.files[name])
        elif operation == "upload":
            assert "--clobber" not in args
            for item in args[6:]:
                path = Path(item)
                assert path.name not in self.files
                self.files[path.name] = path.read_bytes()
                if self.fail_after == len(self.files):
                    raise subprocess.CalledProcessError(1, args)
        elif operation == "edit":
            assert "--draft=false" in args
            self.draft = False
            self.tag = SHA
        else:
            raise AssertionError(f"Unexpected command {args}")
        return ""


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.local = self.root / "local"
        fixtures(self.local)
        self.host = Hosting()
        self.args = argparse.Namespace(directory=self.local, repo="owner/repo", version=VERSION,
                                       source=self.root, sha=SHA, notes=self.root / "notes",
                                       draft=False, prerelease=False, target=release.TARGETS[0])
        self.addCleanup(patch.stopall)
        patch.object(release, "command", side_effect=self.host.command).start()
        patch.object(release, "release_info", side_effect=self.host.info).start()

    def test_new_release_published_only_after_full_verification(self):
        release.publish(self.args)
        self.assertFalse(self.host.draft)
        self.assertEqual(set(self.host.files), set(release.expected_files(VERSION)))
        self.assertEqual(self.host.calls[-1][2], "edit")

    def test_missing_platform_does_not_create_release(self):
        (self.local / release.payloads(VERSION)[-1]).unlink()
        with self.assertRaises(ValueError):
            release.publish(self.args)
        self.assertFalse(self.host.exists)

    def test_corrupt_local_payload_does_not_create_release(self):
        (self.local / release.payloads(VERSION)[0]).write_text("corrupt")
        with self.assertRaisesRegex(ValueError, "Checksum mismatch"):
            release.publish(self.args)
        self.assertFalse(self.host.exists)

    def test_partial_upload_remains_draft_then_resumes_without_overwrite(self):
        self.host.fail_after = 5
        with self.assertRaises(subprocess.CalledProcessError):
            release.publish(self.args)
        before = self.host.files.copy()
        self.assertTrue(self.host.draft)
        self.host.fail_after = None
        release.publish(self.args)
        self.assertFalse(self.host.draft)
        self.assertTrue(all(self.host.files[k] == v for k, v in before.items()))

    def test_public_partial_release_preserves_existing_payloads_despite_rebuild(self):
        self.host.exists, self.host.draft, self.host.tag = True, False, SHA
        name = release.payloads(VERSION)[0]
        self.host.files[name] = b"original already published binary"
        release.publish(self.args)
        self.assertEqual(self.host.files[name], b"original already published binary")
        self.assertNotIn("edit", [c[2] for c in self.host.calls])

    def test_complete_public_release_does_not_upload_or_edit_again(self):
        release.publish(self.args)
        self.host.calls.clear()
        release.publish(self.args)
        self.assertFalse(any(c[2] in ("upload", "create", "edit") for c in self.host.calls))

    def test_explicit_draft_never_publishes(self):
        self.args.draft = True
        release.publish(self.args)
        self.assertTrue(self.host.draft)

    def test_public_release_cannot_be_hidden_as_draft(self):
        release.publish(self.args)
        self.args.draft = True
        with self.assertRaisesRegex(ValueError, "back into a draft"):
            release.publish(self.args)

    def test_refuses_tag_mismatch(self):
        self.host.tag = "b" * 40
        with self.assertRaisesRegex(ValueError, "Refusing to move"):
            release.publish(self.args)
        self.assertFalse(self.host.exists)

    def test_annotated_tag_compares_peeled_commit(self):
        with patch.object(release, "command", return_value=f'{"b"*40}\trefs/tags/{VERSION}\n{SHA}\trefs/tags/{VERSION}^{{}}'):
            release.verify_tag("owner/repo", VERSION, SHA, self.root)

    def test_prerelease_cannot_be_changed_on_retry(self):
        self.args.prerelease = True
        release.publish(self.args)
        self.args.prerelease = False
        with self.assertRaisesRegex(ValueError, "prerelease status"):
            release.publish(self.args)

    def test_orphan_checksum_cannot_be_silently_replaced(self):
        self.host.exists = True
        name = release.payloads(VERSION)[0]
        self.host.files[name + ".sha256"] = ("0" * 64 + "  " + name).encode()
        with self.assertRaisesRegex(ValueError, "Checksum mismatch"):
            release.publish(self.args)
        self.assertTrue(self.host.draft)

    def test_legacy_checksum_paths_are_not_followed(self):
        name = release.payloads(VERSION)[0]
        path = self.local / name
        (self.local / (name + ".sha256")).write_text(f"{release.digest(path)}  target/release/{name}\n")
        release.verify_assets(self.local, VERSION)

    def test_checksum_wrong_filename_is_rejected(self):
        name = release.payloads(VERSION)[0]
        (self.local / (name + ".sha256")).write_text("0" * 64 + "  another-file")
        with self.assertRaisesRegex(ValueError, "Malformed checksum"):
            release.verify_assets(self.local, VERSION)

    def test_reuses_existing_platform_without_rebuild(self):
        release.publish(self.args)
        self.args.directory = self.root / "reuse"
        with patch.object(release, "write_outputs") as outputs:
            release.reuse(self.args)
            outputs.assert_called_once_with({"reused": "true"})
        release.verify_assets(self.args.directory, VERSION, [self.args.target])

    def test_missing_platform_requests_build(self):
        with patch.object(release, "write_outputs") as outputs:
            release.reuse(self.args)
            outputs.assert_called_once_with({"reused": "false"})

    def test_resolve_freezes_source_and_rejects_wrong_version(self):
        (self.root / "Cargo.toml").write_text('[package]\nversion = "1.2.3"\n')
        with patch.object(release, "write_outputs") as outputs:
            release.resolve(self.args)
            outputs.assert_called_once_with({"version": VERSION, "semver": "1.2.3", "target-ref": SHA})
        self.args.version = "v1.2.4"
        with self.assertRaisesRegex(ValueError, "does not match"):
            release.resolve(self.args)

    def test_public_release_without_tag_is_rejected(self):
        self.host.exists, self.host.draft = True, False
        with self.assertRaisesRegex(ValueError, "existing tag"):
            release.publish(self.args)

    def test_remote_corruption_blocks_publication(self):
        self.host.exists = True
        name = release.payloads(VERSION)[0]
        self.host.files[name] = b"payload"
        original = self.host.command

        def corrupt(*args, **kwargs):
            result = original(*args, **kwargs)
            if args[:3] == ("gh", "release", "download"):
                (Path(args[args.index("--dir") + 1]) / name).write_bytes(b"corrupt")
            return result

        with patch.object(release, "command", side_effect=corrupt), self.assertRaisesRegex(ValueError, "asset digest mismatch"):
            release.publish(self.args)
        self.assertTrue(self.host.draft)

    def test_public_verification_rejects_partial_assets(self):
        self.host.exists, self.host.draft, self.host.tag = True, False, SHA
        self.args.directory = self.root / "downloaded"
        with self.assertRaisesRegex(ValueError, "Missing or empty"):
            release.check_public(self.args)

    def test_public_verification_blocks_draft(self):
        self.args.draft = True
        release.publish(self.args)
        with self.assertRaisesRegex(ValueError, "public stable release"):
            release.check_public(self.args)

    def test_registry_attachment_is_idempotent_and_no_clobber(self):
        release.publish(self.args)
        metadata = self.root / "server.json"
        metadata.write_text('{"name":"demo"}')
        release.attach_file("owner/repo", VERSION, metadata)
        release.attach_file("owner/repo", VERSION, metadata)
        metadata.write_text('{"name":"changed"}')
        with self.assertRaisesRegex(ValueError, "refusing to overwrite"):
            release.attach_file("owner/repo", VERSION, metadata)


class ApiTests(unittest.TestCase):
    def test_403_is_not_absent_release(self):
        response = subprocess.CompletedProcess([], 1, "", "gh: Forbidden (HTTP 403)")
        with patch("release.subprocess.run", return_value=response), self.assertRaises(RuntimeError):
            release.api("repos/owner/repo/releases/tags/v1.2.3")

    def test_404_is_absent_release(self):
        response = subprocess.CompletedProcess([], 1, "", "gh: Not Found (HTTP 404)")
        with patch("release.subprocess.run", return_value=response):
            self.assertIsNone(release.api("missing"))

    def test_transient_failure_is_retried_with_bound(self):
        response = subprocess.CompletedProcess([], 1, "", "gh: bad gateway (HTTP 502)")
        with patch("release.subprocess.run", return_value=response) as call, patch("release.time.sleep"), self.assertRaises(RuntimeError):
            release.api("unavailable")
        self.assertEqual(call.call_count, 4)


class PublicationTests(unittest.TestCase):
    def test_registry_checks_exact_version_and_equal_metadata(self):
        metadata = {"name": "io.github.owner/tool", "version": "1.2.3", "packages": []}
        response = BytesIO(json.dumps({"server": metadata}).encode())
        with patch.object(publish_registry, "urlopen", return_value=response) as request:
            self.assertTrue(publish_registry.registered(metadata))
        self.assertIn("io.github.owner%2Ftool/versions/1.2.3", request.call_args.args[0])

    def test_registry_only_404_allows_new_publication(self):
        metadata = {"name": "demo", "version": "1.2.3"}
        with patch.object(publish_registry, "urlopen", side_effect=HTTPError("url", 404, "Not Found", {}, None)):
            self.assertFalse(publish_registry.registered(metadata))
        with patch.object(publish_registry, "urlopen", side_effect=HTTPError("url", 403, "Forbidden", {}, None)), self.assertRaises(HTTPError):
            publish_registry.registered(metadata)

    def test_registry_different_metadata_requires_investigation(self):
        metadata = {"name": "demo", "version": "1.2.3"}
        response = BytesIO(json.dumps({"server": {**metadata, "version": "0.0.0"}}).encode())
        with patch.object(publish_registry, "urlopen", return_value=response), self.assertRaises(ValueError):
            publish_registry.registered(metadata)

    def test_registry_new_publication_then_verification(self):
        with tempfile.TemporaryDirectory() as work:
            metadata = Path(work) / "server.json"
            metadata.write_text('{"name":"demo", "version":"1.2.3"}')
            with patch.object(publish_registry, "registered", side_effect=[False, False, True]), patch.object(publish_registry, "command") as command, patch.object(publish_registry, "time"), patch.object(publish_registry, "attach_file") as attach:
                publish_registry.publish(metadata, Path("publisher"), "owner/repo", VERSION)
            self.assertEqual(command.call_count, 2)
            attach.assert_called_once()

    def test_registry_verification_timeout_does_not_attach_metadata(self):
        with tempfile.TemporaryDirectory() as work:
            metadata = Path(work) / "server.json"
            metadata.write_text('{}')
            with patch.object(publish_registry, "registered", return_value=False), patch.object(publish_registry, "command"), patch.object(publish_registry, "time"), patch.object(publish_registry, "attach_file") as attach:
                with self.assertRaisesRegex(ValueError, "did not become visible"):
                    publish_registry.publish(metadata, Path("publisher"), "owner/repo", VERSION)
            attach.assert_not_called()

    def test_refuses_homebrew_downgrade(self):
        with tempfile.TemporaryDirectory() as work:
            formula = Path(work) / "formula.rb"
            formula.write_text("https://github.com/example/releases/download/v1.2.4/file")
            with self.assertRaisesRegex(ValueError, "downgrade"):
                check_package_version.check(formula, VERSION)
            check_package_version.check(formula, "v1.2.4")

    def test_existing_registry_version_skips_publish(self):
        with tempfile.TemporaryDirectory() as work:
            metadata = Path(work) / "server.json"
            metadata.write_text('{"name":"demo", "version":"1.2.3"}')
            with patch.object(publish_registry, "registered", return_value=True), patch.object(publish_registry, "command") as command, patch.object(publish_registry, "attach_file") as attach:
                publish_registry.publish(metadata, Path("publisher"), "owner/repo", VERSION)
            command.assert_not_called()
            attach.assert_called_once()

    def test_registry_unavailable_does_not_publish_duplicate(self):
        with tempfile.TemporaryDirectory() as work:
            metadata = Path(work) / "server.json"
            metadata.write_text('{}')
            with patch.object(publish_registry, "registered", side_effect=OSError("network")), patch.object(publish_registry, "command") as command:
                with self.assertRaises(OSError):
                    publish_registry.publish(metadata, Path("publisher"), "owner/repo", VERSION)
            command.assert_not_called()


if __name__ == "__main__":
    unittest.main()
