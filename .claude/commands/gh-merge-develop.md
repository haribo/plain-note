# Develop Merge

Merge a feature PR into `develop` following project conventions with strict validation.

Accepts an optional argument: PR number or branch name. If not provided, detect from the current branch.

## Instructions

### 1. Resolve target PR

- If argument is a PR number (`#123` or `123`): use it directly
- If argument is a branch name: find the open PR for that branch targeting `develop`
- If no argument: detect the current branch, find its open PR targeting `develop`
- If no PR found: **refuse** — no PR, no merge

### 2. Collect PR information

Run all of these in parallel:
- `gh pr view <number> --json number,title,state,baseRefName,headRefName,mergeable,reviews,statusCheckRollup,commits,body,labels`
- `gh pr checks <number>`
- `gh pr diff <number>`

### 3. Validate merge readiness

Perform every check below. Collect **all** violations, then report them together. Do NOT stop at the first failure.

| # | Check | Rule |
|---|-------|------|
| 1 | PR state | Must be `OPEN` |
| 2 | Base branch | Must be `develop` — refuse any other target |
| 3 | Mergeability | `mergeable` must be `MERGEABLE` — if `CONFLICTING`, demand rebase |
| 4 | CI status | Every required check must be `pass`/`success` — no pending, no failures |
| 5 | Reviews | Zero `CHANGES_REQUESTED` — if any, report and stop |
| 6 | Diff review | Read the full diff — flag anything suspicious: debug prints (`dbg!`, `println!` in library code), TODO/FIXME without issue, hardcoded secrets, commented-out code, any code path that could read note plaintext or the E2E key, unrelated changes |

If **any** check fails: report all violations in a structured summary and **stop**.

### 4. Craft squash commit message

- Read `docs/git-commits.md` for format rules
- Analyze the PR title, body, and all individual commit messages
- Determine the correct `type(scope):` prefix from the actual changes (not blindly from the PR title)
- Write a single-line message (max 72 chars excluding the `(#N)` suffix, imperative, no capital, no period)
- Append ` (#<PR number>)` — required because `--subject` overrides GitHub's auto-append
- Breaking changes: use `type(scope)!:`
- Present the proposed message to the user for approval before merging

### 5. Merge

- Execute: `gh pr merge <number> --squash --delete-branch --subject "<approved message>" --body ""`
- If merge fails: report the error, do not retry

### 6. Local cleanup

- `git checkout develop`
- `git pull origin develop`
- If the feature branch still exists locally: `git branch -d <branch>`
- Confirm merge success with `git log --oneline -1`

## Output format

```
PR #<number> merged into develop
  <commit hash> <commit message>
  Branch <branch> deleted (remote + local)
```
