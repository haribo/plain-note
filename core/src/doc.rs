//! Structured document model and canonical Markdown round-trip.
//!
//! The stored format stays Markdown; this model is the editing/interop layer
//! shared by the WYSIWYG editors. See `docs/design/document-model.md`.
//!
//! [`doc_to_markdown`] emits a single canonical form and
//! [`markdown_to_doc`] parses the supported subset. Anything outside the subset
//! (tables, images, HTML, …) is preserved verbatim in [`Block::Raw`] so no
//! content is ever lost. Non-canonical Markdown is normalized on the first
//! serialization — the price of an idempotent round-trip.

/// A note as a sequence of blocks.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Doc {
    pub blocks: Vec<Block>,
}

/// A block-level element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Heading {
        level: u8,
        inlines: Vec<Inline>,
    },
    Paragraph {
        inlines: Vec<Inline>,
    },
    BulletList {
        items: Vec<Vec<Inline>>,
    },
    OrderedList {
        items: Vec<Vec<Inline>>,
    },
    TaskList {
        items: Vec<TaskItem>,
    },
    Quote {
        inlines: Vec<Inline>,
    },
    CodeBlock {
        text: String,
        lang: Option<String>,
    },
    /// Verbatim Markdown outside the supported subset (round-trips byte-for-byte).
    Raw {
        text: String,
    },
}

/// A task-list item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskItem {
    pub checked: bool,
    pub inlines: Vec<Inline>,
}

/// Inline content: a marked text run or a link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inline {
    Run { text: String, marks: Marks },
    Link { href: String, inlines: Vec<Inline> },
}

/// The set of inline marks on a run. v1 parses one mark per span (no nesting).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Marks {
    pub bold: bool,
    pub italic: bool,
    pub strikethrough: bool,
    pub code: bool,
}

impl Marks {
    fn one(f: fn(&mut Marks)) -> Marks {
        let mut m = Marks::default();
        f(&mut m);
        m
    }
}

// --- serialization (canonical) ---

/// Serialize a document to canonical Markdown.
pub fn doc_to_markdown(doc: &Doc) -> String {
    doc.blocks
        .iter()
        .map(block_to_md)
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn block_to_md(b: &Block) -> String {
    match b {
        Block::Heading { level, inlines } => {
            format!("{} {}", "#".repeat(*level as usize), inlines_to_md(inlines))
        }
        Block::Paragraph { inlines } => inlines_to_md(inlines),
        Block::BulletList { items } => items
            .iter()
            .map(|it| format!("- {}", inlines_to_md(it)))
            .collect::<Vec<_>>()
            .join("\n"),
        Block::OrderedList { items } => items
            .iter()
            .enumerate()
            .map(|(i, it)| format!("{}. {}", i + 1, inlines_to_md(it)))
            .collect::<Vec<_>>()
            .join("\n"),
        Block::TaskList { items } => items
            .iter()
            .map(|t| {
                format!(
                    "- [{}] {}",
                    if t.checked { "x" } else { " " },
                    inlines_to_md(&t.inlines)
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Block::Quote { inlines } => format!("> {}", inlines_to_md(inlines)),
        Block::CodeBlock { text, lang } => {
            format!("```{}\n{}\n```", lang.as_deref().unwrap_or(""), text)
        }
        Block::Raw { text } => text.clone(),
    }
}

fn inlines_to_md(inlines: &[Inline]) -> String {
    inlines.iter().map(inline_to_md).collect()
}

fn inline_to_md(i: &Inline) -> String {
    match i {
        Inline::Run { text, marks } => apply_marks(text, *marks),
        Inline::Link { href, inlines } => format!("[{}]({href})", inlines_to_md(inlines)),
    }
}

/// Wrap text with its marks in a fixed nesting order (bold outermost).
fn apply_marks(text: &str, m: Marks) -> String {
    let mut s = text.to_string();
    if m.code {
        s = format!("`{s}`");
    }
    if m.strikethrough {
        s = format!("~~{s}~~");
    }
    if m.italic {
        s = format!("*{s}*");
    }
    if m.bold {
        s = format!("**{s}**");
    }
    s
}

// --- parsing ---

/// Parse Markdown into a document. Unsupported constructs are kept as
/// [`Block::Raw`]; non-canonical input is normalized on the next serialization.
pub fn markdown_to_doc(md: &str) -> Doc {
    let lines: Vec<&str> = md.split('\n').collect();
    let mut blocks = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.trim().is_empty() {
            i += 1;
            continue;
        }
        // Fenced code block.
        if let Some(lang) = line.trim_start().strip_prefix("```") {
            let lang = lang.trim();
            let mut text = Vec::new();
            i += 1;
            while i < lines.len() && !lines[i].trim_start().starts_with("```") {
                text.push(lines[i]);
                i += 1;
            }
            if i < lines.len() {
                i += 1; // consume closing fence
            }
            blocks.push(Block::CodeBlock {
                text: text.join("\n"),
                lang: if lang.is_empty() {
                    None
                } else {
                    Some(lang.to_string())
                },
            });
            continue;
        }
        if let Some((level, rest)) = parse_heading(line) {
            blocks.push(Block::Heading {
                level,
                inlines: parse_inlines(rest),
            });
            i += 1;
        } else if parse_task(line).is_some() {
            let mut items = Vec::new();
            while i < lines.len() {
                match parse_task(lines[i]) {
                    Some((checked, rest)) => {
                        items.push(TaskItem {
                            checked,
                            inlines: parse_inlines(rest),
                        });
                        i += 1;
                    }
                    None => break,
                }
            }
            blocks.push(Block::TaskList { items });
        } else if parse_bullet(line).is_some() {
            let mut items = Vec::new();
            while i < lines.len() {
                match parse_bullet(lines[i]) {
                    Some(rest) => {
                        items.push(parse_inlines(rest));
                        i += 1;
                    }
                    None => break,
                }
            }
            blocks.push(Block::BulletList { items });
        } else if parse_ordered(line).is_some() {
            let mut items = Vec::new();
            while i < lines.len() {
                match parse_ordered(lines[i]) {
                    Some(rest) => {
                        items.push(parse_inlines(rest));
                        i += 1;
                    }
                    None => break,
                }
            }
            blocks.push(Block::OrderedList { items });
        } else if let Some(rest) = line.strip_prefix("> ") {
            blocks.push(Block::Quote {
                inlines: parse_inlines(rest),
            });
            i += 1;
        } else if is_unsupported(line) {
            let mut raw = Vec::new();
            while i < lines.len() && !lines[i].trim().is_empty() && is_unsupported(lines[i]) {
                raw.push(lines[i]);
                i += 1;
            }
            blocks.push(Block::Raw {
                text: raw.join("\n"),
            });
        } else {
            blocks.push(Block::Paragraph {
                inlines: parse_inlines(line),
            });
            i += 1;
        }
    }
    Doc { blocks }
}

fn parse_heading(line: &str) -> Option<(u8, &str)> {
    let hashes = line.len() - line.trim_start_matches('#').len();
    if (1..=6).contains(&hashes)
        && let Some(rest) = line[hashes..].strip_prefix(' ')
    {
        return Some((hashes as u8, rest));
    }
    None
}

fn parse_task(line: &str) -> Option<(bool, &str)> {
    let rest = line.strip_prefix("- [")?;
    let checked = match rest.as_bytes().first()? {
        b' ' => false,
        b'x' | b'X' => true,
        _ => return None,
    };
    rest[1..].strip_prefix("] ").map(|r| (checked, r))
}

fn parse_bullet(line: &str) -> Option<&str> {
    if parse_task(line).is_some() {
        return None;
    }
    line.strip_prefix("- ").or_else(|| line.strip_prefix("* "))
}

fn parse_ordered(line: &str) -> Option<&str> {
    let digits = line.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 {
        return line[digits..].strip_prefix(". ");
    }
    None
}

/// Lines starting with these markers are kept verbatim in a `Raw` block.
fn is_unsupported(line: &str) -> bool {
    line.starts_with('|')          // tables
        || line.starts_with("![")  // images
        || line.starts_with('<')   // HTML
        || line.starts_with("[^")  // footnotes
        || line.starts_with('\t')  // indented (nesting / indented code)
        || line.starts_with("    ")
}

/// Parse a single line of inline Markdown. v1 handles one mark per span (no
/// nesting) and links; unmatched delimiters are treated as literal text.
fn parse_inlines(s: &str) -> Vec<Inline> {
    let mut out = Vec::new();
    let mut plain = String::new();
    let mut i = 0;
    while i < s.len() {
        if s.as_bytes()[i] == b'['
            && let Some((label, href, next)) = match_link(s, i)
        {
            flush(&mut plain, &mut out);
            out.push(Inline::Link {
                href: href.to_string(),
                inlines: parse_inlines(label),
            });
            i = next;
            continue;
        }
        if s[i..].starts_with("**")
            && let Some((inner, next)) = match_delim(s, i, "**")
        {
            flush(&mut plain, &mut out);
            out.push(run(inner, Marks::one(|m| m.bold = true)));
            i = next;
            continue;
        }
        if s[i..].starts_with("~~")
            && let Some((inner, next)) = match_delim(s, i, "~~")
        {
            flush(&mut plain, &mut out);
            out.push(run(inner, Marks::one(|m| m.strikethrough = true)));
            i = next;
            continue;
        }
        // Single '*' italic, but not the start of a '**' bold marker.
        if s.as_bytes()[i] == b'*'
            && !s[i..].starts_with("**")
            && let Some((inner, next)) = match_delim(s, i, "*")
        {
            flush(&mut plain, &mut out);
            out.push(run(inner, Marks::one(|m| m.italic = true)));
            i = next;
            continue;
        }
        if s.as_bytes()[i] == b'`'
            && let Some((inner, next)) = match_delim(s, i, "`")
        {
            flush(&mut plain, &mut out);
            out.push(run(inner, Marks::one(|m| m.code = true)));
            i = next;
            continue;
        }
        let ch = s[i..].chars().next().unwrap();
        plain.push(ch);
        i += ch.len_utf8();
    }
    flush(&mut plain, &mut out);
    out
}

fn flush(plain: &mut String, out: &mut Vec<Inline>) {
    if !plain.is_empty() {
        out.push(Inline::Run {
            text: std::mem::take(plain),
            marks: Marks::default(),
        });
    }
}

fn run(text: &str, marks: Marks) -> Inline {
    Inline::Run {
        text: text.to_string(),
        marks,
    }
}

/// Match `delim`...`delim` starting at `i`; return the inner text and the index
/// just past the closing delimiter. Empty spans are rejected.
fn match_delim<'a>(s: &'a str, i: usize, delim: &str) -> Option<(&'a str, usize)> {
    let start = i + delim.len();
    let close = s[start..].find(delim)? + start;
    if close == start {
        return None; // empty span, e.g. "**"
    }
    Some((&s[start..close], close + delim.len()))
}

/// Match `[label](href)` starting at `i`.
fn match_link(s: &str, i: usize) -> Option<(&str, &str, usize)> {
    let rest = &s[i + 1..];
    let close_label = rest.find(']')?;
    let after = &rest[close_label + 1..];
    if !after.starts_with('(') {
        return None;
    }
    let close_href = after.find(')')?;
    let label = &rest[..close_label];
    let href = &after[1..close_href];
    let next = i + 1 + close_label + 1 + 1 + close_href + 1;
    Some((label, href, next))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializing then parsing canonical Markdown is the identity on the text.
    fn roundtrip(md: &str) -> String {
        doc_to_markdown(&markdown_to_doc(md))
    }

    #[test]
    fn blocks_roundtrip() {
        let cases = [
            "# Titre",
            "## Sous-titre",
            "Un paragraphe simple.",
            "- puce un\n- puce deux",
            "1. un\n2. deux\n3. trois",
            "- [ ] à faire\n- [x] fait",
            "> une citation",
            "```\ncode\nverbatim\n```",
            "```rust\nfn main() {}\n```",
        ];
        for c in cases {
            assert_eq!(roundtrip(c), c, "roundtrip changed: {c:?}");
        }
    }

    #[test]
    fn inline_marks_roundtrip() {
        let cases = [
            "texte **gras** ici",
            "de l'*italique*",
            "du ~~barré~~",
            "un `bout de code`",
            "un [lien](https://exemple.org)",
        ];
        for c in cases {
            assert_eq!(roundtrip(c), c, "inline roundtrip changed: {c:?}");
        }
    }

    #[test]
    fn parses_inline_structure() {
        let doc = markdown_to_doc("a **b** c");
        assert_eq!(
            doc.blocks,
            vec![Block::Paragraph {
                inlines: vec![
                    Inline::Run {
                        text: "a ".into(),
                        marks: Marks::default()
                    },
                    Inline::Run {
                        text: "b".into(),
                        marks: Marks::one(|m| m.bold = true)
                    },
                    Inline::Run {
                        text: " c".into(),
                        marks: Marks::default()
                    },
                ],
            }]
        );
    }

    #[test]
    fn task_items_carry_checked_state() {
        let doc = markdown_to_doc("- [ ] a\n- [x] b");
        match &doc.blocks[0] {
            Block::TaskList { items } => {
                assert_eq!(items.len(), 2);
                assert!(!items[0].checked);
                assert!(items[1].checked);
            }
            other => panic!("expected TaskList, got {other:?}"),
        }
    }

    #[test]
    fn unsupported_is_preserved_as_raw() {
        // A table is outside the subset: kept verbatim.
        let table = "| a | b |\n| - | - |\n| 1 | 2 |";
        let doc = markdown_to_doc(table);
        assert_eq!(doc.blocks, vec![Block::Raw { text: table.into() }]);
        assert_eq!(roundtrip(table), table);
    }

    #[test]
    fn mixed_document_roundtrips() {
        let md = "# Titre\n\nUn paragraphe avec du **gras**.\n\n- [ ] tâche\n- [x] faite\n\n> note";
        assert_eq!(roundtrip(md), md);
    }

    #[test]
    fn ordered_list_is_renumbered() {
        // Non-canonical numbering normalizes to 1., 2., 3.
        assert_eq!(roundtrip("3. a\n7. b"), "1. a\n2. b");
    }

    #[test]
    fn empty_doc() {
        assert_eq!(doc_to_markdown(&Doc::default()), "");
        assert_eq!(markdown_to_doc(""), Doc::default());
    }
}
