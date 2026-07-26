# Git workflow

See also: [git-commits.md](git-commits.md) for commit conventions, [git-issues.md](git-issues.md) for issue conventions.

## Branches

| Branch | Role |
|--------|------|
| `main` | Stable / released |
| `develop` | Integration |

Both branches are permanent — never push directly, always via PR.

## Branch protection

Enforced at two layers:

- **Local**: the `pre-push` hook refuses any direct push to `main` or `develop`.
- **Remote (GitHub)**: enable branch protection once the repository exists. With
  the `gh` CLI authenticated:

  ```sh
  gh api -X PUT repos/haribo/plain-note/branches/main/protection \
    -F required_pull_request_reviews.required_approving_review_count=0 \
    -F enforce_admins=true -F required_status_checks=false -F restrictions=false
  gh api -X PUT repos/haribo/plain-note/branches/develop/protection \
    -F required_pull_request_reviews.required_approving_review_count=0 \
    -F enforce_admins=true -F required_status_checks=false -F restrictions=false
  ```

  Tighten `required_status_checks` to the CI job names once CI exists.

## Issue-first workflow

Every change starts with a GitHub issue, except trivial changes (typo,
formatting, dependency bump) where the PR alone suffices.

- The issue describes the **what/why** — the PR describes the **how**
- The branch name includes the issue number for traceability
- The PR body references the issue with `Closes #N` to auto-close on merge

```
issue #12 → branch feat/12-crdt-model → PR "Closes #12" → squash merge
```

## Feature workflow

```bash
# 1. Create issue
/gh-issue

# 2. Create branch from develop — include issue number
git checkout -b feat/12-crdt-model develop

# 3. Work, commit
/git-commit

# 4. Rebase on develop before opening PR
git fetch origin && git rebase origin/develop

# 5. Open PR — MUST target develop, reference the issue
/gh-pr-create

# 6. Wait for CI to pass
gh pr checks

# 7. Squash merge — NEVER use --merge on feature PRs
/gh-merge-develop
```

## Release workflow

```bash
# 1. Open PR develop → main (only when integration is validated)
gh pr create --base main --head develop

# 2. Wait for CI to pass
gh pr checks

# 3. Merge commit — NEVER squash release PRs
gh pr merge --merge

# 4. Tag
git tag vX.Y.Z && git push origin vX.Y.Z
```

## Merge strategy

| Target | Strategy | Command |
|--------|----------|---------|
| Feature → `develop` | **Squash** | `/gh-merge-develop` |
| `develop` → `main` | **Merge commit** | `gh pr merge --merge` |

**NEVER merge a feature PR with `--merge` — always squash via `/gh-merge-develop`.**
**NEVER target `main` with a feature PR — always target `develop`.**

## Rules

- Never push directly to `main` or `develop` — always via PR
- One logical change per PR — split unrelated work into separate PRs
- Keep feature branches short-lived (days, not weeks)
- Rebase on `develop` before opening PR to avoid merge conflicts
- A PR introducing user-facing behavior must update `docs/design/` in the same
  diff — bug fixes and refactors are exempt

## Branch naming

```
feat/12-short-description
fix/34-short-description
refactor/56-short-description
docs/78-short-description
chore/short-description
```

Prefix matches commit type. Include issue number after the slash. Use kebab-case.
May omit the issue number for trivial `chore`/`style` changes without an issue.
The `pre-push` hook enforces this pattern.
