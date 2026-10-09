#!/usr/bin/env bash
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

all_steps=(lint test comments goldens qml clean)
steps=("$@")
((${#steps[@]})) || steps=("${all_steps[@]}")
IFS=, read -r -a skip <<< "${VERIFY_SKIP:-}"

failed=()
say() { printf '\n== %s\n' "$*"; }
skipped() { local s; for s in "${skip[@]}"; do [[ $s == "$1" ]] && return 0; done; return 1; }

tree_at_start=$(git status --porcelain --untracked-files=all)

step_lint() { mise lint; }
step_test() { mise test; }
step_comments() { scripts/check-comments.sh; }
step_goldens() { mise goldens; }

step_qml() {
  compgen -G 'ui/*.qml' >/dev/null || { echo 'No ui/*.qml. Skipped.'; return 0; }
  local q
  q=$(command -v qmllint || command -v qmllint6 || echo /usr/lib/qt6/bin/qmllint)
  [[ -x $q ]] || { echo 'qmllint not installed. CI runs it. Skipped.'; return 0; }
  "$q" --import disable --type disable --property disable \
    --unqualified disable ui/*.qml
}

step_clean() {
  local after
  after=$(git status --porcelain --untracked-files=all)
  if [[ $after != "$tree_at_start" ]]; then
    printf 'Verification changed the tree. Tests must write to a temp dir:\n'
    diff <(printf '%s\n' "$tree_at_start") <(printf '%s\n' "$after") | sed -n 's/^> /  /p'
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
