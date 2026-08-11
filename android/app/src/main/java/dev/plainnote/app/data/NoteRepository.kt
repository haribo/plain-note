package dev.plainnote.app.data

import android.content.Context
import dev.plainnote.core.FolderInfo
import dev.plainnote.core.NoteApp
import dev.plainnote.core.NoteContent
import dev.plainnote.core.NoteSummary
import dev.plainnote.core.NoteVersionInfo
import java.io.File

/**
 * Store + sync operations the app needs, as an interface so the ViewModel can be
 * unit-tested against a fake. The production implementation is
 * [NativeNoteRepository], backed by the Rust [NoteApp] facade.
 */
interface NoteRepository {

    // --- local ---

    /** Notes in `folder` (null = all folders), pinned first. */
    fun listNotes(folder: String? = null): List<NoteSummary>

    fun search(query: String): List<NoteSummary>

    fun listFolders(): List<FolderInfo>

    fun createFolder(name: String, parent: String? = null): String

    fun renameFolder(id: String, name: String)

    fun moveFolder(id: String, parent: String?)

    fun deleteFolder(id: String)

    fun createNote(): String

    fun moveNote(id: String, folder: String?)

    fun setPinned(id: String, pinned: Boolean)

    fun addTag(id: String, tag: String)

    fun removeTag(id: String, tag: String)

    fun trash(id: String)

    fun getNote(id: String): NoteContent

    /** A note's version timeline, newest first. */
    fun history(id: String): List<NoteVersionInfo>

    /** A note's content at a given version. */
    fun noteAt(id: String, versionId: String): NoteContent

    /** Restore a note to a past version (a new forward edit). */
    fun restoreVersion(id: String, versionId: String)

    fun setTitle(id: String, title: String)

    fun setBody(id: String, text: String)

    fun delete(id: String)

    // --- sync & pairing ---

    fun isEnrolled(): Boolean

    /** Join a group from a pairing blob (scanned from a QR, or pasted). */
    fun pair(blob: String)

    /** One-shot push/pull; returns the new sequence number. */
    fun sync(): ULong
}

/**
 * Thin wrapper over the Rust [NoteApp] facade. All calls are blocking and must
 * run off the main thread (the ViewModel dispatches them on IO).
 *
 * The store and the enrollment config live in the app's private files
 * directory; the Rust side creates the parent directories on first use.
 */
class NativeNoteRepository(context: Context) : NoteRepository {

    private val root = File(context.filesDir, "plain-note")

    private val app: NoteApp = NoteApp(
        File(root, "store.automerge").absolutePath,
        File(root, "config.json").absolutePath,
    )

    override fun listNotes(folder: String?): List<NoteSummary> = app.listNotes(folder, null)

    override fun search(query: String): List<NoteSummary> = app.search(query)

    override fun listFolders(): List<FolderInfo> = app.listFolders()

    override fun createFolder(name: String, parent: String?): String = app.createFolder(name, parent)

    override fun renameFolder(id: String, name: String) = app.renameFolder(id, name)

    override fun moveFolder(id: String, parent: String?) = app.moveFolder(id, parent)

    override fun deleteFolder(id: String) = app.deleteFolder(id)

    override fun createNote(): String = app.createNote()

    override fun moveNote(id: String, folder: String?) = app.moveNote(id, folder)

    override fun setPinned(id: String, pinned: Boolean) = app.setPinned(id, pinned)

    override fun addTag(id: String, tag: String) = app.addTag(id, tag)

    override fun removeTag(id: String, tag: String) = app.removeTag(id, tag)

    override fun trash(id: String) = app.trash(id)

    override fun getNote(id: String): NoteContent = app.getNote(id)

    override fun history(id: String): List<NoteVersionInfo> = app.history(id)

    override fun noteAt(id: String, versionId: String): NoteContent = app.noteAt(id, versionId)

    override fun restoreVersion(id: String, versionId: String) = app.restoreVersion(id, versionId)

    override fun setTitle(id: String, title: String) = app.setTitle(id, title)

    override fun setBody(id: String, text: String) = app.setBody(id, text)

    override fun delete(id: String) = app.delete(id)

    override fun isEnrolled(): Boolean = app.isEnrolled()

    override fun pair(blob: String) = app.pair(blob)

    override fun sync(): ULong = app.sync()
}
