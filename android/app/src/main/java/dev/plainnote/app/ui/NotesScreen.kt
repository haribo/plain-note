package dev.plainnote.app.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.ArrowBack
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import dev.plainnote.core.NoteContent

/** Top-level screen: the note list, or the editor when a note is open. */
@Composable
fun NotesScreen(vm: NotesViewModel) {
    val editing by vm.editing.collectAsState()
    val current = editing
    if (current == null) {
        NoteListScreen(vm)
    } else {
        NoteEditor(vm, current)
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun NoteListScreen(vm: NotesViewModel) {
    val notes by vm.notes.collectAsState()
    Scaffold(
        topBar = { TopAppBar(title = { Text("Plain Note") }) },
        floatingActionButton = {
            FloatingActionButton(onClick = { vm.createAndOpen() }) {
                Icon(Icons.Filled.Add, contentDescription = "Nouvelle note")
            }
        },
    ) { padding ->
        if (notes.isEmpty()) {
            Box(Modifier.fillMaxSize().padding(padding)) {
                Text(
                    "Aucune note",
                    modifier = Modifier.padding(24.dp),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        } else {
            LazyColumn(Modifier.fillMaxSize().padding(padding)) {
                items(notes, key = { it.id }) { note ->
                    ListItem(
                        headlineContent = {
                            Text(note.title.ifEmpty { "(sans titre)" })
                        },
                        modifier = Modifier
                            .fillMaxWidth()
                            .clickable { vm.open(note.id) },
                    )
                    HorizontalDivider()
                }
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun NoteEditor(vm: NotesViewModel, note: NoteContent) {
    var title by remember(note.id) { mutableStateOf(note.title) }
    var body by remember(note.id) { mutableStateOf(note.text) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Édition") },
                navigationIcon = {
                    IconButton(onClick = { vm.close() }) {
                        Icon(Icons.Filled.ArrowBack, contentDescription = "Retour")
                    }
                },
                actions = {
                    IconButton(onClick = { vm.delete(note.id) }) {
                        Icon(Icons.Filled.Delete, contentDescription = "Supprimer")
                    }
                },
            )
        },
    ) { padding ->
        androidx.compose.foundation.layout.Column(
            Modifier.fillMaxSize().padding(padding).padding(16.dp),
        ) {
            OutlinedTextField(
                value = title,
                onValueChange = {
                    title = it
                    vm.saveTitle(note.id, it)
                },
                label = { Text("Titre") },
                modifier = Modifier.fillMaxWidth(),
            )
            TextField(
                value = body,
                onValueChange = {
                    body = it
                    vm.saveBody(note.id, it)
                },
                label = { Text("Contenu (Markdown)") },
                textStyle = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace),
                modifier = Modifier.fillMaxSize().padding(top = 12.dp),
            )
        }
    }
}
