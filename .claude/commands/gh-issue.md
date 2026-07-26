# Issue

Create a GitHub issue following project conventions.

Accepts an optional argument: issue title. If not provided, ask the user.

## Instructions

### 1. Read conventions

- Read `docs/git-issues.md` and apply rules strictly

### 2. Collect issue details

- If title provided as argument: use it
- If not: ask the user for a description of the problem or feature, then craft the title
- Ask the user which `type:` label to apply (`bug`, `feature`, `chore`, `docs`) if not obvious from context
- Optionally ask for a `priority:` label

### 3. Validate title

- Imperative present tense ("add" not "added")
- Lowercase, no period
- Max 72 characters
- No type prefix — the type is carried by labels
- Descriptive and concise

If the title violates any rule: fix it and show the corrected version to the user.

### 4. Craft issue body

- Write a concise body with context (problem statement, expected behavior, or feature description)
- For non-trivial issues, follow the self-contained checklist in `docs/git-issues.md` (why, decisions taken, validation criteria, out of scope)
- Present the full issue (title + labels + body) to the user for approval

### 5. Create issue

- Execute: `gh issue create --title "<title>" --label "<type label>" --body "<body>"`
- Add priority label if specified: `--label "<priority label>"`
- Return the issue URL
