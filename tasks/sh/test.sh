#!/bin/sh
set -eu

SCRIPT_DIR=$(cd "$(dirname "$0")" >/dev/null 2>&1 && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/../.." >/dev/null 2>&1 && pwd)

BUILD_MODE=${BUILD_MODE:-debug}

cd "$REPO_ROOT"

case "$BUILD_MODE" in
  debug)
    echo "+ cargo nextest run -p empack-lib --features test-utils -p empack-tests"
    cargo nextest run -p empack-lib --features test-utils -p empack-tests
    ;;
  profile|release)
    echo "+ cargo nextest run --release -p empack-lib --features test-utils -p empack-tests"
    cargo nextest run --release -p empack-lib --features test-utils -p empack-tests
    ;;
  *)
    echo "Unsupported BUILD_MODE: $BUILD_MODE" >&2
    echo "Expected one of: debug, profile, release" >&2
    exit 1
    ;;
esac