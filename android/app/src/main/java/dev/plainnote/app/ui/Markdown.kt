package dev.plainnote.app.ui

import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.sp

/**
 * Markdown editing helpers: transforms over a [TextFieldValue] (used by the
 * formatting toolbar) and a small Markdown -> [AnnotatedString] renderer (used
 * by the preview). The buffer stays plain Markdown — this is not a WYSIWYG.
 */
object Markdown {

    /** Wrap the selection with an inline `marker` (e.g. `**`), or unwrap it. */
    fun wrap(v: TextFieldValue, marker: String): TextFieldValue {
        val s = v.selection.min
        val e = v.selection.max
        val sel = v.text.substring(s, e)
        val unwrap = sel.length >= 2 * marker.length &&
            sel.startsWith(marker) && sel.endsWith(marker)
        val out = if (unwrap) sel.substring(marker.length, sel.length - marker.length)
        else "$marker$sel$marker"
        val text = v.text.substring(0, s) + out + v.text.substring(e)
        val cursor = if (sel.isEmpty() && !unwrap) s + marker.length else s + out.length
        return TextFieldValue(text, androidx.compose.ui.text.TextRange(if (sel.isEmpty()) cursor else s, cursor))
    }

    /** Apply/toggle a heading of `level` on the current line. */
    fun heading(v: TextFieldValue, level: Int): TextFieldValue =
        onLine(v) { setHeadingLine(it, level) }

    /** Toggle a line prefix (`- `, `1. `, `> `) on the current line. */
    fun linePrefix(v: TextFieldValue, prefix: String): TextFieldValue =
        onLine(v) { toggleLinePrefix(it, prefix) }

    /** Insert a Markdown link around the selection, selecting the `url`. */
    fun link(v: TextFieldValue): TextFieldValue {
        val s = v.selection.min
        val e = v.selection.max
        val label = v.text.substring(s, e).ifEmpty { "texte" }
        val out = "[$label](url)"
        val text = v.text.substring(0, s) + out + v.text.substring(e)
        val urlStart = s + 1 + label.length + 2
        return TextFieldValue(text, androidx.compose.ui.text.TextRange(urlStart, urlStart + 3))
    }

    private fun onLine(v: TextFieldValue, f: (String) -> String): TextFieldValue {
        val text = v.text
        val cur = v.selection.min
        val lineStart = text.lastIndexOf('\n', (cur - 1).coerceAtLeast(0)).let { if (it < 0) 0 else it + 1 }
        val nl = text.indexOf('\n', lineStart)
        val lineEnd = if (nl < 0) text.length else nl
        val newLine = f(text.substring(lineStart, lineEnd))
        val out = text.substring(0, lineStart) + newLine + text.substring(lineEnd)
        val cursor = lineStart + newLine.length
        return TextFieldValue(out, androidx.compose.ui.text.TextRange(cursor))
    }

    private fun headingLevelOf(line: String): Int {
        val h = line.length - line.trimStart('#').length
        return if (h in 1..6 && line.getOrNull(h) == ' ') h else 0
    }

    private fun setHeadingLine(line: String, level: Int): String {
        val cur = headingLevelOf(line)
        val body = if (cur > 0) line.substring(cur + 1) else line
        return if (cur == level) body else "${"#".repeat(level)} $body"
    }

    private fun stripListMarker(line: String): String {
        for (p in listOf("- ", "* ", "> ")) if (line.startsWith(p)) return line.substring(p.length)
        val digits = line.takeWhile { it.isDigit() }.length
        if (digits > 0 && line.substring(digits).startsWith(". ")) return line.substring(digits + 2)
        return line
    }

    private fun toggleLinePrefix(line: String, prefix: String): String =
        if (line.startsWith(prefix)) line.substring(prefix.length) else prefix + stripListMarker(line)

    /** A French word/character summary, e.g. "42 mots · 210 caractères". */
    fun count(text: String): String {
        val words = text.split(Regex("\\s+")).count { it.isNotEmpty() }
        val chars = text.length
        val w = if (words == 1) "mot" else "mots"
        val c = if (chars == 1) "caractère" else "caractères"
        return "$words $w · $chars $c"
    }

    // --- preview rendering ---

    fun render(src: String): AnnotatedString = buildAnnotatedString {
        var inCode = false
        src.split('\n').forEachIndexed { i, line ->
            if (i > 0) append('\n')
            val fence = line.trimStart().startsWith("```")
            if (fence) {
                inCode = !inCode
                return@forEachIndexed
            }
            if (inCode) {
                withStyle(SpanStyle(fontFamily = FontFamily.Monospace)) { append(line) }
                return@forEachIndexed
            }
            val trimmed = line.trimStart()
            val hl = headingLevelOf(trimmed)
            when {
                hl > 0 -> withStyle(
                    SpanStyle(fontWeight = FontWeight.Bold, fontSize = headingSize(hl)),
                ) { inline(trimmed.substring(hl + 1)) }

                trimmed.startsWith("- ") || trimmed.startsWith("* ") -> {
                    append("•  "); inline(trimmed.substring(2))
                }

                trimmed.startsWith("> ") ->
                    withStyle(SpanStyle(fontStyle = FontStyle.Italic)) { inline(trimmed.substring(2)) }

                else -> inline(line)
            }
        }
    }

    private fun headingSize(level: Int): TextUnit = when (level) {
        1 -> 24.sp
        2 -> 20.sp
        3 -> 18.sp
        else -> 16.sp
    }

    /** Append one line's inline Markdown (code, bold, italic), non-nested. */
    private fun AnnotatedString.Builder.inline(s: String) {
        var i = 0
        while (i < s.length) {
            when {
                s.startsWith("**", i) -> {
                    val end = s.indexOf("**", i + 2)
                    if (end >= 0) {
                        withStyle(SpanStyle(fontWeight = FontWeight.Bold)) { append(s.substring(i + 2, end)) }
                        i = end + 2
                    } else { append(s.substring(i)); i = s.length }
                }
                s[i] == '*' -> {
                    val end = s.indexOf('*', i + 1)
                    if (end >= 0) {
                        withStyle(SpanStyle(fontStyle = FontStyle.Italic)) { append(s.substring(i + 1, end)) }
                        i = end + 1
                    } else { append(s[i]); i++ }
                }
                s[i] == '`' -> {
                    val end = s.indexOf('`', i + 1)
                    if (end >= 0) {
                        withStyle(SpanStyle(fontFamily = FontFamily.Monospace)) { append(s.substring(i + 1, end)) }
                        i = end + 1
                    } else { append(s[i]); i++ }
                }
                else -> { append(s[i]); i++ }
            }
        }
    }
}
