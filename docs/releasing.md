# Release pipeline and recovery

## Normal flow

1. Merge feature PRs into `develop`, then promote `develop` into `main`.
2. Run **Prepare Release** from `main` and review its version/changelog PR.
3. Merge the release PR. **Release** validates the version and freezes the source
   commit SHA. An existing tag must resolve to that exact commit.
4. Integration tests and all four platform builds must pass. Builds upload only
   temporary **Actions artifacts**, not public release assets.
5. The publication job checks every archive, MCPB and SHA-256 file. It creates a
   draft, uploads missing files, downloads and verifies the complete remote set,
   then publishes. A failed upload leaves a draft, not a public partial release.
6. Homebrew/asdf and MCP Registry run only after successful publication. An
   explicitly requested draft or prerelease is not distributed to these channels.

The Release workflow is serialised with `cancel-in-progress: false`. Package
manager publication has its own serialisation shared by automatic and manual
invocations. Recovery must not downgrade an already newer Homebrew formula.

## Reproducible build-tool installation

All workflows use the local `setup-makevn` composite action. It selects the
**runner's** architecture (not the Rust cross-compilation target), downloads the
version in `tools/ci/makevn.lock.json`, and verifies its locked SHA-256 **before**
extraction or execution. Downloads have bounded retries and timeouts.

There is no `latest` API lookup, no remote `curl | sh`, and no new API key or PAT.
The installer is reviewed as repository code; the lock also records the upstream
source commit. To upgrade, review that release and update the version, source
commit and all three archive digests together. Run the installer tests and a real
installation before merging the change. Installation uses a fresh job-local
directory and does not overwrite a developer's existing installation.

## Credentials

- `GITHUB_TOKEN`: automatic, scoped to the repository; read access in builds,
  write access only where publishing requires it.
- `SAFESELECT_RELEASE_TOKEN`: existing cross-repository credential used for
  Homebrew/asdf. Grant only the required repositories and write permissions.
  Missing credentials or denied writes fail the publication job, not silently
  succeed. Nothing about the makevn fix requires rotating this token.
- MCP Registry authentication continues to use GitHub OIDC.

## Recovering a failed release

Prefer **Re-run failed jobs** on the original Actions run. This retains successful
builds and only repeats failed jobs and their dependants:

```bash
gh run rerun <run-id> --failed
```

If a new invocation is needed (for example, after a workflow fix), run the current
workflow but select the original tag/commit as the source:

```bash
gh workflow run release.yml --ref main \
  -f version=v0.7.6 -f target_ref=v0.7.6
```

Use the release's actual prerelease/draft settings when applicable. The current
workflow tooling is checked out separately from the source being released, so a
tag predating these helper scripts can still be recovered.

Recovery does **not** delete a release, move a tag, overwrite assets or stop merely
because a release already exists. Complete platform assets are verified and
reused. Missing files are uploaded; missing checksum files are computed from the
existing payloads. Existing checksums/digests must match. An orphan checksum that
does not match a rebuilt payload, an unfinished upload or a changed tag requires
investigation, not automatic replacement.

After uploads the complete remote set is verified again. Package managers can
then be retried even for an already public release. MCP Registry checks the exact
version and skips duplicate publication only if its metadata matches; attaching
`server.json` is also idempotent and never overwrites different metadata.

For package-manager-only recovery:

```bash
gh workflow run publish-package-managers.yml --ref main -f version=v0.7.6
```

This uses the same verification and publisher as Release. It rejects draft,
prerelease, missing/corrupt assets and a source commit inconsistent with the tag.

## Validation

```bash
python3 -m unittest discover -s tools/ci -p 'test_*.py' -v
bash -n tools/ci/install-makevn.sh
go run github.com/rhysd/actionlint/cmd/actionlint@v1.7.12 -shellcheck= -pyflakes=
```

Tests cover first publication, failed/partial uploads, public release recovery,
immutable existing payloads, tag mismatch, corrupt checksums, API 403 versus 404,
bounded transient retries, registry idempotency, Homebrew downgrade prevention,
installer corruption/host selection and the workflow dependency graph. Verify
runs these tests in addition to the existing Rust/Java/CRAP checks; their thresholds
are unchanged.
