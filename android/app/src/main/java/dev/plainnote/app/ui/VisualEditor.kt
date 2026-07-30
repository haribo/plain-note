package dev.plainnote.app.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.FormatListBulleted
import androidx.compose.material.icons.filled.CheckBox
import androidx.compose.material.icons.filled.Notes
import androidx.compose.material.icons.filled.Title
import androidx.compose.material3.Checkbox
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.plainnote.core.Block
import dev.plainnote.core.Doc
import dev.plainnote.core.Inline
import dev.plainnote.core.ListItem
import dev.plainnote.core.Marks
import dev.plainnote.core.TaskItem
import dev.plainnote.core.docToMarkdown
import dev.plainnote.core.markdownToDoc

/**
 * Visual (WYSIWYG) editor over the core document model. Internally a flat list
 * of lines (converted to/from the `Doc` on load/save); the format toolbar acts
 * on the current line and Enter adds a line. Storage stays Markdown.
 *
 * v0 flattens inline marks to plain text on edit — a later increment restores
 * non-destructive, displayed marks.
 */
@Composable
fun VisualEditor(initialMarkdown: String, onBodyChange: (String) -> Unit) {
    var lines by remember { mutableStateOf(docToLines(markdownToDoc(initialMarkdown))) }
    var focused by remember { mutableStateOf(0) }
    var pendingFocus by remember { mutableStateOf<Int?>(null) }
    val focusRequester = remember { FocusRequester() }

    LaunchedEffect(pendingFocus) {
        pendingFocus?.let {
            runCatching { focusRequester.requestFocus() }
            pendingFocus = null
        }
    }

    fun commit(newLines: List<Line>) {
        lines = newLines
        onBodyChange(docToMarkdown(Doc(linesToBlocks(newLines))))
    }

    fun setLine(index: Int, line: Line) =
        commit(lines.toMutableList().also { it[index] = line })

    /** Handle a text change, splitting into a new line when Enter is pressed. */
    fun onLineText(index: Int, newText: String) {
        val nl = newText.indexOf('\n')
        if (nl < 0) {
            setLine(index, lines[index].copy(text = newText))
            return
        }
        val before = newText.substring(0, nl)
        val after = newText.substring(nl + 1)
        val cur = lines[index]
        // Continue lists; otherwise the new line is a paragraph.
        val nextKind = when (cur.kind) {
            LineKind.Bullet, LineKind.Ordered, LineKind.Task -> cur.kind
            else -> LineKind.Paragraph
        }
        val updated = lines.toMutableList()
        updated[index] = cur.copy(text = before)
        updated.add(index + 1, Line(nextKind, after))
        commit(updated)
        pendingFocus = index + 1
    }

    fun setKind(kind: LineKind) {
        val l = lines.getOrNull(focused) ?: return
        setLine(
            focused,
            l.copy(
                kind = kind,
                level = if (kind == LineKind.Heading) 1 else l.level,
                checked = if (kind == LineKind.Task) l.checked else false,
            ),
        )
    }

    Column(Modifier.fillMaxSize().imePadding()) {
        Column(
            Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(16.dp, 8.dp),
        ) {
            var ordinal = 0
            lines.forEachIndexed { index, line ->
                ordinal = if (line.kind == LineKind.Ordered) ordinal + 1 else 0
                LineRow(
                    line = line,
                    ordinal = ordinal,
                    focusModifier = if (index == pendingFocus) Modifier.focusRequester(focusRequester) else Modifier,
                    onFocused = { focused = index },
                    onText = { onLineText(index, it) },
                    onToggle = { setLine(index, line.copy(checked = it)) },
                )
            }
        }
        FormatBar(
            onParagraph = { setKind(LineKind.Paragraph) },
            onHeading = { setKind(LineKind.Heading) },
            onBullet = { setKind(LineKind.Bullet) },
            onTask = { setKind(LineKind.Task) },
        )
    }
}

@Composable
private fun LineRow(
    line: Line,
    ordinal: Int,
    focusModifier: Modifier,
    onFocused: () -> Unit,
    onText: (String) -> Unit,
    onToggle: (Boolean) -> Unit,
) {
    Row(Modifier.fillMaxWidth().padding(vertical = 2.dp), verticalAlignment = Alignment.CenterVertically) {
        when (line.kind) {
            LineKind.Task -> Checkbox(checked = line.checked, onCheckedChange = onToggle)
            LineKind.Bullet -> Marker("•  ")
            LineKind.Ordered -> Marker("$ordinal.  ")
            LineKind.Quote -> Marker("│  ")
            else -> {}
        }
        if (line.kind == LineKind.Code || line.kind == LineKind.Raw) {
            Text(
                line.text,
                fontFamily = FontFamily.Monospace,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.fillMaxWidth().padding(vertical = 8.dp),
            )
        } else {
            val style = LocalTextStyle.current.copy(
                color = MaterialTheme.colorScheme.onSurface,
                fontSize = if (line.kind == LineKind.Heading) headingSize(line.level) else LocalTextStyle.current.fontSize,
                fontWeight = if (line.kind == LineKind.Heading) FontWeight.Bold else null,
                fontStyle = if (line.kind == LineKind.Quote) FontStyle.Italic else null,
            )
            BasicTextField(
                value = line.text,
                onValueChange = onText,
                textStyle = style,
                cursorBrush = SolidColor(MaterialTheme.colorScheme.primary),
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(vertical = 6.dp)
                    .onFocusChanged { if (it.isFocused) onFocused() }
                    .then(focusModifier),
            )
        }
    }
}

@Composable
private fun Marker(text: String) {
    Text(text, color = MaterialTheme.colorScheme.onSurfaceVariant)
}

@Composable
private fun FormatBar(
    onParagraph: () -> Unit,
    onHeading: () -> Unit,
    onBullet: () -> Unit,
    onTask: () -> Unit,
) {
    Surface(tonalElevation = 2.dp) {
        Row(Modifier.fillMaxWidth().padding(4.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onParagraph) {
                Icon(Icons.Filled.Notes, contentDescription = "Paragraphe")
            }
            IconButton(onClick = onHeading) {
                Icon(Icons.Filled.Title, contentDescription = "Titre")
            }
            IconButton(onClick = onBullet) {
                Icon(Icons.AutoMirrored.Filled.FormatListBulleted, contentDescription = "Liste à puces")
            }
            IconButton(onClick = onTask) {
                Icon(Icons.Filled.CheckBox, contentDescription = "Case à cocher")
            }
        }
    }
}

private fun headingSize(level: Int) = when (level) {
    1 -> 24.sp
    2 -> 20.sp
    else -> 18.sp
}

// --- flat line model <-> core Doc ---

private enum class LineKind { Paragraph, Heading, Bullet, Ordered, Task, Quote, Code, Raw }

private data class Line(
    val kind: LineKind,
    val text: String,
    val checked: Boolean = false,
    val level: Int = 1,
)

private fun docToLines(doc: Doc): List<Line> = buildList {
    for (b in doc.blocks) when (b) {
        is Block.Heading -> add(Line(LineKind.Heading, inlineText(b.inlines), level = b.level.toInt()))
        is Block.Paragraph -> add(Line(LineKind.Paragraph, inlineText(b.inlines)))
        is Block.BulletList -> b.items.forEach { add(Line(LineKind.Bullet, inlineText(it.inlines))) }
        is Block.OrderedList -> b.items.forEach { add(Line(LineKind.Ordered, inlineText(it.inlines))) }
        is Block.TaskList -> b.items.forEach { add(Line(LineKind.Task, inlineText(it.inlines), it.checked)) }
        is Block.Quote -> add(Line(LineKind.Quote, inlineText(b.inlines)))
        is Block.CodeBlock -> add(Line(LineKind.Code, b.text))
        is Block.Raw -> add(Line(LineKind.Raw, b.text))
    }
}.ifEmpty { listOf(Line(LineKind.Paragraph, "")) }

private fun linesToBlocks(lines: List<Line>): List<Block> {
    val blocks = mutableListOf<Block>()
    var i = 0
    while (i < lines.size) {
        val l = lines[i]
        when (l.kind) {
            LineKind.Heading -> { blocks.add(Block.Heading(l.level.toUByte(), plainRun(l.text))); i++ }
            LineKind.Paragraph -> { blocks.add(Block.Paragraph(plainRun(l.text))); i++ }
            LineKind.Quote -> { blocks.add(Block.Quote(plainRun(l.text))); i++ }
            LineKind.Code -> { blocks.add(Block.CodeBlock(l.text, null)); i++ }
            LineKind.Raw -> { blocks.add(Block.Raw(l.text)); i++ }
            LineKind.Bullet -> {
                val items = mutableListOf<ListItem>()
                while (i < lines.size && lines[i].kind == LineKind.Bullet) { items.add(ListItem(plainRun(lines[i].text))); i++ }
                blocks.add(Block.BulletList(items))
            }
            LineKind.Ordered -> {
                val items = mutableListOf<ListItem>()
                while (i < lines.size && lines[i].kind == LineKind.Ordered) { items.add(ListItem(plainRun(lines[i].text))); i++ }
                blocks.add(Block.OrderedList(items))
            }
            LineKind.Task -> {
                val items = mutableListOf<TaskItem>()
                while (i < lines.size && lines[i].kind == LineKind.Task) { items.add(TaskItem(lines[i].checked, plainRun(lines[i].text))); i++ }
                blocks.add(Block.TaskList(items))
            }
        }
    }
    return blocks
}

private fun plainRun(text: String): List<Inline> =
    listOf(Inline.Run(text, Marks(bold = false, italic = false, strikethrough = false, code = false)))

/** Flatten inline content to plain text (v0: marks/link styling dropped on edit). */
private fun inlineText(inlines: List<Inline>): String = buildString {
    for (i in inlines) when (i) {
        is Inline.Run -> append(i.text)
        is Inline.Link -> append(inlineText(i.inlines))
    }
}
