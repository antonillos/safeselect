# Linux onboarding recording

Reproducible Linux CLI demo using the **current source build**, not a released
archive or asdf installation. Preparation builds the selected checkout in an
isolated, digest-pinned Rust container and displays the source revision in the
recording. The macOS [Compass UX demo](../compass-import-ux/README.md) complements
this Linux connectivity scenario.

[Watch GIF](../../docs/recordings/linux-onboarding-envrefs.gif) · [Watch MP4](../../docs/recordings/linux-onboarding-envrefs.mp4)

1. Show Linux/Java and the current SafeSelect source revision.
2. Import a synthetic Compass export with database and password-authenticated SSH credentials.
3. Choose exported literal passwords and explicitly accept process-local storage for both.
4. Read and export `WORKSPACE_STAGING_DB_PASSWORD` and `STAGING_SSH_PASSWORD`
   silently in the launching shell: import-process values do not survive its exit.
5. Show password-free configuration and run `safeselect check --environment staging`.

This main demo imports once. Update/create/skip are covered by the separate
[reimport demo](../compass-import-ux/README.md), not repeated in this clip.

Run from the repository root with Docker, VHS, OpenSSL and the embedded sidecar JAR available:

```bash
./demo/linux-onboarding/prepare.sh
vhs demo/linux-onboarding.tape
```

Outputs (ignored by Git): `demo/recordings/linux-onboarding-envrefs.gif` and
`demo/recordings/linux-onboarding-envrefs.mp4`. After reviewing the rendered
flow and successful final check, publish the gallery copies:

```bash
cp demo/recordings/linux-onboarding-envrefs.gif docs/recordings/linux-onboarding-envrefs.gif
cp demo/recordings/linux-onboarding-envrefs.mp4 docs/recordings/linux-onboarding-envrefs.mp4
```

Preparation recreates **only** the `safeselect-linux-onboarding` stack. The
terminal is an Ubuntu 24.04 Linux container, not the host shell. Java 17 and
asdf 0.20.0 are prerequisites installed in its image; SafeSelect itself is
built from `Cargo.lock`, the Rust source and the existing embedded Java artifact.
No Java source is rebuilt; prepare that artifact with the repository's prescribed
makevn workflow if it is absent. Only these public build inputs are copied into
the build container; host credentials and project configuration are not mounted.
The terminal image supports Linux arm64 and amd64. Working outputs and caches
remain under ignored `demo/.runtime/linux-onboarding`.

The Compass-shaped JSON is synthetic, not captured from Compass. Both passwords
are disposable fixture values; they are never displayed or written to TOML.
Questions are light text and selected answers cyan. Long answers appear below
their questions, while short confirmations remain inline. The tape clears
inherited `NO_COLOR` and enables a high-contrast palette.

The bastion uses the synthetic `bastion-demo-password` with public-key
authentication disabled; its host key is pinned
from the fixture container during preparation. No personal keys or credentials
are mounted, and no database or SSH ports are published on the host. MongoDB
uses a disposable TLS certificate trusted only in the terminal container's Java
truststore; import TLS settings are not disabled.

The database credential belongs only to this disposable fixture. SafeSelect's
read-only policy remains enabled. This recording checks connectivity; it does
not replace the security integration suite or demonstrate a write rejection.

Validate the tape and shell syntax without recording:

```bash
bash -n demo/linux-onboarding/prepare.sh
vhs validate demo/linux-onboarding.tape
```

VHS 0.12.0 has an upstream render-context bug that can report success without
creating GIF/MP4 files. Use VHS 0.11.0 from a temporary installation,
without replacing the system version. Use a working VHS version and
check that both output files exist and are nonempty; tape validation alone does
not prove rendering succeeded. For an isolated installation with Go available:

```bash
GOBIN=/tmp/safeselect-vhs-tools go install github.com/charmbracelet/vhs@v0.11.0
/tmp/safeselect-vhs-tools/vhs demo/linux-onboarding.tape
test -s demo/recordings/linux-onboarding-envrefs.gif
test -s demo/recordings/linux-onboarding-envrefs.mp4
```

Stop and remove this stack when finished:

```bash
docker compose -f demo/linux-onboarding/compose.yaml down
```

To repeat the recording, rerun preparation first: it resets the isolated
terminal's asdf install and project, without touching the user's configuration.
