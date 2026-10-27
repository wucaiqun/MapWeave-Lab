#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build -p native-viewer
exec gdb -ex "break main" -ex run target/debug/native-viewer
