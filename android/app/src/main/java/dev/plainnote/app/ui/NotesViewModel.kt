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
import kotlinx.coroutines.withContext

/**
 * Holds the note list and the note being edited. Every facade call runs on the
 * IO dispatcher, never the main thread.
 */
class NotesViewModel(app: Application) : AndroidViewModel(app) {

    private val repo = NoteRepository(app)

    private val _notes = MutableStateFlow<List<NoteSummary>>(emptyList())
    val notes = _notes.asStateFlow()

    private val _editing = MutableStateFlow<NoteContent?>(null)
    val editing = _editing.asStateFlow()

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
        withContext(Dispatchers.Main) { _editing.value = null }
        _notes.value = repo.listNotes()
    }
}
