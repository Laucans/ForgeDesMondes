#!/usr/bin/env bash
# Runs the `design-system-single-writer` step of .github/workflows/gates.yml
# against scratch git repositories — no network, nothing outside a temp dir.
#
#   bash scripts/test-design-system-gate.sh
#
# The step's `run:` body is read out of gates.yml itself, so what is tested is
# what the CI runs (GATES_YML points it at another copy). Exit status: 0 when every case behaves, 1 otherwise.
set -uo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
gates=${GATES_YML:-$root/.github/workflows/gates.yml}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

step="$work/step.sh"
awk '
  /^  design-system-single-writer:/ { job = 1; next }
  job && /^  [a-z#]/ { job = 0 }
  job && /^        run: \|$/ { body = 1; next }
  body && /^          / { sub(/^          /, ""); print; next }
  body && /^$/ { print; next }
  body { body = 0 }
' "$gates" > "$step"
[ "$(head -n 1 "$step")" = "set -euo pipefail" ] || { echo "cannot read the step out of gates.yml"; exit 1; }

failures=0
check() { # name, expected exit, actual exit
  if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1 (expected exit $2, got $3)"; failures=$((failures + 1)); fi
}

export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null

# A repo whose main has mock/{design,kit,designer,kitchen,pages}; prints nothing.
scratch() {
  rm -rf "$work/repo"; git init -q -b main "$work/repo"; cd "$work/repo" || exit 1
  mkdir -p mock/design mock/kit mock/designer mock/kitchen mock/pages
  for f in mock/design/tokens.css mock/kit/Button.html mock/designer/x mock/kitchen/x mock/pages/a.html mock/style.css; do echo 1 > "$f"; done
  git add -A; git commit -q -m base
  base_sha=$(git rev-parse HEAD)
  git checkout -q -b pr
}
commit() { git add -A; git commit -q -m change; head_sha=$(git rev-parse HEAD); }

# run <head ref> <head repo> [expected annotations]; sets $out and $code.
run() {
  out=$(HEAD_REF=$1 HEAD_REPO=$2 BASE_REPO=o/r BASE_REF=main BASE_SHA=${base_sha:-} HEAD_SHA=${head_sha:-} bash "$step" 2>&1)
  code=$?
  annotations=$(printf '%s\n' "$out" | grep -c '^::error file=' || true)
}

# --- the four cases of the issue -------------------------------------------
scratch; echo 2 > mock/kit/Button.html; commit
run feature/x o/r; check "edit under mock/kit/ fails" 1 $code
check "  ... one annotation" 1 "$annotations"
check "  ... names the rule" 1 "$(printf '%s\n' "$out" | grep -c 'mock/design/ and mock/kit/ are read-only outside the design-system branch')"
scratch; echo 2 > mock/design/tokens.css; commit
run feature/x o/r; check "edit under mock/design/ fails" 1 $code
scratch; echo 2 > mock/style.css; commit
run feature/x o/r; check "edit outside both passes" 0 $code
scratch; echo 2 > mock/design/tokens.css; echo 2 > mock/style.css; commit
run design-system o/r; check "design-system, same repository passes" 0 $code

# --- who is exempt ----------------------------------------------------------
scratch; echo 2 > mock/kit/Button.html; commit
for ref in design-system-2 feature/design-system Design-System design-system/x ''; do
  run "$ref" o/r; check "head '$ref' fails" 1 $code
done
run design-system fork/r; check "fork branch named design-system fails" 1 $code
run design-system ''; check "unknown head repository fails" 1 $code

# --- every kind of change, every offender once -------------------------------
scratch; echo 2 > mock/kit/Button.html; rm mock/design/tokens.css; echo n > mock/kit/New.html; echo 2 > mock/style.css; echo 2 > mock/designer/x; echo 2 > mock/kitchen/x; commit
run feature/x o/r; check "3 protected paths, 3 annotations" 3 "$annotations"
check "  ... none for unprotected files" 0 "$(printf '%s\n' "$out" | grep -c 'file=mock/\(style\|designer\|kitchen\|pages\)')"
scratch; chmod +x mock/kit/Button.html; commit
run feature/x o/r; check "mode change fails" 1 $code
scratch; mkdir mock/other; git mv mock/kit/Button.html mock/other/Button.html; commit
run feature/x o/r; check "rename out of mock/kit/ fails" 1 $code
check "  ... annotates the old side" 1 "$(printf '%s\n' "$out" | grep -c 'file=mock/kit/Button.html::')"
scratch; git mv mock/kit/Button.html mock/design/Button.html; commit
run feature/x o/r; check "rename between protected folders: one annotation per side" 2 "$annotations"
scratch; git rm -rq mock/kit; git commit -qm "no kit"; base_sha=$(git rev-parse HEAD); echo x > mock/kit; commit
run feature/x o/r; check "a file put where mock/kit/ was not fails" 1 $code
scratch; echo 2 > mock/designer/x; echo 2 > mock/kitchen/x; echo 2 > mock/pages/a.html; echo 2 > mock/README.md; echo 2 > mock/design.md; commit
run feature/x o/r; check "look-alike paths pass" 0 $code
scratch; echo n > "mock/kit/we,ird:%name.html"; commit
run feature/x o/r; check "an odd file name stays one escaped annotation" 1 "$annotations"
check "  ... escaped" 1 "$(printf '%s\n' "$out" | grep -c 'file=mock/kit/we%2Cird%3A%25name.html::')"
scratch; head_sha=$base_sha
run feature/x o/r; check "empty diff passes" 0 $code

# --- the merge base ------------------------------------------------------------
scratch; echo 2 > mock/style.css; commit
git checkout -q main; echo 2 > mock/kit/Button.html; git commit -qam "base moves"; moved=$(git rev-parse HEAD)
base_sha=$moved
run feature/x o/r; check "a kit commit on the base after the branch point does not count" 0 $code

# --- fail closed ------------------------------------------------------------------
scratch; echo 2 > mock/kit/Button.html; commit
good_base=$base_sha
base_sha=; run feature/x o/r; check "no base sha fails" 1 $code
check "  ... with an ::error" 1 "$(printf '%s\n' "$out" | grep -c '^::error::')"
base_sha=0000000000000000000000000000000000000001; run feature/x o/r; check "unresolvable base fails" 1 $code
base_sha=$good_base
git -C "$work/repo" checkout -q --orphan lone && git -C "$work/repo" commit -q --allow-empty -m lone
base_sha=$(git rev-parse HEAD); run feature/x o/r; check "no merge base fails" 1 $code
rm -rf "$work/clone"; git clone -q --depth 1 "file://$work/repo" "$work/clone" 2>/dev/null; cd "$work/clone" || exit 1
base_sha=$(git rev-parse HEAD); head_sha=$base_sha; run feature/x o/r; check "shallow history fails" 1 $code
cd "$work/repo" && git checkout -q pr 2>/dev/null; base_sha=$good_base

# --- inputs are data ----------------------------------------------------------------
scratch; echo 2 > mock/style.css; commit
run 'x$(touch pwned);echo' o/r; check "metacharacters in the branch name: verdict unchanged" 0 $code
check "  ... nothing executed" 0 "$([ -e pwned ] && echo 1 || echo 0)"
scratch; echo 2 > mock/kit/Button.html; commit
run 'x$(touch pwned);echo' o/r; check "metacharacters in the branch name, protected change still fails" 1 $code
check "  ... nothing executed" 0 "$([ -e pwned ] && echo 1 || echo 0)"
check "the step body has no \${{ }}" 0 "$(grep -c '\${{' "$step")"

# --- the job around the step -----------------------------------------------------
job=$(awk '/^  design-system-single-writer:/ {j=1; print; next} j && /^  [a-z#]/ {exit} j' "$gates")
check "job runs on pull_request only" 1 "$(printf '%s\n' "$job" | grep -c "^    if: github.event_name == 'pull_request'\$")"
check "job carries no if: false" 0 "$(printf '%s\n' "$job" | grep -c 'if: false')"
check "history is fetched whole" 1 "$(printf '%s\n' "$job" | grep -c 'fetch-depth: 0')"

[ "$failures" -eq 0 ] && echo "all cases behave" || echo "$failures case(s) misbehave"
[ "$failures" -eq 0 ]
