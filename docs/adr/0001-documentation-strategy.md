# ADR 0001 — Documentation strategy

## Status

Active

## Context

`note` is a solo + AI project spanning several Rust crates (`core`, `cli`,
`relay`) and future clients (GUI, Android). Without a shared strategy,
documentation accretes ad-hoc: every session re-litigates audience, source of
truth, document types, lifecycle, detail level, and format. This ADR fixes the
framework once.

## Decisions

### 1. Audience priority

| Priority | Audience |
|----------|----------|
| **PRIMARY** | Solo dev + Claude assistants |
| **SECONDARY** | Future human contributors |
| **OUT OF SCOPE** | Non-tech stakeholders |

Style optimized for PRIMARY. SECONDARY is served without extra pedagogy.
Non-tech artifacts (marketing, slides) live elsewhere, not in `docs/`.

### 2. Source of truth

| Source | Scope |
|--------|-------|
| `docs/design/` | The product as observed by the user — what the system does, what the user can/cannot do, business rules, contracts |
| Code (with rustdoc / inline comments where non-obvious) | How it is implemented |
| `docs/sync-protocol.md` | Canonical wire format between clients and relay (message shapes, field names, envelope layout, sequence semantics) |

The sync-protocol spec is treated as **source of truth for the wire format**, the
way an OpenAPI file would be: the design must not paraphrase it. Field names,
exact caps, byte layouts, and message types live there and in the code, never
duplicated into design.

ADRs (`docs/adr/`, `docs/<crate>/adr/`) capture decisions with rationale,
regardless of which layer they affect. A rule may live in design (the *what*,
user-observable) with its rationale in an ADR (the *why*, with alternatives).
Both coexist; the ADR is not a duplicate.

**Forbidden**: documentation that paraphrases code. Struct definitions, function
signatures, file trees, generated structure — if it is derivable from reading the
code, it is not in the doc.

**Discriminant test for protocol/wire rules** — when unsure whether a fact
belongs to `docs/design/` or to `docs/sync-protocol.md` / code:

- If the value can change without affecting user-observable behavior, it is HOW →
  protocol spec / code. Examples: message type names, exact nonce length, JSON
  field names, sequence-number width.
- If the user-observable behavior depends on the value or its existence, it is
  WHAT → design. Examples: "the relay can never read note content", "conflicts
  never lose data", "a device can be revoked without re-keying the group".

The gap between design and code is intentional. ADRs and technical docs fill it
where decisions or non-obvious patterns deserve a written trace. The rest is the
code itself.

### 3. Document types

Three types only.

| # | Type | Location | Scope |
|---|---|---|---|
| 1 | **Design** | `docs/design/` | User-observable rules of the product |
| 2 | **ADR** | `docs/adr/`, `docs/<crate>/adr/` | Decisions with rationale, alternatives rejected, consequences |
| 3 | **Technical doc** | `docs/<crate>/*.md` (excluding `adr/`), or `docs/*.md` for cross-cutting concerns | Conventions and operational rules that are neither design nor ADR |

`<crate>` is one of `core`, `cli`, `gui`, `relay`, `android`. Cross-cutting
technical docs that belong to no single crate live at `docs/` root (e.g.
`code-comments.md`, `git-*.md`, `sync-protocol.md`).

**Out of scope as types:**
- Code comments — governed by `docs/code-comments.md`, not a documentation type.
- Root `README.md` — single-file GitHub convention, not a category.
- Onboarding tutorials — a senior dev reads design + ADRs + code. Add only if a
  real gap emerges.

A new technical doc must pass the severe 4-point test (§ 4).

### 4. Lifecycle and ownership

**ADR lifecycle** — append-only. An accepted ADR is immutable: in-place edits only
for corrections of form and for clarifications that do not change the decision.
A reversal is a new ADR; the old one keeps its body, and both sides carry the
link. An ADR is never deleted, nor moved to another directory. Status:
- `Active` — current decision.
- `Superseded by ADR-XYZ` — replaced; the new record carries `Supersedes ADR-ABC`
  back to this one. One-sided links rot: mark both.
- `Deprecated` — no longer applies, and nothing replaced it.

Applies from the merge of this change. Records written earlier were maintained
under an editable policy and may have been amended in place.

**Design lifecycle** — synchronous with code (per `CLAUDE.md`): a change
introducing user-facing behavior updates `docs/design/` in the same diff.
Design-first if silent.

**Technical-doc lifecycle** — synchronous with the code/behavior it describes.

**Drift detection** — reactive, triggered by signals (a noticed divergence, a
focused audit, a new doc type), not calendar-based.

**Severe 4-point test** — applies to: new technical doc, new H2/H3 section,
change to an existing rule. Skipped for typos and reformulations. All four
required:

1. **Singular concern**: fits one word/concept, no "and" mixing unrelated topics.
   `crypto` ✓ — `security (auth + rate limit + headers + ...)` ✗
2. **Not in the code**: says something types, signatures, or generated structure
   don't already say. "why nonces are 24 bytes (random, cross-device)" ✓ —
   listing struct fields ✗
3. **No duplicate**: not already in design, an ADR, a code comment, or another
   technical doc. `grep` confirms uniqueness.
4. **Senior dev would write it spontaneously**: hand the repo to a senior dev and
   they would eventually feel the need to write it because the info is missing.

If any criterion fails, remove, merge, or rewrite.

### 5. Level of detail (design docs)

**Adaptive**: capture every user-observable invariant, no more. Size follows the
concept — simple concept → short doc; complex concept → long doc. Reducing a
complex doc artificially causes re-deciding rules every session; inflating a
simple doc with pedagogy is forbidden.

**Mandatory style guards:**

1. Tables > prose for any set of (key, value, condition).
2. No explicit paraphrase ("as we saw above…").
3. No narrative examples ("imagine Bob who…").
4. No rule derivable from another rule in the same doc.
5. No pedagogy.

**Practical test**: each line captures an invariant a senior dev cannot infer
from the rest of the doc. If a line fails, remove it.

### 6. Format and style

| Element | Rule |
|---|---|
| Diagrams | Forbidden by default; prefer state-transition tables. Mermaid only if the content cannot be tabular (e.g. deep tree) and renders natively on GitHub. ASCII art and PNG forbidden. |
| Headings | H1 = title; H2 = sections; H3 = subsections; H4 maximum. Beyond H4 = refactor. |
| Cross-references | Standard markdown `[text](path)`. No line numbers. Section anchors sparingly. |
| File naming | lowercase, kebab-case. No underscores, no mixedCase. |
| Code blocks | Rust source **forbidden** in docs — paraphrase (§ 2). Allowed only for textual data a user or operator produces/consumes: CLI output, sample inputs, QR payload shape. Wire-format JSON belongs in `sync-protocol.md`, not design. |
| Emojis | Forbidden in docs. |

## Rationale

**Audience** — "everyone" = "no one". Pedagogy for non-tech conflicts with a
dense reference for solo dev + Claude. PRIMARY/SECONDARY split serves both without
compromise.

**Source of truth** — over-documentation creates drift. If a byte layout is in
design, the reader of the code doesn't see the rationale; if the rationale is a
code comment, it lives where the dev needs it. The intentional gap prevents drift.
Variants rejected: design-as-single-source (bottleneck in a solo+AI flow),
design-only-for-critical-invariants (who decides what is critical). Adopted:
design = user-observable, code + protocol spec = implementation, ADRs = decisions.

**Document types** — three types give enough structure without fragmentation.
Readers navigate by topic (file name), not by type. Code comments are a code
construct, not a doc type. README is a GitHub convention.

## Consequences

- Every file in `docs/` is exactly one of the three types.
- Design docs describe behavior, not implementation — no struct dumps, no Rust
  source blocks (data representations allowed per § 6).
- A technical doc must pass the 4-point test to be created or kept.
- `docs/adr/` (project-level) starts with this ADR. Crate-scoped ADRs live in
  `docs/<crate>/adr/`.
- A new "type" cannot be added without amending this ADR.

## References

- `CLAUDE.md` — operational rules referencing this ADR for the why
- `docs/sync-protocol.md` — canonical wire format (source of truth)
