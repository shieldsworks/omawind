#!/usr/bin/env bash
# The one command that says a change is done.
#
#   scripts/verify.sh              every step
#   scripts/verify.sh lint test    only the named steps
#   VERIFY_SKIP=lint,test scripts/verify.sh
#
# Steps: lint test comments goldens qml clean
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

all_steps=(lint test comments goldens qml clean)
steps=("$@")
((${#steps[@]})) || steps=("${all_steps[@]}")
IFS=, read -r -a skip <<< "${VERIFY_SKIP:-}"

failed=()
say() { printf '\n== %s\n' "$*"; }
skipped() { local s; for s in "${skip[@]}"; do [[ $s == "$1" ]] && return 0; done; return 1; }

# Snapshot the tree first, so "clean" can tell files verification created
# from work in progress that was already there.
before=$(git status --porcelain --untracked-files=all)

step_lint() { mise lint; }
step_test() { mise test; }
step_comments() { scripts/check-comments.sh; }
step_goldens() { mise goldens; }

step_qml() {
  compgen -G 'ui/*.qml' >/dev/null || { echo 'No ui/*.qml. Skipped.'; return 0; }
  local q
  q=$(command -v qmllint || command -v qmllint6 || echo /usr/lib/qt6/bin/qmllint)
  [[ -x $q ]] || { echo 'qmllint not installed. CI runs it. Skipped.'; return 0; }
  # Ubuntu 24.04 ships Qt 6.4. Quickshell is not on that import path, so
  # import, type, property, and unqualified warnings are noise. Syntax still fails.
  # shellcheck disable=SC2086
  "$q" --import disable --type disable --property disable \
    --unqualified disable ${QMLLINT_FLAGS:-} ui/*.qml
}

step_clean() {
  local after
  after=$(git status --porcelain --untracked-files=all)
  if [[ $after != "$before" ]]; then
    printf 'Verification changed the tree. Tests must write to a temp dir:\n'
    diff <(printf '%s\n' "$before") <(printf '%s\n' "$after") | sed -n 's/^> /  /p'
    return 1
  fi
}

for s in "${steps[@]}"; do
  [[ " ${all_steps[*]} " == *" $s "* ]] || { printf 'Unknown step: %s\n' "$s" >&2; exit 2; }
  skipped "$s" && continue
  say "$s"
  "step_$s" || failed+=("$s")
done

if ((${#failed[@]})); then
  printf '\nFAILED: %s\n' "${failed[*]}" >&2
  exit 1
fi
printf '\nAll verification steps passed.\n'
