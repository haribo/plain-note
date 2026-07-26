# Commit conventions

See also: [git-workflow.md](git-workflow.md) for branching and PR rules.

## Format

```
<type>(<scope>): <description>
<type>(<scope>)!: <description>   ← breaking change
```

## Types

`feat` | `fix` | `docs` | `style` | `refactor` | `perf` | `test` | `chore` | `ci` | `build`

## Scope

Recommended on all commits. Matches the area of change — a crate, a subsystem, or
a tooling area.

Examples: `core`, `cli`, `relay`, `gui`, `android`, `crypto`, `sync`, `model`,
`ci`, `deps`

May be omitted for a generic `style` or `chore` spanning the whole workspace.

## Breaking changes

Append `!` after the scope:

```
feat(sync)!: change delta envelope layout
```

## Squash merge commits

When a feature PR is squash-merged into `develop`, GitHub auto-appends the PR
number:

```
type(scope): description (#PR)
```

The PR title must follow `type(scope): description` — without `(#PR)`.

## Rules

1. Single line only — no body, no footer
2. Max 72 characters (excluding auto-appended `(#PR)` suffix)
3. Imperative present tense ("add" not "added")
4. No capital letter, no period
5. No AI references or promotional content
