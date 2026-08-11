package dev.plainnote.app.ui

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import dev.plainnote.app.data.AppGraph
import dev.plainnote.app.data.NoteRepository
import dev.plainnote.core.FolderInfo
import dev.plainnote.core.NoteContent
import dev.plainnote.core.NoteSummary
import dev.plainnote.core.NoteVersionInfo
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex

/** Discreet sync status surfaced in the top bar. */
enum class SyncState { Idle, Syncing, UpToDate, Offline }

/** Debounce after the last edit before an automatic sync. */
private const val AUTO_SYNC_DEBOUNCE_MS = 3_000L

/**
 * Holds the note list (filtered by folder or search), the folder tree, the note
 * being edited, and a transient status line. Every facade call runs on the IO
 * dispatcher, never the main thread.
 */
class NotesViewModel(
    app: Application,
    private val repo: NoteRepository,
    private val io: CoroutineDispatcher,
) : AndroidViewModel(app) {

    /** Production entry point used by `by viewModels()`. */
    constructor(app: Application) : this(app, AppGraph.repository(app), Dispatchers.IO)

    private val _notes = MutableStateFlow<List<NoteSummary>>(emptyList())
    val notes = _notes.asStateFlow()

    private val _folders = MutableStateFlow<List<FolderInfo>>(emptyList())
    val folders = _folders.asStateFlow()

    /** The folder whose notes are shown, or null for "all notes". */
    private val _currentFolder = MutableStateFlow<FolderInfo?>(null)
    val currentFolder = _currentFolder.asStateFlow()

    private val _query = MutableStateFlow("")
    val query = _query.asStateFlow()

    private val _editing = MutableStateFlow<NoteContent?>(null)
    val editing = _editing.asStateFlow()

    private val _status = MutableStateFlow<String?>(null)
    val status = _status.asStateFlow()

    private val _syncState = MutableStateFlow(SyncState.Idle)
    val syncState = _syncState.asStateFlow()

    // Serializes syncs: tryLock skips a request while one is already running.
    private val syncMutex = Mutex()
    private var pendingAutoSync: Job? = null

    init {
        refresh()
    }

    private fun report(e: Exception) {
        _status.value = "Erreur : ${e.message}"
    }

    /** Reload notes honoring the active search query or folder filter. */
    private fun reloadNotes() {
        val q = _query.value.trim()
        _notes.value = if (q.isNotEmpty()) repo.search(q) else repo.listNotes(_currentFolder.value?.id)
    }

    fun refresh() = viewModelScope.launch(io) {
        try {
            _folders.value = repo.listFolders()
            reloadNotes()
        } catch (e: Exception) {
            report(e)
        }
    }

    fun setPinned(id: String, pinned: Boolean) = viewModelScope.launch(io) {
        try {
            repo.setPinned(id, pinned)
            reloadNotes()
        } catch (e: Exception) {
            report(e)
        }
    }

    fun moveNote(id: String, folder: String?) = viewModelScope.launch(io) {
        try {
            repo.moveNote(id, folder)
            reloadNotes()
        } catch (e: Exception) {
            report(e)
        }
    }

    fun addTag(id: String, tag: String) = viewModelScope.launch(io) {
        try {
            repo.addTag(id, tag.trim())
            reloadNotes()
        } catch (e: Exception) {
            report(e)
        }
    }

    fun removeTag(id: String, tag: String) = viewModelScope.launch(io) {
        try {
            repo.removeTag(id, tag)
            reloadNotes()
        } catch (e: Exception) {
            report(e)
        }
    }

    fun trashNote(id: String) = viewModelScope.launch(io) {
        try {
            repo.trash(id)
            reloadNotes()
        } catch (e: Exception) {
            report(e)
        }
    }

    /** Folder ids whose subtree is expanded in the drawer (in-memory). */
    private val _expandedFolders = MutableStateFlow<Set<String>>(emptySet())
    val expandedFolders = _expandedFolders.asStateFlow()

    fun toggleFolderExpanded(id: String) {
        _expandedFolders.value = _expandedFolders.value.toMutableSet().also {
            if (!it.add(id)) it.remove(id)
        }
    }

    fun createFolder(name: String, parent: String? = null) = viewModelScope.launch(io) {
        try {
            repo.createFolder(name.trim(), parent)
            _folders.value = repo.listFolders()
        } catch (e: Exception) {
            report(e)
        }
    }

    fun renameFolder(id: String, name: String) = viewModelScope.launch(io) {
        try {
            repo.renameFolder(id, name.trim())
            _folders.value = repo.listFolders()
        } catch (e: Exception) {
            report(e)
        }
    }

    fun moveFolder(id: String, parent: String?) = viewModelScope.launch(io) {
        try {
            repo.moveFolder(id, parent)
            _folders.value = repo.listFolders()
        } catch (e: Exception) {
            report(e)
        }
    }

    fun deleteFolder(id: String) = viewModelScope.launch(io) {
        try {
            repo.deleteFolder(id)
            // The core reparents children; drop the filter if it pointed here.
            if (_currentFolder.value?.id == id) _currentFolder.value = null
            _folders.value = repo.listFolders()
            reloadNotes()
        } catch (e: Exception) {
            report(e)
        }
    }

    fun selectFolder(folder: FolderInfo?) = viewModelScope.launch(io) {
        _query.value = ""
        _currentFolder.value = folder
        try {
            reloadNotes()
        } catch (e: Exception) {
            report(e)
        }
    }

    fun setQuery(q: String) = viewModelScope.launch(io) {
        _query.value = q
        try {
            reloadNotes()
        } catch (e: Exception) {
            report(e)
        }
    }

    /** Display name for a folder id (empty = root), for note meta. */
    fun folderName(id: String): String? =
        if (id.isEmpty()) null else _folders.value.firstOrNull { it.id == id }?.name

    fun createAndOpen() = viewModelScope.launch(io) {
        try {
            val id = repo.createNote()
            // A new note lands in the current folder for a natural flow.
            _currentFolder.value?.let { repo.moveNote(id, it.id) }
            _editing.value = repo.getNote(id)
            reloadNotes()
        } catch (e: Exception) {
            report(e)
        }
    }

    fun open(id: String) = viewModelScope.launch(io) {
        try {
            _editing.value = repo.getNote(id)
        } catch (e: Exception) {
            report(e)
        }
    }

    fun close() {
        _editing.value = null
        refresh()
    }

    fun saveTitle(id: String, title: String) = viewModelScope.launch(io) {
        try {
            repo.setTitle(id, title)
            scheduleAutoSync()
        } catch (e: Exception) {
            report(e)
        }
    }

    fun saveBody(id: String, text: String) = viewModelScope.launch(io) {
        try {
            repo.setBody(id, text)
            scheduleAutoSync()
        } catch (e: Exception) {
            report(e)
        }
    }

    fun delete(id: String) = viewModelScope.launch(io) {
        try {
            repo.delete(id)
            _editing.value = null
            reloadNotes()
        } catch (e: Exception) {
            report(e)
        }
    }

    /** Manual sync (indicator tap / drawer): shows a snackbar. */
    fun sync() = syncNow(auto = false)

    /** Sync on app foreground. */
    fun onForeground() = syncNow(auto = true)

    /** Debounced automatic sync after the last edit. */
    private fun scheduleAutoSync() {
        pendingAutoSync?.cancel()
        pendingAutoSync = viewModelScope.launch {
            delay(AUTO_SYNC_DEBOUNCE_MS)
            syncNow(auto = true)
        }
    }

    /**
     * One guarded sync path. Automatic syncs are silent (no snackbar) and quiet
     * on failure — only the [syncState] indicator reflects them.
     */
    private fun syncNow(auto: Boolean) = viewModelScope.launch(io) {
        if (!repo.isEnrolled()) {
            if (!auto) _status.value = "Appareil non associé"
            return@launch
        }
        if (!syncMutex.tryLock()) return@launch // a sync is already running
        _syncState.value = SyncState.Syncing
        try {
            val seq = repo.sync()
            _folders.value = repo.listFolders()
            reloadNotes()
            _syncState.value = SyncState.UpToDate
            if (!auto) _status.value = "Synchronisé (seq $seq)"
        } catch (e: Exception) {
            _syncState.value = SyncState.Offline
            if (!auto) _status.value = "Échec de la synchronisation : ${e.message}"
        } finally {
            syncMutex.unlock()
        }
    }

    fun pair(blob: String) = viewModelScope.launch(io) {
        _status.value = try {
            repo.pair(blob)
            _folders.value = repo.listFolders()
            reloadNotes()
            "Appareil associé"
        } catch (e: Exception) {
            "Échec de l'association : ${e.message}"
        }
    }

    // --- version history (see docs/design/note-history.md) ---

    /** Non-null while the history timeline is open (newest first). */
    private val _history = MutableStateFlow<List<NoteVersionInfo>?>(null)
    val history = _history.asStateFlow()

    /** Non-null while previewing a past version (read-only). */
    private val _versionPreview = MutableStateFlow<NoteContent?>(null)
    val versionPreview = _versionPreview.asStateFlow()

    fun openHistory() = viewModelScope.launch(io) {
        val id = _editing.value?.id ?: return@launch
        try {
            _history.value = repo.history(id)
        } catch (e: Exception) {
            report(e)
        }
    }

    fun closeHistory() {
        _history.value = null
        _versionPreview.value = null
    }

    fun previewVersion(versionId: String) = viewModelScope.launch(io) {
        val id = _editing.value?.id ?: return@launch
        try {
            _versionPreview.value = repo.noteAt(id, versionId)
        } catch (e: Exception) {
            report(e)
        }
    }

    fun closePreview() {
        _versionPreview.value = null
    }

    fun restoreVersion(versionId: String) = viewModelScope.launch(io) {
        val id = _editing.value?.id ?: return@launch
        try {
            repo.restoreVersion(id, versionId)
            _editing.value = repo.getNote(id) // refresh the open editor
            _versionPreview.value = null
            _history.value = null
            reloadNotes()
        } catch (e: Exception) {
            report(e)
        }
    }

    fun clearStatus() {
        _status.value = null
    }
}
