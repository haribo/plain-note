# Document model & Markdown contract

## Purpose

A **structured** representation of a note, shared by the WYSIWYG editors (Android
first, GTK later). The **stored format stays Markdown**; the model is the
editing/interop layer. The Markdown round-trip lives in `core` so every client
uses one canonical serializer — no divergent implementations, no sync churn.

## Where it lives

`core::doc`: the `Doc` type plus `markdown_to_doc` / `doc_to_markdown`. Exposed
to Kotlin through the mobile facade (a later increment); used natively by GTK.

## Model (v1)

`Doc` is a sequence of **blocks**:

- `Heading { level: 1..=6, inlines }`
- `Paragraph { inlines }`
- `BulletList { items: [inlines] }`
- `OrderedList { items: [inlines] }`
- `TaskList { items: [{ checked, inlines }] }` — checkboxes
- `Quote { inlines }`
- `CodeBlock { text, lang? }`
- `Raw { text }` — escape hatch (see Losslessness)

`Inline` is a marked text run or a link:

- `Run { text, marks ⊆ {Bold, Italic, Strikethrough, Code} }`
- `Link { href, inlines }`

v1 keeps lists **flat** (no nesting); nesting is a later extension of the model.

## Markdown subset (the contract)

`# … ######` headings · `- ` bullets · `1. ` ordered · `- [ ]`/`- [x]` tasks ·
`> ` quote · ```` ``` ```` fenced code · `**bold**` · `*italic*` · `~~strike~~` ·
`` `code` `` · `[text](url)`. Each construct has **one** canonical output form.

## Canonicalization & idempotency

The serializer emits a single canonical form. Property (tested):
`markdown_to_doc(doc_to_markdown(doc)) == doc`. Non-canonical Markdown
(`_italic_`, `* ` bullets, extra blank lines) is **normalized on the first
save** — documented and accepted; it is the price of idempotency and it keeps
clients from producing spurious diffs.

## Losslessness via `Raw`

Any block-level construct outside the subset (tables, images, HTML, footnotes,
nested lists, …) is captured **verbatim** in a `Raw` block and re-emitted
**byte-for-byte**. Guarantee: **no silent loss**. WYSIWYG editors render `Raw`
blocks read-only with an "edit as Markdown" affordance; the raw Markdown mode
edits everything.

## Client usage

Storage stays Markdown. At edit time: note text → `markdown_to_doc` → edit the
`Doc` in the native editor → `doc_to_markdown` → save **only if the resulting
Markdown actually changed** (avoid spurious writes/diffs).

## Testing

Round-trip property tests over the subset + `Raw` passthrough; golden tests for
the canonical forms.

## Out of scope

The editor UI/engine (per client), UniFFI exposure of the model, nested lists,
and tables/images as first-class nodes (kept as `Raw` for now).
