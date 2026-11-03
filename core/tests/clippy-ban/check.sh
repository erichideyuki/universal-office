#!/usr/bin/env bash
# Asserts that every path of the `disallowed-methods` list in core/clippy.toml resolves: clippy
# must report one `clippy::disallowed_methods` diagnostic per entry on this crate (see Cargo.toml).
set -euo pipefail
cd "$(dirname "$0")"
expected=$(grep -c '^ *{ path = ' ../../clippy.toml)
actual=$(cargo clippy --message-format=json 2>/dev/null | grep -c '"code":{"code":"clippy::disallowed_methods"' || true)
echo "disallowed_methods: $actual diagnostics for $expected entries"
test "$actual" -eq "$expected"
