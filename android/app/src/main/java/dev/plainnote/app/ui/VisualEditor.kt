package dev.plainnote.app.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.FormatListBulleted
import androidx.compose.material.icons.filled.CheckBox
import androidx.compose.material.icons.filled.FormatBold
import androidx.compose.material.icons.filled.FormatItalic
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.OffsetMapping
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.input.TransformedText
import androidx.compose.ui.text.input.VisualTransformation
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
 * on the current line, Enter splits at the caret, and Backspace at the start of
 * a line degrades its type then merges into the previous line.
 *
 * v0 flattens inline marks to plain text on edit — a later increment restores
 * non-destructive, displayed marks.
 */
@Composable
fun VisualEditor(initialMarkdown: String, onBodyChange: (String) -> Unit) {
    var lines by remember { mutableStateOf(docToLines(markdownToDoc(initialMarkdown))) }
    var nextId by remember { mutableStateOf(lines.size.toLong()) }
    var focused by remember { mutableStateOf(0) }
    var focusedValue by remember { mutableStateOf(TextFieldValue()) }
    var pendingFocus by remember { mutableStateOf<Long?>(null) }
    val focusRequester = remember { FocusRequester() }

    LaunchedEffect(pendingFocus) {
        if (pendingFocus != null) {
            runCatching { focusRequester.requestFocus() }
            pendingFocus = null
        }
    }

    fun commit(newLines: List<Line>) {
        lines = newLines
        onBodyChange(docToMarkdown(Doc(linesToBlocks(newLines))))
    }
    fun setText(index: Int, text: String) =
        commit(lines.toMutableList().also { it[index] = it[index].copy(text = text) })
    fun freshId(): Long { val v = nextId; nextId = v + 1; return v }

    fun split(index: Int, before: String, after: String) {
        val cur = lines[index]
        val nextKind = when (cur.kind) {
            LineKind.Bullet, LineKind.Ordered, LineKind.Task -> cur.kind
            else -> LineKind.Paragraph
        }
        val updated = lines.toMutableList()
        updated[index] = cur.copy(text = before)
        val newLine = Line(freshId(), nextKind, after)
        updated.add(index + 1, newLine)
        commit(updated)
        pendingFocus = newLine.id
    }

    /** Returns true if the Backspace was handled (degrade or merge). */
    fun backspaceAtStart(index: Int): Boolean {
        val cur = lines[index]
        if (cur.kind != LineKind.Paragraph) {
            commit(lines.toMutableList().also {
                it[index] = cur.copy(kind = LineKind.Paragraph, level = 1, checked = false)
            })
            return true
        }
        if (index == 0) return false
        val prev = lines[index - 1]
        if (prev.kind == LineKind.Code || prev.kind == LineKind.Raw) return false
        val updated = lines.toMutableList()
        updated[index - 1] = prev.copy(text = prev.text + cur.text)
        updated.removeAt(index)
        commit(updated)
        pendingFocus = prev.id
        return true
    }

    fun setKind(kind: LineKind) {
        val l = lines.getOrNull(focused) ?: return
        commit(lines.toMutableList().also {
            it[focused] = l.copy(
                kind = kind,
                level = if (kind == LineKind.Heading) 1 else l.level,
                checked = if (kind == LineKind.Task) l.checked else false,
            )
        })
    }

    // No imePadding here: the enclosing editor already applies it (avoids double padding).
    Column(Modifier.fillMaxSize()) {
        Column(
            Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(16.dp, 8.dp),
        ) {
            var ordinal = 0
            lines.forEachIndexed { index, line ->
                ordinal = if (line.kind == LineKind.Ordered) ordinal + 1 else 0
                LineRow(
                    line = line,
                    ordinal = ordinal,
                    focusModifier = if (line.id == pendingFocus) Modifier.focusRequester(focusRequester) else Modifier,
                    onFocused = { focused = index },
                    onValue = { focusedValue = it },
                    onText = { setText(index, it) },
                    onSplit = { before, after -> split(index, before, after) },
                    onBackspaceAtStart = { backspaceAtStart(index) },
                    onToggle = { commit(lines.toMutableList().also { l -> l[index] = line.copy(checked = it) }) },
                )
            }
        }
        FormatBar(
            onParagraph = { setKind(LineKind.Paragraph) },
            onHeading = { setKind(LineKind.Heading) },
            onBullet = { setKind(LineKind.Bullet) },
            onTask = { setKind(LineKind.Task) },
            onBold = { setText(focused, Markdown.wrap(focusedValue, "**").text) },
            onItalic = { setText(focused, Markdown.wrap(focusedValue, "*").text) },
        )
    }
}

@Composable
private fun LineRow(
    line: Line,
    ordinal: Int,
    focusModifier: Modifier,
    onFocused: () -> Unit,
    onValue: (TextFieldValue) -> Unit,
    onText: (String) -> Unit,
    onSplit: (String, String) -> Unit,
    onBackspaceAtStart: () -> Boolean,
    onToggle: (Boolean) -> Unit,
) {
    // Caret-aware value; re-synced when the line's text changes externally (merge).
    var tfv by remember(line.id) { mutableStateOf(TextFieldValue(line.text, TextRange(line.text.length))) }
    LaunchedEffect(line.text) {
        if (tfv.text != line.text) tfv = TextFieldValue(line.text, TextRange(line.text.length))
    }

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
                value = tfv,
                onValueChange = { v ->
                    val nl = v.text.indexOf('\n')
                    if (nl >= 0) {
                        onSplit(v.text.substring(0, nl), v.text.substring(nl + 1))
                    } else {
                        tfv = v
                        onValue(v)
                        onText(v.text)
                    }
                },
                textStyle = style,
                visualTransformation = hideMarkers(MaterialTheme.colorScheme.primary),
                cursorBrush = SolidColor(MaterialTheme.colorScheme.primary),
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(vertical = 6.dp)
                    .onFocusChanged { if (it.isFocused) { onFocused(); onValue(tfv) } }
                    .onPreviewKeyEvent { ev ->
                        ev.type == KeyEventType.KeyDown &&
                            ev.key == Key.Backspace &&
                            tfv.selection.start == 0 &&
                            tfv.selection.collapsed &&
                            onBackspaceAtStart()
                    }
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
    onBold: () -> Unit,
    onItalic: () -> Unit,
) {
    Surface(tonalElevation = 2.dp) {
        Row(Modifier.fillMaxWidth().padding(4.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onBold) {
                Icon(Icons.Filled.FormatBold, contentDescription = "Gras")
            }
            IconButton(onClick = onItalic) {
                Icon(Icons.Filled.FormatItalic, contentDescription = "Italique")
            }
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

private const val M_BOLD = 1
private const val M_ITALIC = 2
private const val M_CODE = 4
private const val M_STRIKE = 8
private const val M_LINK = 16

// Longest markers first so `***`/`**` win over `*`.
private val MARKERS = listOf(
    "***" to (M_BOLD or M_ITALIC),
    "**" to M_BOLD,
    "~~" to M_STRIKE,
    "*" to M_ITALIC,
    "`" to M_CODE,
)

/**
 * True-WYSIWYG transform (3B, option A): inline markers are hidden entirely and
 * their content is styled. A pure function of the text (no caret-based reveal),
 * with a non-identity [OffsetMapping] so the caret skips the hidden markers
 * instead of resting on them. An unterminated marker is left literal, unstyled.
 */
private fun hideMarkers(linkColor: Color) =
    VisualTransformation { text -> transformHidingMarkers(text.text, linkColor) }

internal fun transformHidingMarkers(src: String, linkColor: Color = Color.Unspecified): TransformedText {
    val n = src.length
    val dropped = BooleanArray(n)
    val flags = IntArray(n)

    // Recursive descent so nested marks (e.g. `***x***` = bold+italic) stack.
    fun parse(lo: Int, hi: Int, base: Int) {
        var i = lo
        while (i < hi) {
            // Link `[label](url)`: keep the label (styled), hide `[`, `]`, `(url)`.
            // Malformed (no `(url)`, empty label) falls through and stays literal.
            if (src[i] == '[') {
                val rb = src.indexOf(']', i + 1)
                if (rb in (i + 2) until hi && rb + 1 < hi && src[rb + 1] == '(') {
                    val rp = src.indexOf(')', rb + 2)
                    if (rp in (rb + 2)..(hi - 1)) {
                        dropped[i] = true
                        parse(i + 1, rb, base or M_LINK)
                        for (k in rb..rp) dropped[k] = true
                        i = rp + 1
                        continue
                    }
                }
            }
            // A marker matches only with a non-empty content and a closing token
            // that fits before `hi` — so `**` alone stays literal, unstyled.
            val hit = MARKERS.firstOrNull { (m, _) ->
                src.startsWith(m, i) &&
                    src.indexOf(m, i + m.length).let { it in (i + m.length + 1)..(hi - m.length) }
            }
            if (hit != null) {
                val (m, bit) = hit
                val close = src.indexOf(m, i + m.length)
                for (k in i until i + m.length) dropped[k] = true
                parse(i + m.length, close, base or bit)
                for (k in close until close + m.length) dropped[k] = true
                i = close + m.length
                continue
            }
            flags[i] = base
            i++
        }
    }
    parse(0, n, 0)

    val sb = StringBuilder()
    val o2t = IntArray(n + 1)
    val spans = mutableListOf<Triple<Int, Int, Int>>() // transStart, transEnd, flags
    var runFlags = -1
    var runStart = 0
    for (i in 0 until n) {
        if (!dropped[i]) {
            val f = flags[i]
            if (f != runFlags) {
                if (runFlags > 0 && sb.length > runStart) spans.add(Triple(runStart, sb.length, runFlags))
                runFlags = f
                runStart = sb.length
            }
            sb.append(src[i])
        }
        o2t[i + 1] = sb.length
    }
    if (runFlags > 0 && sb.length > runStart) spans.add(Triple(runStart, sb.length, runFlags))

    val ann = buildAnnotatedString {
        append(sb.toString())
        for ((start, end, f) in spans) addStyle(spanFor(f, linkColor), start, end)
    }
    val mapping = object : OffsetMapping {
        override fun originalToTransformed(offset: Int): Int = o2t[offset.coerceIn(0, n)]
        override fun transformedToOriginal(offset: Int): Int {
            val t = offset.coerceIn(0, sb.length)
            // Last source index mapping to t: places the caret past hidden markers.
            var s = 0
            for (i in 0..n) if (o2t[i] == t) s = i
            return s
        }
    }
    return TransformedText(ann, mapping)
}

private fun spanFor(f: Int, linkColor: Color) = SpanStyle(
    fontWeight = if (f and M_BOLD != 0) FontWeight.Bold else null,
    fontStyle = if (f and M_ITALIC != 0) FontStyle.Italic else null,
    fontFamily = if (f and M_CODE != 0) FontFamily.Monospace else null,
    color = if (f and M_LINK != 0) linkColor else Color.Unspecified,
    textDecoration = when {
        f and M_STRIKE != 0 && f and M_LINK != 0 -> androidx.compose.ui.text.style.TextDecoration.combine(
            listOf(
                androidx.compose.ui.text.style.TextDecoration.LineThrough,
                androidx.compose.ui.text.style.TextDecoration.Underline,
            ),
        )
        f and M_STRIKE != 0 -> androidx.compose.ui.text.style.TextDecoration.LineThrough
        f and M_LINK != 0 -> androidx.compose.ui.text.style.TextDecoration.Underline
        else -> null
    },
)

/**
 * Read-only rendered view of a note: the same block model, styled, with inline
 * markers stripped (true rendered look — no editing, so no caret mapping needed).
 */
@Composable
fun DocView(markdown: String, modifier: Modifier = Modifier) {
    // markdownToDoc crosses UniFFI (native lib); DocLines is pure so it can be
    // rendered under Robolectric in screenshot tests with hand-built lines.
    val lines = remember(markdown) { docToLines(markdownToDoc(markdown)) }
    DocLines(lines, modifier)
}

@Composable
internal fun DocLines(lines: List<Line>, modifier: Modifier = Modifier) {
    Column(modifier.verticalScroll(rememberScrollState()).padding(16.dp, 8.dp)) {
        var ordinal = 0
        lines.forEach { line ->
            ordinal = if (line.kind == LineKind.Ordered) ordinal + 1 else 0
            ViewRow(line, ordinal)
        }
    }
}

@Composable
private fun ViewRow(line: Line, ordinal: Int) {
    val linkColor = MaterialTheme.colorScheme.primary
    when (line.kind) {
        LineKind.Code, LineKind.Raw -> Text(
            line.text,
            fontFamily = FontFamily.Monospace,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.fillMaxWidth().padding(vertical = 6.dp),
        )
        LineKind.Task -> Row(
            Modifier.fillMaxWidth().padding(vertical = 2.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Checkbox(checked = line.checked, onCheckedChange = null)
            Text(
                renderInline(line.text, linkColor),
                color = MaterialTheme.colorScheme.onSurface,
                modifier = Modifier.padding(start = 4.dp),
            )
        }
        else -> Row(Modifier.fillMaxWidth().padding(vertical = 4.dp)) {
            val prefix = when (line.kind) {
                LineKind.Bullet -> "•  "
                LineKind.Ordered -> "$ordinal.  "
                LineKind.Quote -> "│  "
                else -> ""
            }
            if (prefix.isNotEmpty()) Text(prefix, color = MaterialTheme.colorScheme.onSurfaceVariant)
            Text(
                renderInline(line.text, linkColor),
                color = MaterialTheme.colorScheme.onSurface,
                fontSize = if (line.kind == LineKind.Heading) headingSize(line.level) else LocalTextStyle.current.fontSize,
                fontWeight = if (line.kind == LineKind.Heading) FontWeight.Bold else null,
                fontStyle = if (line.kind == LineKind.Quote) FontStyle.Italic else null,
            )
        }
    }
}

/**
 * Inline Markdown → styled text with markers stripped, for the read-only view.
 * Delegates to the editor's transform so both render identically.
 */
private fun renderInline(text: String, linkColor: Color): AnnotatedString =
    transformHidingMarkers(text, linkColor).text

private fun headingSize(level: Int) = when (level) {
    1 -> 24.sp
    2 -> 20.sp
    else -> 18.sp
}

// --- flat line model <-> core Doc ---

internal enum class LineKind { Paragraph, Heading, Bullet, Ordered, Task, Quote, Code, Raw }

internal data class Line(
    val id: Long,
    val kind: LineKind,
    val text: String,
    val checked: Boolean = false,
    val level: Int = 1,
)

internal fun docToLines(doc: Doc): List<Line> {
    var id = 0L
    fun next() = id++
    return buildList {
        for (b in doc.blocks) when (b) {
            is Block.Heading -> add(Line(next(), LineKind.Heading, inlineMarkdown(b.inlines), level = b.level.toInt()))
            is Block.Paragraph -> add(Line(next(), LineKind.Paragraph, inlineMarkdown(b.inlines)))
            is Block.BulletList -> b.items.forEach { add(Line(next(), LineKind.Bullet, inlineMarkdown(it.inlines))) }
            is Block.OrderedList -> b.items.forEach { add(Line(next(), LineKind.Ordered, inlineMarkdown(it.inlines))) }
            is Block.TaskList -> b.items.forEach { add(Line(next(), LineKind.Task, inlineMarkdown(it.inlines), it.checked)) }
            is Block.Quote -> add(Line(next(), LineKind.Quote, inlineMarkdown(b.inlines)))
            is Block.CodeBlock -> add(Line(next(), LineKind.Code, b.text))
            is Block.Raw -> add(Line(next(), LineKind.Raw, b.text))
        }
    }.ifEmpty { listOf(Line(0, LineKind.Paragraph, "")) }
}

internal fun linesToBlocks(lines: List<Line>): List<Block> {
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

internal fun plainRun(text: String): List<Inline> =
    listOf(Inline.Run(text, Marks(bold = false, italic = false, strikethrough = false, code = false)))

/** Serialize inlines back to canonical inline Markdown (marks preserved). */
internal fun inlineMarkdown(inlines: List<Inline>): String = buildString {
    for (i in inlines) when (i) {
        is Inline.Run -> append(applyMarks(i.text, i.marks))
        is Inline.Link -> append("[").append(inlineMarkdown(i.inlines)).append("](").append(i.href).append(")")
    }
}

internal fun applyMarks(text: String, m: Marks): String {
    var s = text
    if (m.code) s = "`$s`"
    if (m.strikethrough) s = "~~$s~~"
    if (m.italic) s = "*$s*"
    if (m.bold) s = "**$s**"
    return s
}
