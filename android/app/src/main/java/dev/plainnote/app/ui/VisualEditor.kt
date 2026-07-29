package dev.plainnote.app.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.List
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.CheckBox
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Title
import androidx.compose.material3.Checkbox
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.text.font.FontFamily
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
 * SPIKE — experimental block-based visual editor over the core document model.
 * Edits `Doc` blocks and serializes back to Markdown. v0 flattens a block's
 * inline formatting to plain text on edit (marks dropped) — hence opt-in.
 */
@Composable
fun VisualEditor(initialMarkdown: String, onBodyChange: (String) -> Unit) {
    var blocks by remember { mutableStateOf(markdownToDoc(initialMarkdown).blocks) }

    fun commit(newBlocks: List<Block>) {
        blocks = newBlocks
        onBodyChange(docToMarkdown(Doc(newBlocks)))
    }
    fun replace(index: Int, block: Block) =
        commit(blocks.toMutableList().also { it[index] = block })
    fun delete(index: Int) =
        commit(blocks.toMutableList().also { it.removeAt(index) })
    fun addParagraph() =
        commit(blocks + Block.Paragraph(plainRun("")))

    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(12.dp, 8.dp)) {
        blocks.forEachIndexed { index, block ->
            BlockRow(
                block = block,
                onChange = { replace(index, it) },
                onSetType = { replace(index, changeType(block, it)) },
                onDelete = { delete(index) },
            )
        }
        TextButton(onClick = { addParagraph() }, modifier = Modifier.padding(top = 8.dp)) {
            Icon(Icons.Filled.Add, contentDescription = null)
            Text("  Ajouter un bloc")
        }
    }
}

private enum class BlockType { Paragraph, H1, H2, Checklist, Bullet }

@Composable
private fun BlockRow(
    block: Block,
    onChange: (Block) -> Unit,
    onSetType: (BlockType) -> Unit,
    onDelete: () -> Unit,
) {
    Row(Modifier.fillMaxWidth().padding(vertical = 2.dp), verticalAlignment = Alignment.Top) {
        BlockMenu(onSetType = onSetType, onDelete = onDelete)
        Box(Modifier.fillMaxWidth()) {
            when (block) {
                is Block.Heading -> InlineField(
                    text = inlineText(block.inlines),
                    onText = { onChange(Block.Heading(block.level, plainRun(it))) },
                    fontSize = if (block.level.toInt() == 1) 24.sp else 20.sp,
                    weight = FontWeight.Bold,
                )
                is Block.Paragraph -> InlineField(
                    text = inlineText(block.inlines),
                    onText = { onChange(Block.Paragraph(plainRun(it))) },
                )
                is Block.TaskList -> Column {
                    block.items.forEachIndexed { i, item ->
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Checkbox(
                                checked = item.checked,
                                onCheckedChange = { checked ->
                                    val items = block.items.toMutableList()
                                    items[i] = TaskItem(checked, item.inlines)
                                    onChange(Block.TaskList(items))
                                },
                            )
                            InlineField(
                                text = inlineText(item.inlines),
                                onText = {
                                    val items = block.items.toMutableList()
                                    items[i] = TaskItem(item.checked, plainRun(it))
                                    onChange(Block.TaskList(items))
                                },
                            )
                        }
                    }
                }
                is Block.BulletList -> Column {
                    block.items.forEachIndexed { i, item ->
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Text("•  ", color = MaterialTheme.colorScheme.onSurfaceVariant)
                            InlineField(
                                text = inlineText(item.inlines),
                                onText = {
                                    val items = block.items.toMutableList()
                                    items[i] = ListItem(plainRun(it))
                                    onChange(Block.BulletList(items))
                                },
                            )
                        }
                    }
                }
                is Block.OrderedList -> Column {
                    block.items.forEachIndexed { i, item ->
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Text("${i + 1}.  ", color = MaterialTheme.colorScheme.onSurfaceVariant)
                            InlineField(
                                text = inlineText(item.inlines),
                                onText = {
                                    val items = block.items.toMutableList()
                                    items[i] = ListItem(plainRun(it))
                                    onChange(Block.OrderedList(items))
                                },
                            )
                        }
                    }
                }
                is Block.Quote -> InlineField(
                    text = inlineText(block.inlines),
                    onText = { onChange(Block.Quote(plainRun(it))) },
                    fontStyle = androidx.compose.ui.text.font.FontStyle.Italic,
                )
                is Block.CodeBlock -> ReadOnly(block.text, mono = true)
                is Block.Raw -> ReadOnly(block.text, mono = true)
            }
        }
    }
}

@Composable
private fun BlockMenu(onSetType: (BlockType) -> Unit, onDelete: () -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box {
        IconButton(onClick = { open = true }, modifier = Modifier.width(36.dp)) {
            Icon(Icons.Filled.MoreVert, contentDescription = "Bloc")
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            DropdownMenuItem(
                text = { Text("Paragraphe") },
                onClick = { onSetType(BlockType.Paragraph); open = false },
            )
            DropdownMenuItem(
                text = { Text("Titre 1") },
                leadingIcon = { Icon(Icons.Filled.Title, null) },
                onClick = { onSetType(BlockType.H1); open = false },
            )
            DropdownMenuItem(
                text = { Text("Titre 2") },
                onClick = { onSetType(BlockType.H2); open = false },
            )
            DropdownMenuItem(
                text = { Text("Case à cocher") },
                leadingIcon = { Icon(Icons.Filled.CheckBox, null) },
                onClick = { onSetType(BlockType.Checklist); open = false },
            )
            DropdownMenuItem(
                text = { Text("Liste à puces") },
                leadingIcon = { Icon(Icons.AutoMirrored.Filled.List, null) },
                onClick = { onSetType(BlockType.Bullet); open = false },
            )
            DropdownMenuItem(
                text = { Text("Supprimer") },
                leadingIcon = { Icon(Icons.Filled.Delete, null) },
                onClick = { onDelete(); open = false },
            )
        }
    }
}

@Composable
private fun InlineField(
    text: String,
    onText: (String) -> Unit,
    fontSize: androidx.compose.ui.unit.TextUnit = androidx.compose.ui.unit.TextUnit.Unspecified,
    weight: FontWeight? = null,
    fontStyle: androidx.compose.ui.text.font.FontStyle? = null,
) {
    val style = LocalTextStyle.current.copy(
        color = MaterialTheme.colorScheme.onSurface,
        fontSize = fontSize,
        fontWeight = weight,
        fontStyle = fontStyle,
    )
    BasicTextField(
        value = text,
        onValueChange = onText,
        textStyle = style,
        cursorBrush = SolidColor(MaterialTheme.colorScheme.primary),
        modifier = Modifier.fillMaxWidth().padding(vertical = 8.dp),
    )
}

@Composable
private fun ReadOnly(text: String, mono: Boolean) {
    Text(
        text,
        fontFamily = if (mono) FontFamily.Monospace else FontFamily.Default,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.fillMaxWidth().padding(vertical = 8.dp),
    )
}

// --- helpers ---

private fun plainRun(text: String): List<Inline> =
    listOf(Inline.Run(text, Marks(bold = false, italic = false, strikethrough = false, code = false)))

/** Flatten inline content to plain text (spike: marks/link styling dropped on edit). */
private fun inlineText(inlines: List<Inline>): String = buildString {
    for (i in inlines) when (i) {
        is Inline.Run -> append(i.text)
        is Inline.Link -> append(inlineText(i.inlines))
    }
}

private fun changeType(block: Block, type: BlockType): Block {
    val inlines = blockInlines(block)
    return when (type) {
        BlockType.Paragraph -> Block.Paragraph(inlines)
        BlockType.H1 -> Block.Heading(1u, inlines)
        BlockType.H2 -> Block.Heading(2u, inlines)
        BlockType.Checklist -> Block.TaskList(listOf(TaskItem(false, inlines)))
        BlockType.Bullet -> Block.BulletList(listOf(ListItem(inlines)))
    }
}

/** Best-effort inlines for a block, for type changes. */
private fun blockInlines(block: Block): List<Inline> = when (block) {
    is Block.Heading -> block.inlines
    is Block.Paragraph -> block.inlines
    is Block.Quote -> block.inlines
    is Block.TaskList -> block.items.firstOrNull()?.inlines ?: plainRun("")
    is Block.BulletList -> block.items.firstOrNull()?.inlines ?: plainRun("")
    is Block.OrderedList -> block.items.firstOrNull()?.inlines ?: plainRun("")
    is Block.CodeBlock -> plainRun(block.text)
    is Block.Raw -> plainRun(block.text)
}
