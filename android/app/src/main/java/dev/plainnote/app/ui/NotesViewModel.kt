package dev.plainnote.app.ui

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import dev.plainnote.app.data.NoteRepository
import dev.plainnote.core.FolderInfo
import dev.plainnote.core.NoteContent
import dev.plainnote.core.NoteSummary
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

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
        } catch (e: Exception) {
            report(e)
        }
    }

    fun saveBody(id: String, text: String) = viewModelScope.launch(Dispatchers.IO) {
        try {
            repo.setBody(id, text)
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

    fun sync() = viewModelScope.launch(Dispatchers.IO) {
        _status.value = try {
            if (!repo.isEnrolled()) {
                "Appareil non associé"
            } else {
                val seq = repo.sync()
                _folders.value = repo.listFolders()
                reloadNotes()
                "Synchronisé (seq $seq)"
            }
        } catch (e: Exception) {
            "Échec de la synchronisation : ${e.message}"
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
