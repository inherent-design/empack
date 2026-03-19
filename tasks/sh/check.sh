#!/bin/sh
set -eu

SCRIPT_DIR=$(cd "$(dirname "$0")" >/dev/null 2>&1 && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/../.." >/dev/null 2>&1 && pwd)

CHECK_MODE=${CHECK_MODE:-check}

cd "$REPO_ROOT"

case "$CHECK_MODE" in
  check)
    echo "+ cargo check --workspace --all-targets"
    cargo check --workspace --all-targets
    ;;
  clippy)
    echo "+ cargo clippy --workspace --all-targets"
    cargo clippy --workspace --all-targets
    ;;
  *)
    echo "Unsupported CHECK_MODE: $CHECK_MODE" >&2
    echo "Expected one of: check, clippy" >&2
    exit 1
    ;;
esac