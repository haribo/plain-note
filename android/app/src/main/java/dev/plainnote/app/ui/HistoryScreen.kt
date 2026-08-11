@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package dev.plainnote.app.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.ChevronRight
import androidx.compose.material3.AssistChip
import androidx.compose.material3.AssistChipDefaults
import androidx.compose.material3.Button
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import dev.plainnote.core.NoteContent
import dev.plainnote.core.NoteVersionInfo
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale

private val HISTORY_FMT: DateTimeFormatter =
    DateTimeFormatter.ofPattern("d MMM yyyy, HH:mm", Locale.FRENCH)

/** Format a unix-millis timestamp as a local, human date. */
fun formatVersionTime(ts: Long): String =
    Instant.ofEpochMilli(ts).atZone(ZoneId.systemDefault()).format(HISTORY_FMT)

/** The version timeline (newest first). The current version is labelled and not
 * selectable; older versions open a read-only preview. */
@Composable
fun HistoryTimeline(
    versions: List<NoteVersionInfo>,
    onSelect: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    if (versions.isEmpty()) {
        Box(modifier.fillMaxSize().padding(24.dp), contentAlignment = Alignment.Center) {
            Text(
                "Aucune version antérieure.",
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        return
    }
    LazyColumn(modifier.fillMaxSize()) {
        itemsIndexed(versions) { index, v ->
            val current = index == 0
            ListItem(
                headlineContent = { Text(formatVersionTime(v.timestamp)) },
                trailingContent = {
                    if (current) {
                        AssistChip(
                            onClick = {},
                            enabled = false,
                            label = { Text("Actuel") },
                            colors = AssistChipDefaults.assistChipColors(
                                disabledLabelColor = MaterialTheme.colorScheme.primary,
                            ),
                        )
                    } else {
                        Icon(Icons.Filled.ChevronRight, contentDescription = null)
                    }
                },
                modifier = Modifier.clickable(enabled = !current) { onSelect(v.versionId) },
            )
            HorizontalDivider()
        }
    }
}

/** Read-only preview of one past version, with Restore / Back. */
@Composable
fun VersionPreview(
    note: NoteContent,
    onRestore: () -> Unit,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(modifier.fillMaxSize()) {
        Surface(color = MaterialTheme.colorScheme.primaryContainer) {
            Text(
                "Version en lecture seule — le texte actuel n'est pas modifié.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onPrimaryContainer,
                modifier = Modifier.fillMaxWidth().padding(16.dp, 10.dp),
            )
        }
        Text(
            note.title.ifEmpty { "(sans titre)" },
            style = MaterialTheme.typography.headlineSmall,
            modifier = Modifier.fillMaxWidth().padding(16.dp, 16.dp, 16.dp, 4.dp),
        )
        DocView(note.text, Modifier.weight(1f).fillMaxWidth())
        Row(
            Modifier.fillMaxWidth().padding(12.dp),
            horizontalArrangement = Arrangement.End,
        ) {
            TextButton(onClick = onBack) { Text("Retour") }
            Button(onClick = onRestore) { Text("Restaurer cette version") }
        }
    }
}

/** Full-screen history surface: timeline, then a version preview, wired to the
 * ViewModel. Renders nothing until history is opened. */
@Composable
fun HistorySheet(vm: NotesViewModel) {
    val history by vm.history.collectAsState()
    val preview by vm.versionPreview.collectAsState()
    val versions = history ?: return
    var selected by remember { mutableStateOf<String?>(null) }

    Dialog(
        onDismissRequest = { selected = null; vm.closeHistory() },
        properties = DialogProperties(usePlatformDefaultWidth = false),
    ) {
        Surface(Modifier.fillMaxSize()) {
            val current = preview
            Scaffold(
                topBar = {
                    TopAppBar(
                        title = { Text(if (current != null) "Aperçu" else "Historique") },
                        navigationIcon = {
                            IconButton(onClick = {
                                if (current != null) {
                                    selected = null
                                    vm.closePreview()
                                } else {
                                    vm.closeHistory()
                                }
                            }) {
                                Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Retour")
                            }
                        },
                    )
                },
            ) { padding ->
                Box(Modifier.padding(padding)) {
                    if (current != null) {
                        VersionPreview(
                            note = current,
                            onRestore = { selected?.let { vm.restoreVersion(it) } },
                            onBack = { selected = null; vm.closePreview() },
                        )
                    } else {
                        HistoryTimeline(
                            versions = versions,
                            onSelect = { selected = it; vm.previewVersion(it) },
                        )
                    }
                }
            }
        }
    }
}
