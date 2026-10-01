#!/usr/bin/env bash
# The checks that need the `claude` CLI, in one run: what CI's `contract` job
# does daily and on every push, and what `make check-contract` does here.
#
#   1. The headless load check of docs/verification/pane.md §A, with no model
#      call and no credentials: the prompt is the native `/plugin-types`
#      command, which fires session.start like any run and writes the
#      contract beside it. The debug log must show the module loaded,
#      /cctop-pane listed, the self-check line, nothing refused, no hook
#      failed; the marker must say `loaded: true`.
#   2. The contract surface: the d.ts just written against the checked-in
#      plugin/.claude/types/claude-code.d.ts, first line (the version) and
#      the built-in tool tables (per machine: MCP servers, feature flags)
#      aside. A difference names the declarations that moved.
#   3. `claude plugin validate --strict ./plugin`.
#   4. scripts/check-plugin-types.sh: the d.ts's version pin and the
#      harness_facts lag.
#
# Every check runs; the failures are listed at the end and the exit is 1
# when there is one. HOME and CLAUDE_CONFIG_DIR are a temp dir for the
# claude run, so nothing lands in this machine's ~/.claude or ~/.cctop.
set -uo pipefail
cd "$(dirname "$0")/.."

if ! command -v claude >/dev/null 2>&1; then
  echo "check-contract: claude not on PATH" >&2
  exit 1
fi
claude_version=$(claude --version | grep -oE '^[0-9]+\.[0-9]+\.[0-9]+')
plugin_version=$(grep -oE '"version": *"[^"]+"' plugin/.claude-plugin/plugin.json | head -1 | grep -oE '[0-9]+\.[0-9]+\.[0-9]+')
dts=plugin/.claude/types/claude-code.d.ts
# The built-in tool tables start here; what precedes is the API surface.
tools_marker='// The inputs of the built-in tools this build has'

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/home" "$work/config" "$work/types"
failures=()
fail() { failures+=("$1"); echo "check-contract: FAIL: $1" >&2; }
pass() { echo "check-contract: ok: $1"; }

# --- 1. the load check -------------------------------------------------------
echo "check-contract: claude $claude_version, plugin $plugin_version"
HOME="$work/home" CLAUDE_CONFIG_DIR="$work/config" CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 \
  claude -p --debug --plugin-dir ./plugin "/plugin-types $work/types" >"$work/stdout.txt" 2>&1
exit_code=$?
log="$work/config/debug/latest"
if [ "$exit_code" -ne 0 ]; then
  fail "claude -p exited $exit_code"
  sed -n '1,20p' "$work/stdout.txt" >&2
fi
if [ ! -f "$log" ]; then
  fail "no debug log at $log"
  log=/dev/null
fi
# Since 2.1.278 the line names the plugin with its source, `cctop@inline`
# under --plugin-dir; the suffix is where it came from, not its name.
load_line=$(grep -E 'hooks module cctop(@[A-Za-z0-9_.-]+)? loaded' "$log" | head -1)
if [ -n "$load_line" ]; then pass "module loaded (${load_line#*loaded }"; else fail "no 'hooks module cctop loaded' line"; fi
if grep -q '/cctop-pane listed' "$log"; then pass "/cctop-pane listed"; else fail "no '/cctop-pane listed' line"; fi
self_check=$(grep -o 'cctop: plugin .*' "$log" | head -1)
case "$self_check" in
  *"self-check ok") pass "$self_check" ;;
  "") fail "no 'cctop: plugin … self-check' line" ;;
  *) fail "$self_check" ;;
esac
refused=$(grep -i 'refused' "$log" | grep -v 'auto-mode' || true)
if [ -z "$refused" ]; then pass "nothing refused"; else fail "refused: $(echo "$refused" | head -3)"; fi
hook_failed=$(grep 'hook failed' "$log" || true)
if [ -z "$hook_failed" ]; then pass "no hook failed"; else fail "hook failed: $(echo "$hook_failed" | head -3)"; fi
cctop_failed=$(grep -E 'cctop: .*failed' "$log" | grep -v 'self-check' || true)
if [ -z "$cctop_failed" ]; then pass "no cctop: … failed line"; else fail "$(echo "$cctop_failed" | head -3)"; fi
marker=$(ls "$work/home/.cctop/pane/"*.json 2>/dev/null | head -1)
if [ -n "$marker" ] && grep -q '"loaded":true' "$marker"; then
  pass "marker written with loaded: true ($(grep -oE '"(version|testedWith|selfCheck)":("[^"]*"|null)' "$marker" | tr '\n' ' '))"
else
  fail "no marker with loaded: true under \$HOME/.cctop/pane"
fi

# --- 2. the contract surface -------------------------------------------------
fresh="$work/types/claude-code.d.ts"
if [ ! -f "$fresh" ]; then
  fail "/plugin-types wrote no $fresh"
else
  fresh_version=$(sed -n '1p' "$fresh" | grep -oE '[0-9]+\.[0-9]+\.[0-9]+')
  # The surface: everything after line 1 and before the tool tables.
  surface() {
    if ! grep -qF "$tools_marker" "$1"; then
      echo "check-contract: note: $1 has no tool-table marker; comparing the whole file" >&2
    fi
    awk -v marker="$tools_marker" 'NR == 1 { next } index($0, marker) == 1 { exit } { print }' "$1"
  }
  surface "$dts" >"$work/checked-in.txt"
  surface "$fresh" >"$work/fresh.txt"
  # A surface of a few lines is an extraction gone wrong, never a contract.
  for side in checked-in fresh; do
    if [ "$(wc -l <"$work/$side.txt")" -lt 1000 ]; then
      fail "the $side surface came out at $(wc -l <"$work/$side.txt") lines: the extraction is broken, not the contract"
    fi
  done
  if diff -q "$work/checked-in.txt" "$work/fresh.txt" >/dev/null; then
    pass "contract surface unchanged (checked in: $(sed -n '1p' "$dts" | grep -oE '[0-9]+\.[0-9]+\.[0-9]+'); written by $fresh_version)"
  else
    # The declarations the changed lines fall under, each once: the hunk
    # headers of a plain diff (`459c459`, `7324,7326d7323`, `100a101`) give
    # the checked-in side's first line, and the nearest `export …` above
    # it names the declaration. BSD and GNU diff alike.
    moved=$(diff "$work/checked-in.txt" "$work/fresh.txt" | grep -E '^[0-9]' | awk -F'[acd]' '{ split($1, r, ","); print r[1] }' \
      | awk 'NR == FNR { changed[$1] = 1; next }
             /^  (export )?(type|interface|const|function|namespace) [A-Za-z_]/ { name = $0; sub(/^  (export )?(type|interface|const|function|namespace) /, "", name); sub(/[^A-Za-z0-9_].*/, "", name); current = name }
             (FNR in changed) && current != "" && !(current in seen) { seen[current] = 1; print current }' - "$work/checked-in.txt")
    moved=${moved//$'\n'/, }
    fail "contract surface changed between $(sed -n '1p' "$dts" | grep -oE '[0-9]+\.[0-9]+\.[0-9]+') (checked in) and $fresh_version (installed); declarations that moved: ${moved:-see the diff}"
    diff -U 4 "$work/checked-in.txt" "$work/fresh.txt" | head -200 >&2
    echo "check-contract: regenerate with: cd plugin && CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 claude -p '/plugin-types ./.claude/types'; then npm run typecheck names every call site" >&2
  fi
fi

# --- 3. the validator ---------------------------------------------------------
if CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 claude plugin validate --strict ./plugin >"$work/validate.txt" 2>&1; then
  pass "claude plugin validate --strict"
else
  fail "claude plugin validate --strict ./plugin"
  cat "$work/validate.txt" >&2
fi

# --- 4. the version pins -------------------------------------------------------
if ./scripts/check-plugin-types.sh >"$work/types.txt" 2>&1; then
  pass "check-plugin-types ($(grep -c warning "$work/types.txt") warning(s))"
  grep warning "$work/types.txt" >&2 || true
else
  fail "check-plugin-types: $(grep -m1 'check-plugin-types:' "$work/types.txt" | sed 's/^check-plugin-types: //')"
  cat "$work/types.txt" >&2
fi

# --- the result line, in the shape docs/verification/pane.md §A records ------
today=$(date -u +%Y-%m-%d)
if [ "${#failures[@]}" -eq 0 ]; then
  result="Headless load check $today: Claude Code $claude_version, plugin $plugin_version — exit 0, load line present, /cctop-pane listed, self-check ok, nothing refused, no hook failed, marker written with loaded: true; contract surface unchanged; validator and version pins green."
else
  result="Headless load check $today: Claude Code $claude_version, plugin $plugin_version — ${#failures[@]} failure(s): $(printf '%s; ' "${failures[@]}")"
fi
echo "check-contract: $result"
if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
  {
    echo "### contract"
    echo
    echo "$result"
    echo
    echo '`docs/verification/pane.md` §A carries the same line; a person copies it there when recording a run.'
  } >>"$GITHUB_STEP_SUMMARY"
fi
[ "${#failures[@]}" -eq 0 ]
