# Commit

Commit ALL pending changes following project conventions. On feature branches, rebuild the entire branch history to keep commits clean and logical.

## Instructions

1. Read `docs/git-commits.md` and apply rules strictly
2. Run `git branch --show-current` — if on `main` or `develop`, propose a branch name following `docs/git-workflow.md` naming rules (e.g., `feat/12-short-description`) and **wait** for the user to switch
3. Run `git status` and `git diff` to understand ALL pending changes
4. Run `git diff develop...HEAD --name-only` to list all files already committed on the branch
5. Run `git log --oneline develop..HEAD` to see existing branch commits
6. Analyze ALL changed files (both pending and already committed) as a whole:
   - Group files by logical step (tightly coupled changes = one commit)
   - Each commit must represent a **finalized logical step**, not a work-in-progress
   - Files that are tightly coupled (a function signature change + all call sites) belong in the same commit
7. If files already committed on the branch are affected by pending changes, or if the current commit grouping doesn't reflect logical steps:
   - `git reset --soft develop` — collapse all branch commits
   - Re-commit all files in logical groups with proper messages
8. If no existing branch commits are affected:
   - Commit pending changes normally, grouped logically
9. Present the proposed commit plan (files per commit + messages) to the user for approval before executing

## Rules

- Never push to remote
- Nothing should remain uncommitted after execution
- Each commit message follows `docs/git-commits.md` format strictly
