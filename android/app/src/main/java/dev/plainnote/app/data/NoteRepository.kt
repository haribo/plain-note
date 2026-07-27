package dev.plainnote.app.data

import android.content.Context
import dev.plainnote.core.NoteApp
import dev.plainnote.core.NoteContent
import dev.plainnote.core.NoteSummary
import java.io.File

/**
 * Thin wrapper over the Rust [NoteApp] facade. All calls are blocking and must
 * run off the main thread (the ViewModel dispatches them on IO).
 *
 * The store lives in the app's private files directory; the Rust side creates
 * the parent directory on first use.
 */
class NoteRepository(context: Context) {

    private val app: NoteApp = NoteApp(
        File(context.filesDir, "plain-note/store.automerge").absolutePath,
    )

    fun listNotes(): List<NoteSummary> = app.listNotes(null, null)

    fun createNote(): String = app.createNote()

    fun getNote(id: String): NoteContent = app.getNote(id)

    fun setTitle(id: String, title: String) = app.setTitle(id, title)

    fun setBody(id: String, text: String) = app.setBody(id, text)

    fun delete(id: String) = app.delete(id)
}
