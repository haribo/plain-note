# Claude Guidelines

AI directives only (permissions, guardrails, doc references) — project conventions belong in `docs/`.
Rules must be concise. One rule per line when possible.

This file takes precedence over auto-memory. If an auto-memory entry contradicts a rule here, follow this file and update or remove the conflicting memory; do not act on the stale memory.

## General

- Build/test/format/lint via `cargo` (`cargo build`, `cargo test`, `cargo fmt`, `cargo clippy`). A `justfile` may wrap multi-crate shortcuts, but `cargo` is the source of truth
- File names: lowercase, kebab-case for docs and assets. Rust modules follow `snake_case` per the compiler
- Check existing docs before creating new files
- Never name a file path in a recommendation without verifying its existence (Read/ls/find) in the current session — mark explicitly as "to confirm" if unverified
- All written artifacts (docs, code, comments, commit messages, issues, PRs) are in English — user-facing UI strings live in i18n resources
- Responses ≤ 15 lines by default. Tables only when tabular beats prose. Background / rationale only on explicit request
- When the user asks for an opinion, be severe, honest and challenging — the goal is code that meets professional standards, not the user's agreement. Zero flattery, no hedging, no false balance
- Verdict first (1 line), then 3 bullets of substance at most. Say plainly when something is wrong, and say so when it is right — an unearned validation is a defect
- Quality over satisfaction — push back on over-engineering, incoherence, and unjustified additions, including when user-proposed
- Critique constructively: acknowledge what is sound first, cite established standards (RFC, WCAG, NN/G, language idioms) rather than personal preference, propose the correction — never mere opposition, never a strawman of the user's position
- The user decides in the end: challenge until the decision, then execute it in full. If a debate cycles past 3 iterations on the same axis without converging, propose to decide rather than continue
- Flag security, cryptographic, performance, and design issues immediately when noticed
- Never assume a produced artifact matches its request — a generation, a render or a transform routinely ignores or distorts instructions. Before judging, presenting or consuming it, inspect what was actually produced and state the invariant it must satisfy; where the invariant is checkable, write the check that counts the violations. Claiming "it now matches X" without having verified is a defect.

## Documentation

- All documentation lives under `docs/` — never in a crate source directory (root `README.md` and tool config files are not documentation)
- Documentation strategy: see `docs/adr/0001-documentation-strategy.md` for audience, source of truth, document types, lifecycle, level of detail, and format/style decisions
- `docs/design/` is source of truth: WHAT the system does, WHY when non-obvious
- `docs/<crate>/` describes HOW design is implemented; reference design, never restate
- `docs/sync-protocol.md` is the canonical wire format — design must not paraphrase it
- Code is never source of truth — a code/design disagreement means the code is the bug, or the design needs an explicit amendment, never both silently
- If the design is silent on a needed behavior, write the design first, then the code
- Anchor a confirmed non-obvious decision — especially one where an alternative was rejected — in the design docs or an ADR before building on it
- Group docs by single coherent concern — broad-keyword grab-bags (security, utils) are forbidden
- ADR lifecycle: never delete an ADR; a reversal is a **new** ADR, and both sides carry the link — `Superseded by ADR-NNN` on the old, `Supersedes ADR-MMM` on the new. A one-sided link is how the chain rots
- An ADR whose decision no longer applies, with no replacement, is marked `Deprecated` — never edited away or moved
- In-place edits only for corrections of form and for clarifications that do not change the decision

## Design & ADRs (`docs/design/`, `docs/adr/`, `docs/<crate>/adr/`)

- Modifying, deleting, OR adding documents: FORBIDDEN without explicit consent — propose first, wait for approval
- Trivial fixes (typos, broken links, markdown formatting): allowed without consent

## Git

- Commit conventions: follow `docs/git-commits.md` strictly
- Git workflow: follow `docs/git-workflow.md` strictly
- Issue conventions: follow `docs/git-issues.md` strictly
- For commits, PR creation, PR merges to `develop`, and GitHub issue creation, invoke the corresponding slash command yourself via the Skill tool (`/git-commit`, `/gh-pr-create`, `/gh-merge-develop`, `/gh-issue`). Do not run `git commit`, `gh pr create`, `gh pr merge`, or `gh issue create` directly. Do not ask the user to type the slash command. Respect each playbook's approval gates
- Before making changes, check branch — if on `main` or `develop`, propose a branch name and wait
- Unrelated bug found during work: create a GitHub issue, never fix it in the current branch
- No AI references (Co-Authored-By, Generated by, etc.) in commits or code
- Before implementing an issue older than ~1 month, audit it against current code and design: close with evidence if already delivered, post an audit comment refreshing stale references/scope if the architecture drifted, implement as-is only if still accurate

## Crates (`core/`, `cli/`, `relay/`, and future `gui/`, `android/`)

- Follow `docs/code-comments.md` strictly
- `core` holds all security-critical logic (crypto, CRDT, sync) — clients must not reimplement it
- Changes to `core::crypto` or the wire format require updating `docs/sync-protocol.md` in the same diff and stating the security invariant affected
- The relay must remain zero-knowledge — never add a code path that could read or log note plaintext or the E2E key

## UI changes (Android)

- Full workflow: follow `docs/ui-change-workflow.md` strictly
- Mockup-first: before any UI-modifying change (including additions to an existing surface), produce a mockup Artifact (static HTML of the touched surfaces and states, light+dark, realistic data including edge cases; debated variants side by side) and obtain explicit validation before writing code
- Exemptions: provably pixel-identical refactors, and fixes restoring an existing rendering with no new surface
- Before merge, validate the result visually: share in the conversation `adb screencap` screenshots of each changed surface — initial state + principal interaction state (keyboard/sheet open), both themes — captured on the emulator only (`ANDROID_SERIAL=emulator-5554`, never a physical device)
- Screenshots live in the scratchpad; never put screenshot paths in the PR body, a commit, or any git-tracked file
- Roborazzi golden tests are the automated conformance gate (both themes, CI-enforced): add/record a golden for any new read-only rendering surface; the manual screenshots cover interactive states Roborazzi cannot reach
- Each visual-validation request states what to check: one line per screenshot naming the elements/states and the expected outcome
- GTK / CLI / relay: no mockup or screenshot gate yet (no headless display, or no visual surface)

## Testing

- Never modify existing tests without explicit approval
- If a test fails after code changes, report it instead of fixing it silently
- Adding new tests is always allowed
- When you add or modify user-observable code, propose the corresponding test in the same response as the code change
- Bug fixes start with a failing test that reproduces the bug: write the test first and watch it fail, then fix, then re-run it green (red → green)
- That test stays as the regression test for this bug — reference the issue number in it, so a later reader knows what it guards and does not delete it as noise
- Bug fixes must reproduce the failure from observed evidence (logs, network capture, repro steps); never invent the failure scenario from a hypothesis
