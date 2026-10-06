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
printf '\nLinux fixture ready. Run: vhs demo/linux-onboarding.tape\n'
