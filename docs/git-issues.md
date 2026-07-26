# Issue conventions

See also: [git-commits.md](git-commits.md) for commit conventions, [git-workflow.md](git-workflow.md) for branching and PR rules.

## Title

```
<imperative description>
```

The title is a pure description — the type is carried by labels, not the title.

## Rules

1. Imperative present tense ("add" not "added")
2. Lowercase, no period
3. Max 72 characters
4. Descriptive and concise — the title should stand alone without needing context

## Labels

Use prefixed labels to categorize issues. The title must not duplicate label
information.

### Type (required — pick one)

| Label | Usage |
|-------|-------|
| `type: bug` | defect or malfunction |
| `type: feature` | new feature or improvement |
| `type: chore` | CI, tooling, maintenance, cleanup |
| `type: docs` | documentation change |

### Priority (optional)

| Label | Usage |
|-------|-------|
| `priority: critical` | requires immediate attention |

### State (optional — closing qualifiers)

| Label | Usage |
|-------|-------|
| `state: wontfix` | will not be worked on |
| `state: duplicate` | already exists |
| `state: invalid` | not a valid issue |

## Self-contained content for autonomous implementation

If an issue may be implemented by someone without context from the originating
discussion (another Claude session, a different dev, or your future self in 6
months), the body must include enough to execute without asking questions:

- **Why** the change is needed (1-3 lines of context)
- **All decisions already taken** (not "TBD" placeholders unless truly open)
- **Explicit mappings** for refactors (current state → target state per artifact)
- **Validation criteria** (concrete checks: grep patterns, file lists, test names)
- **PR strategy** when multi-task (one PR vs split, with rationale)
- **Out of scope** items, to prevent scope creep at implementation time

This rule does not apply to trivial issues (typo fixes, single-line config
changes). Apply judgment: if the implementing party would need to ask "what did
you mean by X?" or "should I do A or B?", the issue is incomplete.

## Epics — grouping related issues

When a feature naturally splits into **~3 or more related pieces of the same
subsystem**, write an **epic** plus short sub-issues rather than independent
issues:

- **The epic carries the shared context once**: why, locked decisions,
  doc-anchoring plan, out-of-scope. It lists the sub-issues as a checklist.
- **Sub-issues stay short**: `Part of #<epic>`, a Build section and a Validation
  section. No repetition of the epic's context.
- **PR strategy**: sibling sub-issues of one epic may ship in a single batched PR
  (`Closes` each) when they were designed together and are individually low-risk.
  Never across epics; never bug fixes mixed with features.
- **Don't over-epic**: an isolated change stays a plain issue. The threshold is
  coherence of design, not size alone.

## Examples

| Title | Labels |
|-------|--------|
| `add xchacha20-poly1305 envelope to core` | `type: feature` |
| `fix delta replay dropping first change on reconnect` | `type: bug`, `priority: critical` |
| `add commitlint check to ci` | `type: chore` |
| `document qr pairing payload` | `type: docs` |
