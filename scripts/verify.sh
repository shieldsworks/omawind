#!/usr/bin/env bash
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

all_steps=(lint test comments goldens qml engine clean)
steps=("$@")
((${#steps[@]})) || steps=("${all_steps[@]}")
IFS=, read -r -a skip <<< "${VERIFY_SKIP:-}"

failed=()
say() { printf '\n== %s\n' "$*"; }
skipped() { local s; for s in "${skip[@]}"; do [[ $s == "$1" ]] && return 0; done; return 1; }

tree_at_start=$(git status --porcelain --untracked-files=all)

step_lint() { mise lint; }
step_test() { mise test; }
step_comments() {
  sha256sum -c <<'EOF' && scripts/check-comments.sh
f3ec8a2d07e8f1e7c50770d0bb93579fc7bb0d60fc8e359a22607ba55cebc6c6  scripts/check-comments.sh
EOF
}
step_goldens() { mise goldens; }

step_qml() {
  compgen -G 'ui/*.qml' >/dev/null || { echo 'No ui/*.qml. Skipped.'; return 0; }
  local q
  q=$(command -v qmllint || command -v qmllint6 || echo /usr/lib/qt6/bin/qmllint)
  [[ -x $q ]] || { echo 'qmllint not installed. CI runs it. Skipped.'; return 0; }
  "$q" --import disable --type disable --property disable \
    --unqualified disable ui/*.qml
}

step_engine() {
  if ! pkg-config --exists Qt6Core Qt6Gui Qt6Qml Qt6Network; then
    echo 'Qt6 QML dev packages not installed. Skipped.'
    return 0
  fi

  local repo_expr ui_dir parent_fn start_script prelude
  repo_expr=$(sed -n 's/^[[:space:]]*readonly property string repo:[[:space:]]*//p' ui/Wind.qml)
  ui_dir=$(sed -n 's/^[[:space:]]*readonly property string uiDir:[[:space:]]*//p' ui/Wind.qml)
  parent_fn=$(awk '
    /^[[:space:]]*function parentOf\(/ { p = 1 }
    p { print }
    p && /^[[:space:]]*}[[:space:]]*$/ { exit }
  ' ui/Wind.qml)
  start_script=$(awk -F\' '/const script = / { print $2; exit }' ui/Wind.qml)
  [[ -n $repo_expr && -n $start_script ]] \
    || { echo 'ui/Wind.qml is missing repo or the start script' >&2; return 1; }
  [[ $(printf '%s\n' "$repo_expr" | wc -l) -eq 1 ]] \
    || { echo 'repo property must be one line' >&2; return 1; }

  prelude=""
  if [[ $repo_expr == *parentOf* || $repo_expr == *uiDir* || -n $ui_dir || -n $parent_fn ]]; then
    [[ -n $ui_dir && -n $parent_fn ]] \
      || { echo 'ui/Wind.qml repo uses uiDir and parentOf, and both must be present' >&2; return 1; }
    [[ $(printf '%s\n' "$ui_dir" | wc -l) -eq 1 ]] \
      || { echo 'uiDir must be one line' >&2; return 1; }
    prelude="${parent_fn}
    readonly property string uiDir: ${ui_dir}"
  fi

  local work
  work=$(mktemp -d)
  trap "rm -rf '$work'; trap - RETURN" RETURN
  export QT_QPA_PLATFORM=offscreen
  export XDG_RUNTIME_DIR=${XDG_RUNTIME_DIR:-/tmp/runtime-omawind-verify}
  mkdir -p "$XDG_RUNTIME_DIR"
  chmod 700 "$XDG_RUNTIME_DIR"

  g++ -fPIC -std=c++17 scripts/qs-url-probe.cpp -o "$work/qs-url-probe" \
    $(pkg-config --cflags --libs Qt6Core Qt6Gui Qt6Qml Qt6Network)

  cat > "$work/probe.qml" <<EOF
import QtQuick
QtObject {
${prelude}
    readonly property string repo: ${repo_expr}
    Component.onCompleted: {
        console.log("REPO=" + repo)
        Qt.quit()
    }
}
EOF

  local failed=0
  check() {
    local name=$1 file=$2 expect=$3
    local root=${4:-}
    local out got ran runtime log
    if [[ -n $root ]]; then
      out=$("$work/qs-url-probe" "$file" "$root" 2>&1) || true
    else
      out=$("$work/qs-url-probe" "$file" 2>&1) || true
    fi
    got=$(printf '%s\n' "$out" | sed -n 's/^qml: REPO=//p' | tail -n 1)
    runtime=$work/rt
    rm -rf "$runtime"
    mkdir -p "$runtime"
    log=$runtime/engine.log
    : > "$work/ran"
    PATH=/usr/bin:/bin RECORD=$work/ran sh -c "$start_script" omawind-start "$runtime" "$got/target/release/omawind" "$log" || true
    ran=$(cat "$work/ran" 2>/dev/null || true)
    printf '\n== %s\n' "$name"
    printf '%s\n' "$out" | sed -n 's/^/  /p'
    printf '  repo: %s\n  want: %s\n  started: %s\n' "$got" "$expect" "${ran:-<none>}"
    if [[ -s $log ]]; then
      printf '  log: %s\n' "$(tail -n 1 "$log")"
    fi
    if [[ $got != "$expect" || $ran != "$expect/target/release/omawind" ]]; then
      echo '  FAIL' >&2
      failed=1
    else
      echo '  ok'
    fi
  }

  mkdir -p "$work/checkout/ui" "$work/checkout/target/release"
  cat > "$work/checkout/target/release/omawind" <<'EOF'
#!/bin/sh
printf '%s\n' "$0" > "$RECORD"
EOF
  chmod +x "$work/checkout/target/release/omawind"
  cp "$work/probe.qml" "$work/checkout/ui/probe.qml"
  check 'checkout, quickshell -p ui/shell.qml' \
    "$work/checkout/ui/probe.qml" "$work/checkout"

  mkdir -p "$work/shell/plugins/org.omahoy.wind/ui" \
    "$work/shell/plugins/org.omahoy.wind/target/release"
  cp "$work/checkout/target/release/omawind" \
    "$work/shell/plugins/org.omahoy.wind/target/release/omawind"
  cp "$work/probe.qml" "$work/shell/plugins/org.omahoy.wind/ui/probe.qml"
  check 'plugin under the shell config root' \
    "$work/shell/plugins/org.omahoy.wind/ui/probe.qml" \
    "$work/shell/plugins/org.omahoy.wind" \
    "$work/shell"

  if ! grep -q 'wrapMode: Text.Wrap' ui/WindWindow.qml \
    || ! grep -q 'engineStatus' ui/WindWindow.qml \
    || ! grep -q 'Wind.lastLog' ui/WindWindow.qml; then
    echo 'FAIL: the window does not wrap Wind.lastLog in the body' >&2
    failed=1
  else
    echo 'window surfaces Wind.lastLog'
  fi

  if ((failed)); then
    echo 'FAILED' >&2
    return 1
  fi
  echo 'All engine-path checks passed.'
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
