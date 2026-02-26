#!/usr/bin/env bash
# Create PR. Requires: gh auth login (or GITHUB_TOKEN).
set -e
cd "$(dirname "$0")"

head_branch="${1:-$(git rev-parse --abbrev-ref HEAD)}"
title="${2:-"chore: update pull request"}"

gh pr create \
  --base main \
  --head "$head_branch" \
  --title "$title" \
  --body-file PR_DESCRIPTION.md
