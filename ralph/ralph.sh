#!/usr/bin/env bash
# Ralph loop for cctop: one Claude Code run per iteration, one story per run.
#
#   ralph/ralph.sh [max_iterations] [--model <id>] [--story US-00N] [--dry-run]
#
# Reads ralph/prd.json, drives Claude Code with ralph/prompt.md, appends to
# ralph/progress.txt, stops on <promise>COMPLETE</promise> or after
# max_iterations (default 10). Archives the previous run to ralph/archive/
# when prd.json's branchName differs from the one recorded in .last-branch.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
PRD_FILE="$SCRIPT_DIR/prd.json"
PROMPT_FILE="$SCRIPT_DIR/prompt.md"
PROGRESS_FILE="$SCRIPT_DIR/progress.txt"
ARCHIVE_DIR="$SCRIPT_DIR/archive"
LAST_BRANCH_FILE="$SCRIPT_DIR/.last-branch"
LOG_DIR="$SCRIPT_DIR/logs"

MAX_ITERATIONS=10
MODEL=""
STORY=""
DRY_RUN=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --model) MODEL="$2"; shift 2 ;;
    --model=*) MODEL="${1#*=}"; shift ;;
    --story) STORY="$2"; shift 2 ;;
    --story=*) STORY="${1#*=}"; shift ;;
    --dry-run) DRY_RUN=1; shift ;;
    -h|--help) sed -n '2,10p' "$0"; exit 0 ;;
    *)
      if [[ "$1" =~ ^[0-9]+$ ]]; then MAX_ITERATIONS="$1"; else echo "unknown argument: $1" >&2; exit 2; fi
      shift ;;
  esac
done

for tool in claude jq git; do
  command -v "$tool" >/dev/null || { echo "ralph: $tool is required" >&2; exit 2; }
done
[[ -f "$PRD_FILE" ]] || { echo "ralph: $PRD_FILE not found" >&2; exit 2; }
[[ -f "$PROMPT_FILE" ]] || { echo "ralph: $PROMPT_FILE not found" >&2; exit 2; }

BRANCH="$(jq -r '.branchName // empty' "$PRD_FILE")"
[[ -n "$BRANCH" ]] || { echo "ralph: prd.json has no branchName" >&2; exit 2; }

# Archive the previous run when the branch changed.
if [[ -f "$LAST_BRANCH_FILE" ]]; then
  LAST_BRANCH="$(cat "$LAST_BRANCH_FILE")"
  if [[ -n "$LAST_BRANCH" && "$LAST_BRANCH" != "$BRANCH" ]]; then
    FOLDER="$ARCHIVE_DIR/$(date +%Y-%m-%d)-${LAST_BRANCH#ralph/}"
    echo "Archiving previous run ($LAST_BRANCH) to $FOLDER"
    mkdir -p "$FOLDER"
    cp "$PRD_FILE" "$FOLDER/" 2>/dev/null || true
    [[ -f "$PROGRESS_FILE" ]] && cp "$PROGRESS_FILE" "$FOLDER/"
    printf '# Ralph Progress Log — %s\n# Started: %s\n# Format: [US-XXX] PASS|FAIL | summary\n#\n' \
      "$BRANCH" "$(date +%Y-%m-%d)" > "$PROGRESS_FILE"
  fi
fi
echo "$BRANCH" > "$LAST_BRANCH_FILE"

if [[ ! -f "$PROGRESS_FILE" ]]; then
  printf '# Ralph Progress Log — %s\n# Started: %s\n# Format: [US-XXX] PASS|FAIL | summary\n#\n' \
    "$BRANCH" "$(date +%Y-%m-%d)" > "$PROGRESS_FILE"
fi

# Work on the PRD's branch, created from main when missing.
cd "$REPO_DIR"
if [[ "$(git rev-parse --abbrev-ref HEAD)" != "$BRANCH" ]]; then
  if git show-ref --verify --quiet "refs/heads/$BRANCH"; then
    git checkout -q "$BRANCH"
  else
    git checkout -q -b "$BRANCH" main
  fi
fi
echo "Branch: $BRANCH"

remaining() { jq '[.userStories[] | select(.passes == false)] | length' "$PRD_FILE"; }
next_story() {
  jq -r '[.userStories[] | select(.passes == false)] | sort_by(.priority) | .[0].id // empty' "$PRD_FILE"
}

if [[ "$(remaining)" == "0" ]]; then
  echo "All stories already pass."
  exit 0
fi

mkdir -p "$LOG_DIR"
export CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1   # the pane stories validate/load the hooks module

CLAUDE_ARGS=(--dangerously-skip-permissions --print)
[[ -n "$MODEL" ]] && CLAUDE_ARGS+=(--model "$MODEL")

echo "Starting Ralph — max $MAX_ITERATIONS iteration(s), $(remaining) story(ies) open"

for i in $(seq 1 "$MAX_ITERATIONS"); do
  TARGET="${STORY:-$(next_story)}"
  [[ -n "$TARGET" ]] || { echo "No open story left."; exit 0; }

  echo
  echo "=============================================================="
  echo "  Ralph iteration $i of $MAX_ITERATIONS — $TARGET"
  echo "=============================================================="

  PROMPT="$(cat "$PROMPT_FILE")"
  PROMPT+=$'\n\n'"Story for this iteration: $TARGET (the highest-priority story with passes: false"
  [[ -n "$STORY" ]] && PROMPT+=", pinned by the operator"
  PROMPT+=")."

  if [[ "$DRY_RUN" == "1" ]]; then
    echo "[dry-run] claude ${CLAUDE_ARGS[*]} < prompt ($(wc -c <<<"$PROMPT") bytes)"
    exit 0
  fi

  LOG="$LOG_DIR/$(date +%Y%m%d-%H%M%S)-$TARGET.log"
  OUTPUT="$(claude "${CLAUDE_ARGS[@]}" <<<"$PROMPT" 2>&1 | tee "$LOG" | tee /dev/stderr)" || true

  if grep -q '<promise>COMPLETE</promise>' <<<"$OUTPUT"; then
    echo
    echo "Ralph completed all stories at iteration $i."
    exit 0
  fi

  if jq -e --arg id "$TARGET" '.userStories[] | select(.id == $id and .passes == true)' "$PRD_FILE" >/dev/null; then
    echo "$TARGET passed. $(remaining) story(ies) left."
    [[ -n "$STORY" ]] && exit 0
  else
    echo "$TARGET is still open after this iteration (see $LOG)."
  fi
  sleep 2
done

echo
echo "Ralph reached max iterations ($MAX_ITERATIONS) with $(remaining) story(ies) still open."
echo "See $PROGRESS_FILE and $LOG_DIR."
exit 1
