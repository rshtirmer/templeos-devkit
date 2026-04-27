#!/usr/bin/env bash
# Smoke runner for the holyc-lint rule corpus.
#
# For each rule we maintain a paired bad-<rule>.HC / good-<rule>.HC file:
#   - bad-<rule>.HC must trigger that rule (grep the rule name in output)
#   - good-<rule>.HC must produce zero error-level diagnostics
#
# Exit non-zero on any expectation miss.

set -u
cd "$(dirname "$0")/../.."

LINT="scripts/holyc-lint.py"
RULES=(
  parametrized-define
  exponent-float-literal
  comma-decl-list
  multi-array-decl
  f32-reference
)

fail=0

for rule in "${RULES[@]}"; do
  bad="tests/lint/bad-${rule}.HC"
  good="tests/lint/good-${rule}.HC"

  if [[ ! -f "$bad" || ! -f "$good" ]]; then
    echo "MISSING: $bad or $good" >&2
    fail=1
    continue
  fi

  out=$(NO_COLOR=1 python3 "$LINT" "$bad" 2>&1 || true)
  if ! grep -q "\[${rule}\]" <<<"$out"; then
    echo "FAIL: $bad did not emit [$rule]" >&2
    echo "$out" >&2
    fail=1
  else
    echo "ok: $bad triggered [$rule]"
  fi

  out=$(NO_COLOR=1 python3 "$LINT" "$good" 2>&1 || true)
  # Count error-level lines only; warnings are tolerated for some rules
  # (parametrized-define is warning-level, but good file has none).
  if grep -E ": error: " <<<"$out" | grep -q "\[${rule}\]"; then
    echo "FAIL: $good triggered [$rule] (should be clean)" >&2
    echo "$out" >&2
    fail=1
  elif grep -q "\[${rule}\]" <<<"$out"; then
    echo "FAIL: $good emitted [$rule] diagnostic (should be clean)" >&2
    echo "$out" >&2
    fail=1
  else
    echo "ok: $good clean of [$rule]"
  fi
done

# Bonus: confirm existing tree still passes.
if ! python3 "$LINT" src tests/Assert.ZC tests/T_Hello.ZC >/dev/null 2>&1; then
  echo "FAIL: existing src/tests no longer lint-clean" >&2
  fail=1
else
  echo "ok: existing tree still lint-clean"
fi

if [[ $fail -ne 0 ]]; then
  echo "lint smoke: FAILED" >&2
  exit 1
fi
echo "lint smoke: OK"
