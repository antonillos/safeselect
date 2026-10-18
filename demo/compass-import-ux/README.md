# Compass credential and reimport UX

Synthetic, offline **macOS source-build** recording. It is not the older
Linux public-release onboarding demo and does not claim database connectivity.

The primary tape performs **one import**, reusing both passwords from a
Compass-shaped fixture into process-local variables (`ATLAS_STAGING_DB_PASSWORD`
and `STAGING_SSH_PASSWORD`). It explicitly declines Keychain storage and
connectivity checks, then shows the password-free configuration.

A separate [reimport recording](../../docs/recordings/compass-reimport.gif)
([MP4](../../docs/recordings/compass-reimport.mp4)) starts with a seeded synthetic
import. It demonstrates update/create/skip, retaining existing sources and
hidden direct password entry when creating a copy. Reproduce it independently:

```bash
./demo/compass-import-ux/prepare-reimport.sh
vhs validate demo/compass-reimport.tape
vhs demo/compass-reimport.tape
cp demo/recordings/compass-reimport.gif docs/recordings/compass-reimport.gif
cp demo/recordings/compass-reimport.mp4 docs/recordings/compass-reimport.mp4
```

Its isolated runtime is `/tmp/safeselect-compass-reimport`, separate from the
main tape, so neither recording depends on running the other.

### Multiple environments in one import

The `compass-environments.tape` clip updates synthetic `pre` and `pro`
connections in a single import, keeping their existing password variable
references. It pauses at the boundary between `pre`'s password-source summary
and `pro`'s connection heading, existing-environment label and update prompts.
No credentials are entered and connectivity checks are declined.

```bash
cargo build
bash demo/compass-import-ux/prepare-environments.sh
vhs validate demo/compass-environments.tape
vhs demo/compass-environments.tape
```

The isolated runtime is `/tmp/safeselect-compass-environments`. Generated GIF,
MP4 and terminal snapshots stay under `demo/recordings` and `demo/.runtime`;
inspect the video before publishing gallery copies. The existing Compass and
Linux onboarding tapes also pause at each connection heading before answering.

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
Working recordings under `demo/recordings` are ignored by Git. After visual
inspection, publish the gallery copies explicitly:

```bash
cp demo/recordings/compass-import-ux.gif docs/recordings/compass-import-ux.gif
cp demo/recordings/compass-import-ux.mp4 docs/recordings/compass-import-ux.mp4
```

The tape explicitly removes inherited `NO_COLOR` and enables a color-capable
terminal. Questions remain light text; selected options and confirmed answers
are cyan, with distinct `?` (active) and `>` (answered) prefixes. Larger text and one-second interaction
pauses make both stages easier to follow. Long selected answers appear below
their questions; short confirmations stay inline. Password entry remains hidden.

Runtime is isolated at `/tmp/safeselect-compass-ux`; preparation resets only that
fixed directory. The fixture uses `.invalid` endpoints and synthetic credentials.
The tape must run on macOS because its destination selector includes Keychain;
it never selects that destination and does not create or modify Keychain entries.
