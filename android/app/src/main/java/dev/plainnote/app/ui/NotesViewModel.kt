package dev.plainnote.app.ui

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import dev.plainnote.app.data.NoteRepository
import dev.plainnote.core.NoteContent
import dev.plainnote.core.NoteSummary
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

/**
 * Holds the note list, the note being edited, and a transient status line.
 * Every facade call runs on the IO dispatcher, never the main thread.
 */
class NotesViewModel(app: Application) : AndroidViewModel(app) {

    private val repo = NoteRepository(app)

    private val _notes = MutableStateFlow<List<NoteSummary>>(emptyList())
    val notes = _notes.asStateFlow()

    private val _editing = MutableStateFlow<NoteContent?>(null)
    val editing = _editing.asStateFlow()

    private val _status = MutableStateFlow<String?>(null)
    val status = _status.asStateFlow()

    init {
        refresh()
    }

    fun refresh() = viewModelScope.launch(Dispatchers.IO) {
        _notes.value = repo.listNotes()
    }

    fun createAndOpen() = viewModelScope.launch(Dispatchers.IO) {
        val id = repo.createNote()
        _editing.value = repo.getNote(id)
        _notes.value = repo.listNotes()
    }

    fun open(id: String) = viewModelScope.launch(Dispatchers.IO) {
        _editing.value = repo.getNote(id)
    }

    fun close() {
        _editing.value = null
        refresh()
    }

    fun saveTitle(id: String, title: String) = viewModelScope.launch(Dispatchers.IO) {
        repo.setTitle(id, title)
    }

    fun saveBody(id: String, text: String) = viewModelScope.launch(Dispatchers.IO) {
        repo.setBody(id, text)
    }

    fun delete(id: String) = viewModelScope.launch(Dispatchers.IO) {
        repo.delete(id)
        _editing.value = null
        _notes.value = repo.listNotes()
    }

    fun sync() = viewModelScope.launch(Dispatchers.IO) {
        _status.value = try {
            if (!repo.isEnrolled()) {
                "Appareil non associé"
            } else {
                val seq = repo.sync()
                _notes.value = repo.listNotes()
                "Synchronisé (seq $seq)"
            }
        } catch (e: Exception) {
            "Échec de la synchronisation : ${e.message}"
        }
    }

    fun pair(blob: String) = viewModelScope.launch(Dispatchers.IO) {
        _status.value = try {
            repo.pair(blob)
            _notes.value = repo.listNotes()
            "Appareil associé"
        } catch (e: Exception) {
            "Échec de l'association : ${e.message}"
        }
    }

    fun clearStatus() {
        _status.value = null
    }
}
