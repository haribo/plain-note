package dev.plainnote.app.ui

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import dev.plainnote.app.data.NoteRepository
import dev.plainnote.core.FolderInfo
import dev.plainnote.core.NoteContent
import dev.plainnote.core.NoteSummary
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
class NotesViewModel(app: Application) : AndroidViewModel(app) {

    private val repo = NoteRepository(app)

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

    fun refresh() = viewModelScope.launch(Dispatchers.IO) {
        try {
            _folders.value = repo.listFolders()
            reloadNotes()
        } catch (e: Exception) {
            report(e)
        }
    }

    fun setPinned(id: String, pinned: Boolean) = viewModelScope.launch(Dispatchers.IO) {
        try {
            repo.setPinned(id, pinned)
            reloadNotes()
        } catch (e: Exception) {
            report(e)
        }
    }

    fun moveNote(id: String, folder: String?) = viewModelScope.launch(Dispatchers.IO) {
        try {
            repo.moveNote(id, folder)
            reloadNotes()
        } catch (e: Exception) {
            report(e)
        }
    }

    fun addTag(id: String, tag: String) = viewModelScope.launch(Dispatchers.IO) {
        try {
            repo.addTag(id, tag.trim())
            reloadNotes()
        } catch (e: Exception) {
            report(e)
        }
    }

    fun removeTag(id: String, tag: String) = viewModelScope.launch(Dispatchers.IO) {
        try {
            repo.removeTag(id, tag)
            reloadNotes()
        } catch (e: Exception) {
            report(e)
        }
    }

    fun trashNote(id: String) = viewModelScope.launch(Dispatchers.IO) {
        try {
            repo.trash(id)
            reloadNotes()
        } catch (e: Exception) {
            report(e)
        }
    }

    fun createFolder(name: String) = viewModelScope.launch(Dispatchers.IO) {
        try {
            repo.createFolder(name.trim())
            _folders.value = repo.listFolders()
        } catch (e: Exception) {
            report(e)
        }
    }

    fun selectFolder(folder: FolderInfo?) = viewModelScope.launch(Dispatchers.IO) {
        _query.value = ""
        _currentFolder.value = folder
        try {
            reloadNotes()
        } catch (e: Exception) {
            report(e)
        }
    }

    fun setQuery(q: String) = viewModelScope.launch(Dispatchers.IO) {
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

    fun createAndOpen() = viewModelScope.launch(Dispatchers.IO) {
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

    fun open(id: String) = viewModelScope.launch(Dispatchers.IO) {
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

    fun saveTitle(id: String, title: String) = viewModelScope.launch(Dispatchers.IO) {
        try {
            repo.setTitle(id, title)
            scheduleAutoSync()
        } catch (e: Exception) {
            report(e)
        }
    }

    fun saveBody(id: String, text: String) = viewModelScope.launch(Dispatchers.IO) {
        try {
            repo.setBody(id, text)
            scheduleAutoSync()
        } catch (e: Exception) {
            report(e)
        }
    }

    fun delete(id: String) = viewModelScope.launch(Dispatchers.IO) {
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
    private fun syncNow(auto: Boolean) = viewModelScope.launch(Dispatchers.IO) {
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

    fun pair(blob: String) = viewModelScope.launch(Dispatchers.IO) {
        _status.value = try {
            repo.pair(blob)
            _folders.value = repo.listFolders()
            reloadNotes()
            "Appareil associé"
        } catch (e: Exception) {
            "Échec de l'association : ${e.message}"
        }
    }

    fun clearStatus() {
        _status.value = null
    }
}
