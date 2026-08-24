#!/usr/bin/env bash
# Create a platform-specific SafeSelect MCP Bundle from a release binary.
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/package-mcpb.sh --binary PATH --target TARGET --version VERSION --output PATH

Packages one SafeSelect binary into an MCPB archive. The generated manifest
asks the client for the project directory and SafeSelect environment rather
than embedding a database connection or credentials in the package.
EOF
}

binary=""
target=""
version=""
output=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --binary) binary="${2:-}"; shift 2 ;;
    --target) target="${2:-}"; shift 2 ;;
    --version) version="${2:-}"; shift 2 ;;
    --output) output="${2:-}"; shift 2 ;;
    --help|-h) usage; exit 0 ;;
    *) printf 'Error: unknown argument: %s\n' "$1" >&2; usage >&2; exit 2 ;;
  esac
done

if [[ -z "$binary" || -z "$target" || -z "$version" || -z "$output" ]]; then
  printf 'Error: --binary, --target, --version, and --output are required.\n' >&2
  usage >&2
  exit 2
fi
if [[ ! -f "$binary" ]]; then
  printf 'Error: release binary does not exist: %s\n' "$binary" >&2
  exit 1
fi
case "$target" in
  *-apple-darwin) platform="darwin" ;;
  *-unknown-linux-gnu) platform="linux" ;;
  *) printf 'Error: unsupported MCPB target: %s\n' "$target" >&2; exit 1 ;;
esac
case "$version" in v*) version="${version#v}" ;; esac
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]]; then
  printf 'Error: version must be semver, got: %s\n' "$version" >&2
  exit 1
fi

repo_root="$(CDPATH= cd -- "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
template="$repo_root/packaging/mcpb/manifest.json.tmpl"
if [[ ! -f "$template" ]]; then
  printf 'Error: MCPB manifest template is missing: %s\n' "$template" >&2
  exit 1
fi
work_dir="$(mktemp -d)"
cleanup() { rm -rf "$work_dir"; }
trap cleanup EXIT

mkdir -p "$work_dir/server" "$(dirname "$output")"
cp "$binary" "$work_dir/server/safeselect"
chmod 0755 "$work_dir/server/safeselect"
sed -e "s/__VERSION__/$version/g" -e "s/__PLATFORM__/$platform/g" "$template" > "$work_dir/manifest.json"
python3 -m json.tool "$work_dir/manifest.json" >/dev/null
rm -f "$output"
(cd "$work_dir" && zip -q -r "$output" manifest.json server)
printf 'Created MCPB: %s\n' "$output"
