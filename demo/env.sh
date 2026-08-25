#!/usr/bin/env bash

# Source this file from the repository root before running SafeSelect commands.
# BASH_SOURCE is unavailable when sourced from zsh, which is the default shell
# on macOS, so obtain zsh's source path through its prompt-expansion syntax.
if [ -n "${BASH_SOURCE:-}" ]; then
  demo_source="${BASH_SOURCE[0]}"
elif [ -n "${ZSH_VERSION:-}" ]; then
  eval 'demo_source=${(%):-%x}'
else
  echo "Source demo/env.sh from bash or zsh." >&2
  return 1 2>/dev/null || exit 1
fi

DEMO_ROOT="$(CDPATH= cd -- "$(dirname "${demo_source}")" && pwd)"
export SAFESELECT_CONFIG_DIR="${DEMO_ROOT}/.runtime"
export SAFESELECT_DEMO_PASSWORD="demo-password"
