package dev.plainnote.app.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Folder
import androidx.compose.material.icons.filled.Link
import androidx.compose.material.icons.filled.Menu
import androidx.compose.material.icons.filled.PushPin
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalDrawerSheet
import androidx.compose.material3.ModalNavigationDrawer
import androidx.compose.material3.NavigationDrawerItem
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.rememberDrawerState
import androidx.compose.material3.DrawerValue
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import dev.plainnote.core.NoteContent
import kotlinx.coroutines.launch

/** Root: the note list under a navigation drawer, or the editor when a note is open. */
@Composable
fun AppRoot(vm: NotesViewModel) {
    val editing by vm.editing.collectAsState()
    val current = editing
    if (current != null) {
        NoteEditor(vm, current)
        return
    }

    val drawerState = rememberDrawerState(DrawerValue.Closed)
    val scope = rememberCoroutineScope()
    ModalNavigationDrawer(
        drawerState = drawerState,
        drawerContent = { AppDrawer(vm) { scope.launch { drawerState.close() } } },
    ) {
        NoteListScreen(vm, onMenu = { scope.launch { drawerState.open() } })
    }
}

@Composable
private fun AppDrawer(vm: NotesViewModel, onClose: () -> Unit) {
    val folders by vm.folders.collectAsState()
    val current by vm.currentFolder.collectAsState()
    var showNewFolder by remember { mutableStateOf(false) }
    ModalDrawerSheet {
        Text(
            "Plain Note",
            style = MaterialTheme.typography.titleLarge,
            modifier = Modifier.padding(24.dp, 20.dp, 24.dp, 12.dp),
        )
        NavigationDrawerItem(
            label = { Text("Toutes les notes") },
            icon = { Icon(Icons.Filled.Menu, contentDescription = null) },
            selected = current == null,
            onClick = { vm.selectFolder(null); onClose() },
            modifier = Modifier.padding(horizontal = 12.dp),
        )
        HorizontalDivider(Modifier.padding(16.dp, 8.dp))
        Text(
            "Dossiers",
            style = MaterialTheme.typography.labelLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(28.dp, 4.dp),
        )
        folders.forEach { f ->
            NavigationDrawerItem(
                label = { Text(f.path) },
                icon = { Icon(Icons.Filled.Folder, contentDescription = null) },
                selected = current?.id == f.id,
                onClick = { vm.selectFolder(f); onClose() },
                modifier = Modifier.padding(horizontal = 12.dp),
            )
        }
        NavigationDrawerItem(
            label = { Text("Nouveau dossier") },
            icon = { Icon(Icons.Filled.Add, contentDescription = null) },
            selected = false,
            onClick = { showNewFolder = true },
            modifier = Modifier.padding(horizontal = 12.dp),
        )
        HorizontalDivider(Modifier.padding(16.dp, 8.dp))
        NavigationDrawerItem(
            label = { Text("Synchroniser") },
            icon = { Icon(Icons.Filled.Refresh, contentDescription = null) },
            selected = false,
            onClick = { vm.sync(); onClose() },
            modifier = Modifier.padding(horizontal = 12.dp),
        )
    }

    if (showNewFolder) {
        var name by remember { mutableStateOf("") }
        AlertDialog(
            onDismissRequest = { showNewFolder = false },
            title = { Text("Nouveau dossier") },
            text = {
                OutlinedTextField(
                    value = name,
                    onValueChange = { name = it },
                    label = { Text("Nom du dossier") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
            },
            confirmButton = {
                TextButton(
                    onClick = { vm.createFolder(name); showNewFolder = false },
                    enabled = name.isNotBlank(),
                ) { Text("Créer") }
            },
            dismissButton = { TextButton(onClick = { showNewFolder = false }) { Text("Annuler") } },
        )
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun NoteListScreen(vm: NotesViewModel, onMenu: () -> Unit) {
    val notes by vm.notes.collectAsState()
    val status by vm.status.collectAsState()
    val query by vm.query.collectAsState()
    val current by vm.currentFolder.collectAsState()
    val snackbar = remember { SnackbarHostState() }
    var showPairing by remember { mutableStateOf(false) }

    LaunchedEffect(status) {
        status?.let {
            snackbar.showSnackbar(it)
            vm.clearStatus()
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(current?.name ?: "Toutes les notes") },
                navigationIcon = {
                    IconButton(onClick = onMenu) {
                        Icon(Icons.Filled.Menu, contentDescription = "Menu")
                    }
                },
                actions = {
                    IconButton(onClick = { vm.sync() }) {
                        Icon(Icons.Filled.Refresh, contentDescription = "Synchroniser")
                    }
                    IconButton(onClick = { showPairing = true }) {
                        Icon(Icons.Filled.Link, contentDescription = "Associer")
                    }
                },
            )
        },
        snackbarHost = { SnackbarHost(snackbar) },
        floatingActionButton = {
            FloatingActionButton(onClick = { vm.createAndOpen() }) {
                Icon(Icons.Filled.Add, contentDescription = "Nouvelle note")
            }
        },
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            OutlinedTextField(
                value = query,
                onValueChange = { vm.setQuery(it) },
                leadingIcon = { Icon(Icons.Filled.Search, contentDescription = null) },
                placeholder = { Text("Rechercher") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth().padding(12.dp, 4.dp, 12.dp, 8.dp),
            )
            if (notes.isEmpty()) {
                Box(Modifier.fillMaxSize()) {
                    Text(
                        if (query.isNotBlank()) "Aucun résultat" else "Aucune note",
                        modifier = Modifier.padding(24.dp),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            } else {
                LazyColumn(Modifier.fillMaxSize()) {
                    items(notes, key = { it.id }) { note ->
                        val meta = buildList {
                            vm.folderName(note.folder)?.let { add(it) }
                            addAll(note.tags.map { "#$it" })
                        }
                        ListItem(
                            headlineContent = { Text(note.title.ifEmpty { "(sans titre)" }) },
                            supportingContent = {
                                if (meta.isNotEmpty()) Text(meta.joinToString("   "), maxLines = 1)
                            },
                            trailingContent = {
                                if (note.pinned) {
                                    Icon(
                                        Icons.Filled.PushPin,
                                        contentDescription = "Épinglée",
                                        tint = MaterialTheme.colorScheme.primary,
                                    )
                                }
                            },
                            modifier = Modifier.fillMaxWidth().clickable { vm.open(note.id) },
                        )
                        HorizontalDivider()
                    }
                }
            }
        }
    }

    if (showPairing) {
        PairingDialog(
            onDismiss = { showPairing = false },
            onPair = { blob ->
                showPairing = false
                vm.pair(blob)
            },
        )
    }
}

@Composable
private fun PairingDialog(onDismiss: () -> Unit, onPair: (String) -> Unit) {
    var blob by remember { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Associer l'appareil") },
        text = {
            OutlinedTextField(
                value = blob,
                onValueChange = { blob = it },
                label = { Text("Code d'appairage") },
                modifier = Modifier.fillMaxWidth(),
            )
        },
        confirmButton = {
            TextButton(onClick = { onPair(blob.trim()) }, enabled = blob.isNotBlank()) {
                Text("Associer")
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Annuler") } },
    )
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
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Retour")
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
        Column(Modifier.fillMaxSize().padding(padding).padding(16.dp)) {
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
