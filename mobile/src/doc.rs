//! UniFFI mirror of [`note_core::doc`] plus the Markdown round-trip functions.
//!
//! `core` stays UniFFI-free, so the model is mirrored here with `From`
//! conversions. The Android editor works on this `Doc`; storage stays Markdown.

use note_core::doc as core;

#[derive(uniffi::Record)]
pub struct Doc {
    pub blocks: Vec<Block>,
}

#[derive(uniffi::Enum)]
pub enum Block {
    Heading { level: u8, inlines: Vec<Inline> },
    Paragraph { inlines: Vec<Inline> },
    BulletList { items: Vec<ListItem> },
    OrderedList { items: Vec<ListItem> },
    TaskList { items: Vec<TaskItem> },
    Quote { inlines: Vec<Inline> },
    CodeBlock { text: String, lang: Option<String> },
    Raw { text: String },
}

/// A plain list item (bullet/ordered). Wraps inlines so the type crosses UniFFI.
#[derive(uniffi::Record)]
pub struct ListItem {
    pub inlines: Vec<Inline>,
}

#[derive(uniffi::Record)]
pub struct TaskItem {
    pub checked: bool,
    pub inlines: Vec<Inline>,
}

#[derive(uniffi::Enum)]
pub enum Inline {
    Run { text: String, marks: Marks },
    Link { href: String, inlines: Vec<Inline> },
}

#[derive(uniffi::Record)]
pub struct Marks {
    pub bold: bool,
    pub italic: bool,
    pub strikethrough: bool,
    pub code: bool,
}

/// Parse Markdown into a structured document.
#[uniffi::export]
pub fn markdown_to_doc(md: String) -> Doc {
    core::markdown_to_doc(&md).into()
}

/// Serialize a document back to canonical Markdown.
#[uniffi::export]
pub fn doc_to_markdown(doc: Doc) -> String {
    core::doc_to_markdown(&doc.into())
}

// --- conversions core <-> mobile ---

fn inlines_in(v: Vec<core::Inline>) -> Vec<Inline> {
    v.into_iter().map(Into::into).collect()
}
fn inlines_out(v: Vec<Inline>) -> Vec<core::Inline> {
    v.into_iter().map(Into::into).collect()
}

impl From<core::Marks> for Marks {
    fn from(m: core::Marks) -> Self {
        Marks {
            bold: m.bold,
            italic: m.italic,
            strikethrough: m.strikethrough,
            code: m.code,
        }
    }
}
impl From<Marks> for core::Marks {
    fn from(m: Marks) -> Self {
        core::Marks {
            bold: m.bold,
            italic: m.italic,
            strikethrough: m.strikethrough,
            code: m.code,
        }
    }
}

impl From<core::Inline> for Inline {
    fn from(i: core::Inline) -> Self {
        match i {
            core::Inline::Run { text, marks } => Inline::Run {
                text,
                marks: marks.into(),
            },
            core::Inline::Link { href, inlines } => Inline::Link {
                href,
                inlines: inlines_in(inlines),
            },
        }
    }
}
impl From<Inline> for core::Inline {
    fn from(i: Inline) -> Self {
        match i {
            Inline::Run { text, marks } => core::Inline::Run {
                text,
                marks: marks.into(),
            },
            Inline::Link { href, inlines } => core::Inline::Link {
                href,
                inlines: inlines_out(inlines),
            },
        }
    }
}

impl From<core::TaskItem> for TaskItem {
    fn from(t: core::TaskItem) -> Self {
        TaskItem {
            checked: t.checked,
            inlines: inlines_in(t.inlines),
        }
    }
}
impl From<TaskItem> for core::TaskItem {
    fn from(t: TaskItem) -> Self {
        core::TaskItem {
            checked: t.checked,
            inlines: inlines_out(t.inlines),
        }
    }
}

impl From<core::Block> for Block {
    fn from(b: core::Block) -> Self {
        match b {
            core::Block::Heading { level, inlines } => Block::Heading {
                level,
                inlines: inlines_in(inlines),
            },
            core::Block::Paragraph { inlines } => Block::Paragraph {
                inlines: inlines_in(inlines),
            },
            core::Block::BulletList { items } => Block::BulletList {
                items: items
                    .into_iter()
                    .map(|it| ListItem {
                        inlines: inlines_in(it),
                    })
                    .collect(),
            },
            core::Block::OrderedList { items } => Block::OrderedList {
                items: items
                    .into_iter()
                    .map(|it| ListItem {
                        inlines: inlines_in(it),
                    })
                    .collect(),
            },
            core::Block::TaskList { items } => Block::TaskList {
                items: items.into_iter().map(Into::into).collect(),
            },
            core::Block::Quote { inlines } => Block::Quote {
                inlines: inlines_in(inlines),
            },
            core::Block::CodeBlock { text, lang } => Block::CodeBlock { text, lang },
            core::Block::Raw { text } => Block::Raw { text },
        }
    }
}
impl From<Block> for core::Block {
    fn from(b: Block) -> Self {
        match b {
            Block::Heading { level, inlines } => core::Block::Heading {
                level,
                inlines: inlines_out(inlines),
            },
            Block::Paragraph { inlines } => core::Block::Paragraph {
                inlines: inlines_out(inlines),
            },
            Block::BulletList { items } => core::Block::BulletList {
                items: items
                    .into_iter()
                    .map(|it| inlines_out(it.inlines))
                    .collect(),
            },
            Block::OrderedList { items } => core::Block::OrderedList {
                items: items
                    .into_iter()
                    .map(|it| inlines_out(it.inlines))
                    .collect(),
            },
            Block::TaskList { items } => core::Block::TaskList {
                items: items.into_iter().map(Into::into).collect(),
            },
            Block::Quote { inlines } => core::Block::Quote {
                inlines: inlines_out(inlines),
            },
            Block::CodeBlock { text, lang } => core::Block::CodeBlock { text, lang },
            Block::Raw { text } => core::Block::Raw { text },
        }
    }
}

impl From<core::Doc> for Doc {
    fn from(d: core::Doc) -> Self {
        Doc {
            blocks: d.blocks.into_iter().map(Into::into).collect(),
        }
    }
}
impl From<Doc> for core::Doc {
    fn from(d: Doc) -> Self {
        core::Doc {
            blocks: d.blocks.into_iter().map(Into::into).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions_round_trip_through_markdown() {
        let md = "# Titre\n\nUn **gras** ici\n\n- [ ] a\n- [x] b";
        let doc = markdown_to_doc(md.to_string());
        assert_eq!(doc_to_markdown(doc), md);
    }
}
