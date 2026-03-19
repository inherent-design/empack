#!/bin/sh
set -eu

SCRIPT_DIR=$(cd "$(dirname "$0")" >/dev/null 2>&1 && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/../.." >/dev/null 2>&1 && pwd)

BUILD_MODE=${BUILD_MODE:-debug}

cd "$REPO_ROOT"

case "$BUILD_MODE" in
  debug)
    echo "+ cargo llvm-cov nextest --workspace --features test-utils --lcov --output-path lcov.info"
    cargo llvm-cov nextest --workspace --features test-utils --lcov --output-path lcov.info
    ;;
  profile|release)
    echo "+ cargo llvm-cov nextest --release --workspace --features test-utils --lcov --output-path lcov.info"
    cargo llvm-cov nextest --release --workspace --features test-utils --lcov --output-path lcov.info
    ;;
  *)
    echo "Unsupported BUILD_MODE: $BUILD_MODE" >&2
    echo "Expected one of: debug, profile, release" >&2
    exit 1
    ;;
esac
