package dev.plainnote.app.ui

import dev.plainnote.core.Block
import dev.plainnote.core.Doc
import dev.plainnote.core.Inline
import dev.plainnote.core.Marks
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * JVM unit tests for the pure Kotlin editor transforms. They only construct
 * UniFFI data classes and call the transforms — the native lib loads lazily on
 * the first FFI call, which never happens here, so no device/`.so` is needed.
 *
 * The full Markdown round-trip (`markdownToDoc`/`docToMarkdown`) is covered on
 * the Rust side (`core`, `mobile`); here we cover the Kotlin block <-> line and
 * inline-serialization logic.
 */
class EditorTransformsTest {

    private fun marks(bold: Boolean = false, italic: Boolean = false, strike: Boolean = false, code: Boolean = false) =
        Marks(bold = bold, italic = italic, strikethrough = strike, code = code)

    // A comparable projection of a line, ignoring its volatile id.
    private data class L(val kind: LineKind, val text: String, val checked: Boolean, val level: Int)

    private fun project(lines: List<Line>) = lines.map { L(it.kind, it.text, it.checked, it.level) }

    @Test
    fun applyMarks_no_marks_is_verbatim() {
        assertEquals("x", applyMarks("x", marks()))
    }

    @Test
    fun applyMarks_single_marks() {
        assertEquals("**x**", applyMarks("x", marks(bold = true)))
        assertEquals("*x*", applyMarks("x", marks(italic = true)))
        assertEquals("~~x~~", applyMarks("x", marks(strike = true)))
        assertEquals("`x`", applyMarks("x", marks(code = true)))
    }

    @Test
    fun applyMarks_nesting_order_matches_core() {
        // Order is code -> strike -> italic -> bold (bold outermost).
        assertEquals("***x***", applyMarks("x", marks(bold = true, italic = true)))
        assertEquals("**`x`**", applyMarks("x", marks(bold = true, code = true)))
        assertEquals("***~~`x`~~***", applyMarks("x", marks(bold = true, italic = true, strike = true, code = true)))
    }

    @Test
    fun inlineMarkdown_serializes_runs_and_links() {
        val inlines = listOf(
            Inline.Run("a ", marks()),
            Inline.Run("b", marks(bold = true)),
            Inline.Link("https://x.dev", listOf(Inline.Run("link", marks()))),
        )
        assertEquals("a **b**[link](https://x.dev)", inlineMarkdown(inlines))
    }

    @Test
    fun linesToBlocks_groups_consecutive_list_items() {
        val lines = listOf(
            Line(0, LineKind.Bullet, "a"),
            Line(1, LineKind.Bullet, "b"),
            Line(2, LineKind.Paragraph, "sep"),
            Line(3, LineKind.Task, "t", checked = true),
            Line(4, LineKind.Task, "u"),
        )
        val blocks = linesToBlocks(lines)
        assertEquals(3, blocks.size)
        val bullet = blocks[0] as Block.BulletList
        assertEquals(2, bullet.items.size)
        assertTrue(blocks[1] is Block.Paragraph)
        val tasks = blocks[2] as Block.TaskList
        assertEquals(2, tasks.items.size)
        assertTrue(tasks.items[0].checked)
        assertTrue(!tasks.items[1].checked)
    }

    @Test
    fun lines_round_trip_through_blocks() {
        val lines = listOf(
            Line(0, LineKind.Heading, "Titre", level = 2),
            Line(1, LineKind.Paragraph, "un **gras** ici"),
            Line(2, LineKind.Bullet, "a"),
            Line(3, LineKind.Bullet, "b"),
            Line(4, LineKind.Ordered, "one"),
            Line(5, LineKind.Ordered, "two"),
            Line(6, LineKind.Task, "done", checked = true),
            Line(7, LineKind.Task, "todo"),
            Line(8, LineKind.Quote, "cite"),
            Line(9, LineKind.Code, "let x = 1"),
            Line(10, LineKind.Raw, "| a | b |"),
        )
        val back = docToLines(Doc(linesToBlocks(lines)))
        assertEquals(project(lines), project(back))
    }

    @Test
    fun empty_doc_yields_one_empty_paragraph() {
        val lines = docToLines(Doc(emptyList()))
        assertEquals(1, lines.size)
        assertEquals(LineKind.Paragraph, lines[0].kind)
        assertEquals("", lines[0].text)
    }

    // --- hide-markers transform (3B) ---

    @Test
    fun hide_removes_inline_markers() {
        assertEquals("Un gras ici", transformHidingMarkers("Un **gras** ici").text.text)
        assertEquals("a b c", transformHidingMarkers("a *b* `c`").text.text)
        assertEquals("barre", transformHidingMarkers("~~barre~~").text.text)
    }

    @Test
    fun hide_stacks_nested_bold_italic() {
        assertEquals("x", transformHidingMarkers("***x***").text.text)
    }

    @Test
    fun hide_keeps_unterminated_marker_literal() {
        // No closing token: the markers stay visible, unstyled.
        assertEquals("un **gras", transformHidingMarkers("un **gras").text.text)
        assertEquals("a * b", transformHidingMarkers("a * b").text.text)
    }

    @Test
    fun hide_offset_mapping_skips_markers() {
        val t = transformHidingMarkers("a **b** c") // -> "a b c"
        assertEquals("a b c", t.text.text)
        // Source end maps to transformed end.
        assertEquals(t.text.text.length, t.offsetMapping.originalToTransformed("a **b** c".length))
        // Transformed 'b' maps back onto the content char, never onto a hidden marker.
        val src = t.offsetMapping.transformedToOriginal(2)
        assertEquals('b', "a **b** c"[src])
    }
}
