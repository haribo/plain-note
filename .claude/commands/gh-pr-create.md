# Pull Request

Run local checks, push, and create a PR targeting `develop`.

Accepts an optional argument: PR title. If not provided, generate one from commits.

## Instructions

### 1. Validate branch

- Run `git branch --show-current`
- If on `main` or `develop`: **refuse** — must be on a feature branch

### 2. Resolve issue reference

- Extract the issue number from the branch name: convention is `type/NUMBER-description` (e.g., `feat/12-crdt-model` → `#12`)
- Parse the number immediately after the first `/`
- If no issue number found and the PR title does NOT start with `chore` or `style`: **refuse** — ask the user to provide the issue number
- If the PR title starts with `chore` or `style`: skip — no issue reference required

### 3. Run local checks — fail on first violation, do NOT push if any step fails

Run from the workspace root:

#### 3.1 Format

- `cargo fmt --all --check`

#### 3.2 Lint

- `cargo clippy --workspace --all-targets -- -D warnings`

#### 3.3 Tests

- `cargo test --workspace`

#### Pre-existing failures — fail closed

If a check fails on code unrelated to this PR (pre-existing regression on `develop`), the gate STILL fails. Do NOT bypass. Open a separate issue documenting the pre-existing failure and stop the push. The current PR proceeds only after the pre-existing failure is fixed or explicitly acknowledged by the user.

#### 3.4 Test-up-to-date — HARD STOP, answer with evidence in the conversation

Two acceptable answers:

(a) "Existing tests cover the change" — quote the test module/function path(s) + the assertion.
(b) "Tests added in this PR" — list the test files/modules in `git diff develop...HEAD`.

UNACCEPTABLE (treated as failure): "manually verified", "clippy passes", "I think so", "no test required", silence.
The only legitimate no-test cases (pure refactor with identical behavior, doc-only, config-only) produce no user-observable change to test — state which applies.
If no acceptable answer: STOP, add tests, re-run § 3.1–3.3, retry the question.

### 4. Prepare PR content

Run in parallel:
- `git log --oneline develop..HEAD` to see all commits
- `git diff develop...HEAD --stat` to see changed files

Draft:
- **Title**: `type(scope): description` (under 70 chars), no `(#N)` suffix — appended on squash merge. Use the argument if provided.
- **Body**: summary bullets + test plan + `Closes #N` (if resolved in step 2)

### 5. Push and create PR

- Push with `git push -u origin <branch>`
- Create PR:

```
gh pr create --base develop --title "<title>" --body "$(cat <<'EOF'
## Summary
<1-3 bullet points>

## Test plan
<bulleted checklist>

Closes #<N>
EOF
)"
```

Omit the `Closes #<N>` line for `chore`/`style` PRs (no issue reference).

### 6. Output

Return the PR URL.
