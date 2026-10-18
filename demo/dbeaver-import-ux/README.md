# DBeaver import UX

Synthetic, offline **macOS current-source** recording, not the public release.
The tape imports one DBeaver ZIP, configures a password-authenticated bastion,
enters a hidden synthetic SSH password, and uses the database password from the
export. Both passwords use process-local variables; the tape never selects
Keychain storage or performs a connectivity check.

The final TOML contains only password references (`ATLAS_STAGING_DB_PASSWORD`
and `STAGING_SSH_PASSWORD`). Session-only values expire with the import;
future commands need variables exported by the launching shell. All endpoints
use `.invalid`, and the fixture contains no real credentials or personal data.

From the repository root:

```bash
cargo build
./demo/dbeaver-import-ux/prepare.sh
vhs validate demo/dbeaver-import-ux.tape
vhs demo/dbeaver-import-ux.tape
```

Preparation creates a `.gitignore` containing `.safeselect/` in the temporary
project, preventing an unrelated setup warning during the import. The final
configuration is displayed on a fresh terminal screen for readability.

Preparation resets only `/tmp/safeselect-dbeaver-ux` and refuses a symlink at
that location. It does not read a real DBeaver workspace or alter global runtime
configuration. The tape requires macOS because its password destination selector
includes Keychain, even though the selected destination is session-only.
VHS needs permission to use local sockets and its Chromium renderer.
VHS 0.12.0 can report success without producing files; use a temporary VHS 0.11.0
installation as described in the [Linux recording instructions](../linux-onboarding/README.md).
After rendering, check that both GIF and MP4 exist and are nonempty.

Visually inspect the MP4 and final frames before publishing:

```bash
cp demo/recordings/dbeaver-import-ux.gif docs/recordings/dbeaver-import-ux.gif
cp demo/recordings/dbeaver-import-ux.mp4 docs/recordings/dbeaver-import-ux.mp4
```

The source tape and fixture are versioned; generated working recordings are
ignored. Snapshot text can omit later interactions, so inspect the actual video.
