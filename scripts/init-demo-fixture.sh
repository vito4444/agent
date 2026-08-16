#!/usr/bin/env bash
# Demo worktrees need a git repo. Nested .git is not committed — recreate locally.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEMO="$ROOT/fixtures/demo-project"
cd "$DEMO"
if [[ ! -d .git ]]; then
  git init
  git config user.email "demo@local"
  git config user.name "demo"
  git add -A
  git commit -m "demo fixture: broken add + failing test"
  echo "initialized $DEMO"
else
  echo "already a git repo: $DEMO"
fi
