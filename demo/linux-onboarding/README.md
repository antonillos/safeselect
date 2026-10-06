# Linux onboarding recording

Reproducible Linux CLI demo using a public SafeSelect release that includes
[PR #279](https://github.com/antonillos/safeselect/pull/279). **Version 0.7.10 does
not support this tape; wait for the feature release before recording.**

1. Install SafeSelect through the public asdf plugin (archive SHA-256 verified).
2. Interactively import a synthetic MongoDB Compass export with a password-authenticated SSH bastion.
3. Read both database and SSH passwords silently and export `DB_PASSWORD` and `BASTION_PASSWORD`.
4. Show the password-free environment TOML and run `safeselect check --environment staging`.

Run from the repository root with Docker, VHS and OpenSSL available:

```bash
./demo/linux-onboarding/prepare.sh
vhs demo/linux-onboarding.tape
```

Outputs (ignored by Git): `demo/recordings/linux-onboarding-envrefs.gif` and
`demo/recordings/linux-onboarding-envrefs.mp4`.

Preparation recreates **only** the `safeselect-linux-onboarding` stack. The
terminal is an Ubuntu 24.04 Linux container, not the host shell. Java 17 and
asdf 0.20.0 are prerequisites installed in its image; SafeSelect itself is
installed visibly in the recording. No source build or development binary is
used. The image supports Linux arm64 and amd64.

The Compass-shaped JSON is a synthetic export, not a capture from the Compass
GUI. It deliberately omits the database password, just as an export may do.
The database uses the synthetic `demo-password`, entered without terminal echo.
On Linux, SafeSelect saves a project-scoped environment-variable reference,
not the password. The tape selects `{env:BASTION_PASSWORD}` during import and
uses `config set-password --password '{env:DB_PASSWORD}'` afterward. No Python
code or TOML parsing is shown or required. Both variables must be exported in the same shell that launches
SafeSelect. The bastion uses the synthetic `bastion-demo-password` with public-key
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
