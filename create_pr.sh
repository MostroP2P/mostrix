#!/usr/bin/env bash
# Create PR for file-attachment branch. Requires: gh auth login (or GITHUB_TOKEN).
set -e
cd "$(dirname "$0")"
gh pr create \
  --base main \
  --head file-attachment \
  --title "fix(admin-chat): UX, validation, and attachment compatibility" \
  --body-file PR_DESCRIPTION.md
