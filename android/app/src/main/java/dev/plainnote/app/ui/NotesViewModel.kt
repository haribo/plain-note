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

    /** Report a facade failure without crashing the app. */
    private fun report(e: Exception) {
        _status.value = "Erreur : ${e.message}"
    }

    fun refresh() = viewModelScope.launch(Dispatchers.IO) {
        try {
            _notes.value = repo.listNotes()
        } catch (e: Exception) {
            report(e)
        }
    }

    fun createAndOpen() = viewModelScope.launch(Dispatchers.IO) {
        try {
            val id = repo.createNote()
            _editing.value = repo.getNote(id)
            _notes.value = repo.listNotes()
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
            _notes.value = repo.listNotes()
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
