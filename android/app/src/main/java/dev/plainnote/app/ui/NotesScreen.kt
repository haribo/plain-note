package dev.plainnote.app.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Article
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Code
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.DriveFileMove
import androidx.compose.material.icons.filled.Folder
import androidx.compose.material.icons.filled.Label
import androidx.compose.material.icons.filled.Link
import androidx.compose.material.icons.filled.Menu
import androidx.compose.material.icons.filled.PushPin
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Visibility
import androidx.compose.material.icons.filled.VisibilityOff
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.AssistChip
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.InputChip
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.ModalDrawerSheet
import androidx.compose.material3.ModalNavigationDrawer
import androidx.compose.material3.NavigationDrawerItem
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.rememberDrawerState
import androidx.compose.material3.DrawerValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.text.input.TextFieldValue
import dev.plainnote.core.FolderInfo
import dev.plainnote.core.NoteSummary
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
    val folders by vm.folders.collectAsState()
    val snackbar = remember { SnackbarHostState() }
    var showPairing by remember { mutableStateOf(false) }
    var actionNote by remember { mutableStateOf<NoteSummary?>(null) }
    var moveNote by remember { mutableStateOf<NoteSummary?>(null) }
    var tagNote by remember { mutableStateOf<NoteSummary?>(null) }

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
                val pinned = notes.filter { it.pinned }
                val others = notes.filter { !it.pinned }
                LazyColumn(Modifier.fillMaxSize()) {
                    if (pinned.isNotEmpty()) {
                        item { SectionHeader("Épinglées") }
                        items(pinned, key = { it.id }) { note ->
                            NoteRow(note, vm.folderName(note.folder), { vm.open(note.id) }) { actionNote = note }
                        }
                    }
                    if (others.isNotEmpty()) {
                        if (pinned.isNotEmpty()) item { SectionHeader("Notes") }
                        items(others, key = { it.id }) { note ->
                            NoteRow(note, vm.folderName(note.folder), { vm.open(note.id) }) { actionNote = note }
                        }
                    }
                }
            }
        }
    }

    actionNote?.let { note ->
        NoteActionsSheet(
            note = note,
            onDismiss = { actionNote = null },
            onPin = { vm.setPinned(note.id, !note.pinned); actionNote = null },
            onMove = { actionNote = null; moveNote = note },
            onTag = { actionNote = null; tagNote = note },
            onTrash = { vm.trashNote(note.id); actionNote = null },
        )
    }

    moveNote?.let { note ->
        MoveDialog(
            folders = folders,
            onDismiss = { moveNote = null },
            onMove = { folderId -> vm.moveNote(note.id, folderId); moveNote = null },
        )
    }

    tagNote?.let { note ->
        TagDialog(
            onDismiss = { tagNote = null },
            onAdd = { tag -> vm.addTag(note.id, tag); tagNote = null },
        )
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
private fun SectionHeader(text: String) {
    Text(
        text,
        style = MaterialTheme.typography.labelLarge,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.fillMaxWidth().padding(16.dp, 12.dp, 16.dp, 4.dp),
    )
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun NoteRow(
    note: NoteSummary,
    folderName: String?,
    onOpen: () -> Unit,
    onLongPress: () -> Unit,
) {
    val meta = buildList {
        folderName?.let { add(it) }
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
        modifier = Modifier.fillMaxWidth().combinedClickable(
            onClick = onOpen,
            onLongClick = onLongPress,
        ),
    )
    HorizontalDivider()
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun NoteActionsSheet(
    note: NoteSummary,
    onDismiss: () -> Unit,
    onPin: () -> Unit,
    onMove: () -> Unit,
    onTag: () -> Unit,
    onTrash: () -> Unit,
) {
    ModalBottomSheet(onDismissRequest = onDismiss) {
        Text(
            note.title.ifEmpty { "(sans titre)" },
            style = MaterialTheme.typography.titleMedium,
            modifier = Modifier.padding(24.dp, 4.dp, 24.dp, 12.dp),
        )
        SheetAction(Icons.Filled.PushPin, if (note.pinned) "Désépingler" else "Épingler", onPin)
        SheetAction(Icons.Filled.DriveFileMove, "Déplacer vers…", onMove)
        SheetAction(Icons.Filled.Label, "Ajouter un tag", onTag)
        SheetAction(
            Icons.Filled.Delete,
            "Mettre à la corbeille",
            onTrash,
            tint = MaterialTheme.colorScheme.error,
        )
        Box(Modifier.padding(bottom = 24.dp))
    }
}

@Composable
private fun SheetAction(
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    label: String,
    onClick: () -> Unit,
    tint: androidx.compose.ui.graphics.Color = MaterialTheme.colorScheme.onSurface,
) {
    ListItem(
        headlineContent = { Text(label, color = tint) },
        leadingContent = { Icon(icon, contentDescription = null, tint = tint) },
        modifier = Modifier.fillMaxWidth().clickable { onClick() },
    )
}

@Composable
private fun MoveDialog(
    folders: List<FolderInfo>,
    onDismiss: () -> Unit,
    onMove: (String?) -> Unit,
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Déplacer vers") },
        text = {
            Column {
                ListItem(
                    headlineContent = { Text("Racine") },
                    modifier = Modifier.fillMaxWidth().clickable { onMove(null) },
                )
                folders.forEach { f ->
                    ListItem(
                        headlineContent = { Text(f.path) },
                        modifier = Modifier.fillMaxWidth().clickable { onMove(f.id) },
                    )
                }
            }
        },
        confirmButton = {},
        dismissButton = { TextButton(onClick = onDismiss) { Text("Annuler") } },
    )
}

@Composable
private fun TagDialog(onDismiss: () -> Unit, onAdd: (String) -> Unit) {
    var tag by remember { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Ajouter un tag") },
        text = {
            OutlinedTextField(
                value = tag,
                onValueChange = { tag = it },
                label = { Text("Tag") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
        },
        confirmButton = {
            TextButton(onClick = { onAdd(tag.trim()) }, enabled = tag.isNotBlank()) {
                Text("Ajouter")
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Annuler") } },
    )
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
    var body by remember(note.id) { mutableStateOf(TextFieldValue(note.text)) }
    var tags by remember(note.id) { mutableStateOf(note.tags) }
    var preview by remember(note.id) { mutableStateOf(false) }
    var visual by remember(note.id) { mutableStateOf(false) }
    var showTag by remember(note.id) { mutableStateOf(false) }

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
                    // Experimental visual (WYSIWYG) editor vs raw Markdown.
                    IconButton(onClick = { visual = !visual }) {
                        Icon(
                            if (visual) Icons.Filled.Code else Icons.AutoMirrored.Filled.Article,
                            contentDescription = "Éditeur visuel / Markdown",
                        )
                    }
                    if (!visual) {
                        IconButton(onClick = { preview = !preview }) {
                            Icon(
                                if (preview) Icons.Filled.VisibilityOff else Icons.Filled.Visibility,
                                contentDescription = "Aperçu",
                            )
                        }
                    }
                    IconButton(onClick = { vm.delete(note.id) }) {
                        Icon(Icons.Filled.Delete, contentDescription = "Supprimer")
                    }
                },
            )
        },
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding).imePadding()) {
            OutlinedTextField(
                value = title,
                onValueChange = {
                    title = it
                    vm.saveTitle(note.id, it)
                },
                label = { Text("Titre") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth().padding(16.dp, 12.dp, 16.dp, 4.dp),
            )
            TagsRow(
                tags = tags,
                onRemove = { t -> tags = tags - t; vm.removeTag(note.id, t) },
                onAdd = { showTag = true },
            )
            Text(
                Markdown.count(body.text),
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.fillMaxWidth().padding(16.dp, 0.dp),
            )
            Box(Modifier.weight(1f).fillMaxWidth()) {
                if (visual) {
                    VisualEditor(
                        initialMarkdown = body.text,
                        onBodyChange = { md ->
                            body = TextFieldValue(md)
                            vm.saveBody(note.id, md)
                        },
                    )
                } else if (preview) {
                    Text(
                        Markdown.render(body.text),
                        modifier = Modifier
                            .fillMaxSize()
                            .verticalScroll(rememberScrollState())
                            .padding(16.dp, 8.dp),
                    )
                } else {
                    TextField(
                        value = body,
                        onValueChange = {
                            body = it
                            vm.saveBody(note.id, it.text)
                        },
                        textStyle = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace),
                        modifier = Modifier.fillMaxSize(),
                    )
                }
            }
            if (!preview && !visual) {
                FormatToolbar(body) { v ->
                    body = v
                    vm.saveBody(note.id, v.text)
                }
            }
        }
    }

    if (showTag) {
        TagDialog(
            onDismiss = { showTag = false },
            onAdd = { t -> tags = tags + t; vm.addTag(note.id, t); showTag = false },
        )
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun TagsRow(tags: List<String>, onRemove: (String) -> Unit, onAdd: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(16.dp, 4.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        tags.forEach { t ->
            InputChip(
                selected = false,
                onClick = {},
                label = { Text("#$t") },
                trailingIcon = {
                    Icon(
                        Icons.Filled.Close,
                        contentDescription = "Retirer",
                        modifier = Modifier.clickable { onRemove(t) },
                    )
                },
            )
        }
        AssistChip(onClick = onAdd, label = { Text("+ tag") })
    }
}

@Composable
private fun FormatToolbar(value: TextFieldValue, onChange: (TextFieldValue) -> Unit) {
    Surface(tonalElevation = 2.dp) {
        Row(
            Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            FmtBtn("B") { onChange(Markdown.wrap(value, "**")) }
            FmtBtn("I") { onChange(Markdown.wrap(value, "*")) }
            FmtBtn("S") { onChange(Markdown.wrap(value, "~~")) }
            FmtBtn("</>") { onChange(Markdown.wrap(value, "`")) }
            FmtBtn("H1") { onChange(Markdown.heading(value, 1)) }
            FmtBtn("H2") { onChange(Markdown.heading(value, 2)) }
            FmtBtn("•") { onChange(Markdown.linePrefix(value, "- ")) }
            FmtBtn("1.") { onChange(Markdown.linePrefix(value, "1. ")) }
            FmtBtn("❝") { onChange(Markdown.linePrefix(value, "> ")) }
            FmtBtn("🔗") { onChange(Markdown.link(value)) }
        }
    }
}

@Composable
private fun FmtBtn(label: String, onClick: () -> Unit) {
    TextButton(onClick = onClick) { Text(label) }
}
