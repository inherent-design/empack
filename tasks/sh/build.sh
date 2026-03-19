#!/bin/sh
set -eu

SCRIPT_DIR=$(cd "$(dirname "$0")" >/dev/null 2>&1 && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/../.." >/dev/null 2>&1 && pwd)

BUILD_MODE=${BUILD_MODE:-debug}
RUN_AFTER_BUILD=${RUN_AFTER_BUILD:-0}

cd "$REPO_ROOT"

case "$BUILD_MODE" in
  debug)
    BIN_PATH=target/debug/empack
    LOG_LEVEL=3
    echo "+ cargo build"
    cargo build
    ;;
  profile)
    BIN_PATH=target/release/empack
    LOG_LEVEL=2
    echo "+ cargo build --release"
    cargo build --release
    ;;
  release)
    BIN_PATH=target/release/empack
    LOG_LEVEL=0
    echo "+ cargo build --release"
    cargo build --release
    ;;
  *)
    echo "Unsupported BUILD_MODE: $BUILD_MODE" >&2
    echo "Expected one of: debug, profile, release" >&2
    exit 1
    ;;
esac

case "$RUN_AFTER_BUILD" in
  0)
    ;;
  1)
    export EMPACK_LOG_LEVEL=$LOG_LEVEL
    echo "+ EMPACK_LOG_LEVEL=$EMPACK_LOG_LEVEL exec $BIN_PATH"
    exec "$BIN_PATH" "$@"
    ;;
  *)
    echo "Unsupported RUN_AFTER_BUILD: $RUN_AFTER_BUILD" >&2
    echo "Expected one of: 0, 1" >&2
    exit 1
    ;;
esac