#!/bin/sh
set -eu

SCRIPT_DIR=$(cd "$(dirname "$0")" >/dev/null 2>&1 && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/../.." >/dev/null 2>&1 && pwd)

cd "$REPO_ROOT"

echo "+ cargo build --release -p empack"
cargo build --release -p empack

export EMPACK_E2E_BIN="$REPO_ROOT/target/release/empack"

FILTER="${1:-e2e_}"

echo "+ cargo nextest run -p empack-tests -E 'test(~$FILTER)'"
cargo nextest run -p empack-tests -E "test(~$FILTER)"
