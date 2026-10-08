#!/usr/bin/env bash
set -euo pipefail
HERE="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
RUNTIME="$HERE/../.runtime/linux-onboarding"
mkdir -p "$RUNTIME/ssh" "$RUNTIME/tls"
# Disposable TLS certificate: the real import requires TLS for forwarded MongoDB.
openssl req -x509 -newkey rsa:2048 -nodes -sha256 -days 2 \
  -subj '/CN=mongodb' -addext 'subjectAltName=DNS:mongodb,DNS:localhost' \
  -keyout "$RUNTIME/tls/server.key" -out "$RUNTIME/tls/ca.crt" 2>/dev/null
cat "$RUNTIME/tls/server.key" "$RUNTIME/tls/ca.crt" > "$RUNTIME/tls/server.pem"
# MongoDB's non-root container user must be able to read this synthetic key.
chmod 644 "$RUNTIME/tls/server.pem" "$RUNTIME/tls/ca.crt"
chmod 600 "$RUNTIME/tls/server.key"
# Build the current Linux CLI from public source inputs only, not host configuration.
REPO="$(cd "$HERE/../.." && pwd)"
SOURCE="$RUNTIME/source"
mkdir -p "$SOURCE/sidecar/target" "$RUNTIME/build" "$RUNTIME/cargo"
cp "$REPO/Cargo.toml" "$REPO/Cargo.lock" "$REPO/build.rs" "$SOURCE/"
rm -rf "$SOURCE/src"
cp -R "$REPO/src" "$SOURCE/src"
cp "$REPO/sidecar/target/safeselect-sidecar.jar" "$SOURCE/sidecar/target/"
REVISION="$(git -C "$REPO" rev-parse --short HEAD)"
VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$REPO/Cargo.toml" | head -n 1)"
printf '%s\n' "$REVISION" > "$RUNTIME/source-revision.txt"
docker run --rm \
  -v "$SOURCE:/source:ro" -v "$RUNTIME/build:/build" -v "$RUNTIME/cargo:/cargo" \
  -e CARGO_HOME=/cargo -e CARGO_TARGET_DIR=/build \
  -e SAFESELECT_BUILD_VERSION="$VERSION-source-$REVISION" \
  -w /source rust:1-bookworm@sha256:59037199c44290f2befcdd58dcc540164763fc296950255aaefeef096a1866b0 \
  cargo build --locked
# Recreate only this disposable stack; no shared volumes or host ports.
docker compose -f "$HERE/compose.yaml" down
docker compose -f "$HERE/compose.yaml" up -d --build --wait
# Synthetic bastion credential only; never configure the host's SSH daemon.
printf 'demo:bastion-demo-password\n' \
  | docker compose -f "$HERE/compose.yaml" exec -T -u root bastion chpasswd
# Pin the key of this freshly created synthetic bastion, not an unverified remote host.
docker compose -f "$HERE/compose.yaml" exec -T bastion cat /etc/ssh/ssh_host_ed25519_key.pub \
  | awk '{print "bastion " $1 " " $2}' > "$RUNTIME/ssh/known_hosts"
docker cp "$RUNTIME/ssh/known_hosts" safeselect-linux-onboarding-terminal:/home/demo/.ssh/known_hosts
docker exec -u root safeselect-linux-onboarding-terminal chown demo:demo /home/demo/.ssh/known_hosts
docker cp "$RUNTIME/tls/ca.crt" safeselect-linux-onboarding-terminal:/tmp/demo-ca.crt
docker exec -u root safeselect-linux-onboarding-terminal keytool -importcert -noprompt \
  -alias safeselect-demo -file /tmp/demo-ca.crt \
  -keystore /etc/ssl/certs/java/cacerts -storepass changeit
docker exec safeselect-linux-onboarding-terminal mkdir -p /home/demo/.local/bin
docker cp "$RUNTIME/build/debug/safeselect" safeselect-linux-onboarding-terminal:/home/demo/.local/bin/safeselect
docker cp "$RUNTIME/source-revision.txt" safeselect-linux-onboarding-terminal:/home/demo/source-revision.txt
docker exec -u root safeselect-linux-onboarding-terminal chown demo:demo /home/demo/.local/bin/safeselect /home/demo/source-revision.txt
printf '\nLinux fixture ready. Run: vhs demo/linux-onboarding.tape\n'
