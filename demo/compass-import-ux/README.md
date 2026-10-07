# Compass credential and reimport UX

Synthetic, offline **macOS source-build** recording. It is not the older
Linux public-release onboarding demo and does not claim database connectivity.

The tape imports both passwords from a Compass-shaped fixture into process-local
variables (`ATLAS_STAGING_DB_PASSWORD` and `ATLAS_STAGING_SSH_PASSWORD`),
explicitly declines Keychain storage and connectivity checks, updates the same
environment without duplicating it, creates a separate environment with hidden
direct password entry, and demonstrates skipping a repeated import.
No password is displayed or written to the generated TOML. Process-local values
expire when each import exits; they are not exported to the parent terminal.

From the repository root:

```bash
cargo build
./demo/compass-import-ux/prepare.sh
vhs validate demo/compass-import-ux.tape
vhs demo/compass-import-ux.tape
```

Use VHS 0.11.0 if the installed 0.12.0 cannot render, as described in the
[Linux recording instructions](../linux-onboarding/README.md).
Verify the GIF and MP4 are nonempty and visually inspect the final frames. VHS
also writes snapshots to `demo/.runtime/compass-import-ux-screen.txt`, but that
text output may omit later interactions; the rendered video is the visual evidence.
Generated files are ignored by Git.

The tape explicitly removes inherited `NO_COLOR` and enables a color-capable
terminal. Questions remain light text; selected options and confirmed answers
are cyan, with distinct `?` (active) and `>` (answered) prefixes. An introductory
Spanish legend explains the contrast. Larger text and one-second interaction
pauses make both stages easier to follow. Password entry remains hidden.

Runtime is isolated at `/tmp/safeselect-compass-ux`; preparation resets only that
fixed directory. The fixture uses `.invalid` endpoints and synthetic credentials.
The tape must run on macOS because its destination selector includes Keychain;
it never selects that destination and does not create or modify Keychain entries.
