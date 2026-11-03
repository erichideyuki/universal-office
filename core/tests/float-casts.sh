#!/usr/bin/env bash
# Float -> integer `as` casts saturate and map NaN to 0 where JS wraps (`x | 0`, `>>> 0`): they
# go through uo_jsnum::conv or a named helper. Clippy's cast lints also fire on every int -> int
# cast (80 hits in uo-opc --lib alone), so this runs them with the JSON output and fails only on
# diagnostics whose source type is f32 or f64. Library code of the engine crates only (uo-jsnum
# holds the conversions; uo-testkit and uo-alloc are tooling; tests and examples may cast
# timings).
set -euo pipefail
cd "$(dirname "$0")/.."
status=0
for crate in $(ls crates | grep -vE '^(uo-jsnum|uo-testkit|uo-alloc)$'); do
  touch "crates/$crate/src/lib.rs"
  hits=$(cargo clippy -p "$crate" --lib --no-deps --message-format=json -- -W clippy::cast_possible_truncation -W clippy::cast_sign_loss 2>/dev/null \
    | grep -E '"message":"casting `f(32|64)` to' || true)
  if [ -n "$hits" ]; then
    echo "float -> int casts in $crate (use uo_jsnum::conv):"
    echo "$hits" | grep -o '"rendered":"[^"]*"' | head -20
    status=1
  fi
done
exit $status
