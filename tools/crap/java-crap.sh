#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(CDPATH= cd -- "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT_DIR="${ROOT_DIR}/target/crap"
CRAP4JAVA_JAR="${CRAP4JAVA_JAR:-${ROOT_DIR}/../crap4java/target/crap4java-0.1.0-SNAPSHOT.jar}"

command -v makevn >/dev/null 2>&1 || { printf 'Error: makevn is required.\n' >&2; exit 2; }
command -v java >/dev/null 2>&1 || { printf 'Error: java is required.\n' >&2; exit 2; }
command -v jq >/dev/null 2>&1 || { printf 'Error: jq is required.\n' >&2; exit 2; }
if [[ ! -f "${CRAP4JAVA_JAR}" ]]; then
  printf 'Error: crap4java JAR not found: %s\n' "${CRAP4JAVA_JAR}" >&2
  exit 2
fi

mkdir -p "${OUT_DIR}"
makevn clean verify-ut-coverage --compact >"${OUT_DIR}/makevn.log" 2>&1
makevn exec -- java -jar "${CRAP4JAVA_JAR}" \
  --format json \
  --jacoco-xml "${ROOT_DIR}/sidecar/target/site/jacoco/jacoco.xml" \
  --report-only \
  >"${OUT_DIR}/java-crap.raw.log" 2>&1

awk '/^\{/{started=1} started {print} /^\}$/{exit}' \
  "${OUT_DIR}/java-crap.raw.log" >"${OUT_DIR}/java-report.json"
jq -e '.entries and .summary' "${OUT_DIR}/java-report.json" >/dev/null
