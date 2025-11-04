#!/usr/bin/env bash
set -u

# Run ALL integration tests in parallel, one test per tmux window.
# Only a single test is shown at a time; switch with Ctrl-b S (bound to choose-window).

if ! command -v tmux >/dev/null 2>&1; then
  echo "tmux is required for docker_test_all. Please install tmux." >&2
  exit 1
fi

SESSION="swap-tests-$(date +%Y%m%d-%H%M%S)"

# Collect integration test names from swap/tests/*.rs
tests=$(find swap/tests -maxdepth 1 -type f -name '*.rs' -exec basename {} \; | sed 's/\.rs$//')
if [ -z "${tests}" ]; then
  echo "No integration tests found under swap/tests" >&2
  exit 1
fi

# Create the session with the first window, and bind Ctrl-b S to show a vertical list of windows (tasks)
tmux new-session -d -s "${SESSION}" -n "w0"
# Bind globally for this tmux server instance (best-effort; ignore if user overrides)
tmux bind-key S choose-tree -w -Z >/dev/null 2>&1 || true

# Kill the session when the last client detaches (close terminal window)
tmux set-hook -t "${SESSION}" client-detached "kill-session -t ${SESSION}" >/dev/null 2>&1 || true

idx=0
for test_name in ${tests}; do
  if [ "${idx}" -eq 0 ]; then
    # Use window 0 for the first test
    target="${SESSION}:0"
  else
    # Create a new window per test (only one test visible at a time)
    tmux new-window -t "${SESSION}" -n "${test_name}" >/dev/null 2>&1 || true
    target="${SESSION}:-1" # the latest window
  fi

  cmd="echo '===== START ${test_name} ====='; cargo test --package swap --test '${test_name}' -- --nocapture; code=\$?; echo '===== END ${test_name} (exit='\$code') ====='"
  tmux send-keys -t "${target}" "bash -lc \"${cmd}\"" C-m
  idx=$((idx + 1))
done

# Window lifetime behavior
if [ "${KEEP_OUTPUT:-0}" = "1" ]; then
  # Keep panes open after command exits (so you can read output)
  tmux set-option -t "${SESSION}" remain-on-exit on >/dev/null 2>&1 || true
else
  # Temporary panes: close automatically when the command exits
  tmux set-option -t "${SESSION}" remain-on-exit off >/dev/null 2>&1 || true
fi

echo "Attached to tmux session: ${SESSION}"
tmux attach -t "${SESSION}"
# On detach, ensure the entire session and all tasks are killed
tmux kill-session -t "${SESSION}" >/dev/null 2>&1 || true
