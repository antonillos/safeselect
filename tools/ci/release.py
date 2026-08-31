#!/usr/bin/env python3
"""Fail-closed, resumable release staging. No tag moves or asset replacement."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import time
from urllib.parse import quote

TARGETS = (
    "aarch64-apple-darwin", "x86_64-apple-darwin",
    "aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu",
)


def command(*args, **kwargs):
    return subprocess.check_output(args, text=True, **kwargs).strip()


def api(path):
    """Only an explicit 404 means absent; permissions/network failures must stop publication."""
    for attempt in range(4):
        result = subprocess.run(["gh", "api", path], text=True, capture_output=True)
        if result.returncode == 0:
            return json.loads(result.stdout)
        if "(HTTP 404)" in result.stderr:
            return None
        transient = any(s in result.stderr for s in ("(HTTP 502)", "(HTTP 503)", "(HTTP 504)"))
        if not transient or attempt == 3:
            raise RuntimeError(f"GitHub API failed: {result.stderr.strip()}")
        time.sleep(2 ** attempt)
    raise AssertionError("unreachable")


def release_info(repo, version):
    info = api(f"repos/{repo}/releases/tags/{quote(version, safe='')}")
    if info is not None:
        return info
    # GitHub does not expose drafts through the tag endpoint.  Listing releases
    # keeps a newly-created draft resumable before it is published.
    releases = api(f"repos/{repo}/releases?per_page=100")
    return next((release for release in releases if release.get("tag_name") == version), None)


def payloads(version, targets=TARGETS):
    return [f"safeselect-{version}-{target}.{suffix}"
            for target in targets for suffix in ("tar.gz", "mcpb")]


def expected_files(version, targets=TARGETS):
    return [file for name in payloads(version, targets) for file in (name, name + ".sha256")]


def digest(path):
    sha = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            sha.update(chunk)
    return sha.hexdigest()


def verify_pair(directory, name):
    payload = directory / name
    checksum = directory / (name + ".sha256")
    if not payload.is_file() or payload.stat().st_size == 0:
        raise ValueError(f"Missing or empty payload: {name}")
    text = checksum.read_text().strip()
    # Legacy releases include build-relative paths in checksum files. Never execute those paths.
    match = re.fullmatch(r"([a-fA-F0-9]{64})\s+\*?(.+)", text)
    if not match or Path(match[2]).name != name:
        raise ValueError(f"Malformed checksum for {name}")
    if digest(payload) != match[1].lower():
        raise ValueError(f"Checksum mismatch for {name}")


def verify_assets(directory, version, targets=TARGETS):
    for name in payloads(version, targets):
        verify_pair(directory, name)


def verify_tag(repo, version, sha, source):
    refs = command("git", "ls-remote", "--tags", "origin",
                   f"refs/tags/{version}", f"refs/tags/{version}^{{}}", cwd=source)
    entries = dict(line.split()[::-1] for line in refs.splitlines())
    tag_sha = entries.get(f"refs/tags/{version}^{{}}", entries.get(f"refs/tags/{version}"))
    if tag_sha and tag_sha != sha:
        raise ValueError(f"Refusing to move {version}: tag is {tag_sha}, requested source is {sha}")
    info = release_info(repo, version)
    if info and not info["draft"] and not tag_sha:
        raise ValueError("A public release must have an existing tag")
    if info and info["draft"] and not tag_sha and info.get("target_commitish") != sha:
        raise ValueError("Refusing to reuse an untagged draft with a different target")
    return info


def write_outputs(values):
    text = "".join(f"{key}={value}\n" for key, value in values.items())
    print(text, end="")
    if os.environ.get("GITHUB_OUTPUT"):
        with open(os.environ["GITHUB_OUTPUT"], "a") as stream:
            stream.write(text)


def resolve(args):
    manifest = (args.source / "Cargo.toml").read_text()
    current = re.search(r'^version = "([^"]+)"', manifest, re.MULTILINE)[1]
    version = args.version or "v" + current
    if not re.fullmatch(r"v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?", version):
        raise ValueError("Invalid release tag")
    if version != "v" + current:
        raise ValueError("Requested version does not match Cargo.toml")
    sha = command("git", "rev-parse", "HEAD", cwd=args.source)
    verify_tag(args.repo, version, sha, args.source)
    write_outputs({"version": version, "semver": current, "target-ref": sha})


def download_existing(repo, version, info, directory, targets=TARGETS):
    directory.mkdir(parents=True, exist_ok=True)
    expected = set(expected_files(version, targets))
    assets = {a["name"]: a for a in info["assets"] if a["name"] in expected}
    if not assets:
        return assets
    if any(a["state"] != "uploaded" for a in assets.values()):
        raise ValueError("Release contains unfinished uploads; investigate before retrying")
    patterns = [arg for name in sorted(assets) for arg in ("--pattern", name)]
    command("gh", "release", "download", version, "--repo", repo,
            "--dir", str(directory), *patterns)
    for name, asset in assets.items():
        expected_digest = asset.get("digest")
        if expected_digest and expected_digest != "sha256:" + digest(directory / name):
            raise ValueError(f"GitHub asset digest mismatch: {name}")
    return assets


def reuse(args):
    info = release_info(args.repo, args.version)
    names = payloads(args.version, [args.target])
    available = {a["name"] for a in (info or {}).get("assets", [])}
    if not set(names).issubset(available):
        write_outputs({"reused": "false"})
        return
    download_existing(args.repo, args.version, info, args.directory, [args.target])
    complete_checksums(args.directory, names)
    verify_assets(args.directory, args.version, [args.target])
    write_outputs({"reused": "true"})


def complete_checksums(directory, names):
    for name in names:
        checksum = directory / (name + ".sha256")
        if not checksum.exists():
            checksum.write_text(f"{digest(directory / name)}  {name}\n")


def stage_missing(local, remote, version):
    """Existing bytes are authoritative, including on a partially uploaded public release."""
    for name in payloads(version):
        destination = remote / name
        if not destination.exists():
            destination.write_bytes((local / name).read_bytes())
    complete_checksums(remote, payloads(version))
    # An orphan remote checksum is retained and must match; never silently replace it.
    verify_assets(remote, version)


def publish(args):
    verify_assets(args.directory, args.version)
    info = verify_tag(args.repo, args.version, args.sha, args.source)
    if info is None:
        command("gh", "release", "create", args.version, "--repo", args.repo,
                "--target", args.sha, "--title", args.version, "--notes-file", str(args.notes),
                "--draft", *(["--prerelease"] if args.prerelease else []))
        info = release_info(args.repo, args.version)
    if info["prerelease"] != args.prerelease:
        raise ValueError("Refusing to change an existing release's prerelease status")
    if not info["draft"] and args.draft:
        raise ValueError("Refusing to turn a public release back into a draft")
    with tempfile.TemporaryDirectory() as work:
        staged = Path(work) / "staged"
        existing = download_existing(args.repo, args.version, info, staged)
        stage_missing(args.directory, staged, args.version)
        missing = [str(staged / name) for name in expected_files(args.version) if name not in existing]
        if missing:
            # Never --clobber: successful uploads survive failed-job and whole-workflow retries.
            command("gh", "release", "upload", args.version, "--repo", args.repo, *missing)
        verified = Path(work) / "verified"
        download_existing(args.repo, args.version, release_info(args.repo, args.version), verified)
        verify_assets(verified, args.version)
        for name in expected_files(args.version):
            if digest(verified / name) != digest(staged / name):
                raise ValueError(f"Uploaded asset changed unexpectedly: {name}")
    if info["draft"] and not args.draft:
        # Detect an external tag change while artifacts were being uploaded.
        verify_tag(args.repo, args.version, args.sha, args.source)
        command("gh", "release", "edit", args.version, "--repo", args.repo, "--draft=false")


def check_public(args):
    sha = command("git", "rev-parse", "HEAD", cwd=args.source)
    info = verify_tag(args.repo, args.version, sha, args.source)
    if not info or info["draft"] or info["prerelease"]:
        raise ValueError("Package publication requires a public stable release")
    download_existing(args.repo, args.version, info, args.directory)
    verify_assets(args.directory, args.version)


def attach_file(repo, version, path):
    """Attach registry metadata once; retries must not overwrite public metadata."""
    info = release_info(repo, version)
    if any(asset["name"] == path.name for asset in info["assets"]):
        with tempfile.TemporaryDirectory() as work:
            command("gh", "release", "download", version, "--repo", repo,
                    "--pattern", path.name, "--dir", work)
            if json.loads((Path(work) / path.name).read_text()) != json.loads(path.read_text()):
                raise ValueError("Existing registry metadata differs; refusing to overwrite")
        return
    command("gh", "release", "upload", version, "--repo", repo, str(path))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operation", choices=["resolve", "reuse", "verify", "publish", "check-public"])
    parser.add_argument("--repo", default=os.environ.get("GITHUB_REPOSITORY"))
    parser.add_argument("--version", default="")
    parser.add_argument("--source", type=Path, default=Path("."))
    parser.add_argument("--directory", type=Path, default=Path("dist/release"))
    parser.add_argument("--target", choices=TARGETS)
    parser.add_argument("--sha")
    parser.add_argument("--notes", type=Path, default=Path("release-notes.md"))
    parser.add_argument("--draft", action="store_true")
    parser.add_argument("--prerelease", action="store_true")
    args = parser.parse_args()
    if args.version and not re.fullmatch(r"v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?", args.version):
        parser.error("Invalid release tag")
    if args.operation != "resolve" and not args.version:
        parser.error("--version is required")
    if args.operation != "verify" and not args.repo:
        parser.error("--repo or GITHUB_REPOSITORY is required")
    if args.operation == "publish" and not re.fullmatch(r"[a-f0-9]{40}", args.sha or ""):
        parser.error("--sha must be a full commit SHA")
    if args.operation == "reuse" and not args.target:
        parser.error("--target is required for reuse")
    operations = {"resolve": resolve, "reuse": reuse, "verify": lambda a: verify_assets(a.directory, a.version),
                  "publish": publish, "check-public": check_public}
    try:
        operations[args.operation](args)
    except (ValueError, RuntimeError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Release failed: {error}\n")


if __name__ == "__main__":
    main()
