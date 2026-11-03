#!/usr/bin/env bash
# The bans clippy cannot express (uo-jsnum plan, "Enforcement"): Rust's float parser accepts
# `inf`/`nan` and rejects white space, so strings go through uo_jsnum::parse; and of
# core::f64::consts only the six that exist in JavaScript may be used (TAU, FRAC_PI_3, LOG2_E...
# are correctly rounded values where the TS computes an expression such as Math.PI / 3).
set -euo pipefail
cd "$(dirname "$0")/.."
status=0
files=$(find crates fuzz -name '*.rs' -not -path '*/target/*' -not -path 'crates/uo-jsnum/*' -not -path 'crates/uo-testkit/*')
if parse=$(grep -nE 'parse::<f(32|64)>|f(32|64)::from_str|\.parse\(\)' $files | grep -E 'f(32|64)'); then
  echo "float parsing outside uo-jsnum (use uo_jsnum::parse):"; echo "$parse"; status=1
fi
if consts=$(grep -nE '(f(32|64)::consts::|consts::)(TAU|FRAC_PI_[0-9]+|FRAC_1_PI|FRAC_2_PI|FRAC_2_SQRT_PI|LOG2_E|LOG10_E|LOG2_10|LOG10_2)|f(32|64)::(consts::)?(TAU|FRAC_PI_[0-9]+)' $files); then
  echo "core::f64::consts other than PI, E, LN_2, LN_10, SQRT_2, FRAC_1_SQRT_2:"; echo "$consts"; status=1
fi
exit $status
