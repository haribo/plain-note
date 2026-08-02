package dev.plainnote.app.data

import android.content.Context
import dev.plainnote.core.FolderInfo
import dev.plainnote.core.NoteApp
import dev.plainnote.core.NoteContent
import dev.plainnote.core.NoteSummary
import java.io.File

/**
 * Thin wrapper over the Rust [NoteApp] facade. All calls are blocking and must
 * run off the main thread (the ViewModel dispatches them on IO).
 *
 * The store and the enrollment config live in the app's private files
 * directory; the Rust side creates the parent directories on first use.
 */
class NoteRepository(context: Context) {

    private val root = File(context.filesDir, "plain-note")

    private val app: NoteApp = NoteApp(
        File(root, "store.automerge").absolutePath,
        File(root, "config.json").absolutePath,
    )

    // --- local ---

    /** Notes in `folder` (null = all folders), pinned first. */
    fun listNotes(folder: String? = null): List<NoteSummary> = app.listNotes(folder, null)

    fun search(query: String): List<NoteSummary> = app.search(query)

    fun listFolders(): List<FolderInfo> = app.listFolders()

    fun createFolder(name: String, parent: String? = null): String = app.createFolder(name, parent)

    fun renameFolder(id: String, name: String) = app.renameFolder(id, name)

    fun moveFolder(id: String, parent: String?) = app.moveFolder(id, parent)

    fun deleteFolder(id: String) = app.deleteFolder(id)

    fun createNote(): String = app.createNote()

    fun moveNote(id: String, folder: String?) = app.moveNote(id, folder)

    fun setPinned(id: String, pinned: Boolean) = app.setPinned(id, pinned)

    fun addTag(id: String, tag: String) = app.addTag(id, tag)

    fun removeTag(id: String, tag: String) = app.removeTag(id, tag)

    fun trash(id: String) = app.trash(id)

    fun getNote(id: String): NoteContent = app.getNote(id)

    fun setTitle(id: String, title: String) = app.setTitle(id, title)

    fun setBody(id: String, text: String) = app.setBody(id, text)

    fun delete(id: String) = app.delete(id)

    // --- sync & pairing ---

    fun isEnrolled(): Boolean = app.isEnrolled()

    /** Join a group from a pairing blob (scanned from a QR, or pasted). */
    fun pair(blob: String) = app.pair(blob)

    /** One-shot push/pull; returns the new sequence number. */
    fun sync(): ULong = app.sync()
}
