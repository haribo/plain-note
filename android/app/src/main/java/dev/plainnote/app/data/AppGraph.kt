package dev.plainnote.app.data

import android.content.Context

/**
 * Process-wide holder for the single [NoteRepository] (and therefore the single
 * `NoteApp`/store). Every component — the ViewModel and the background sync
 * worker — must go through here: two `NoteApp` instances on the same store file
 * would hold independent in-process locks and could corrupt the automerge store.
 */
object AppGraph {
    @Volatile
    private var repo: NoteRepository? = null

    fun repository(context: Context): NoteRepository =
        repo ?: synchronized(this) {
            repo ?: NoteRepository(context.applicationContext).also { repo = it }
        }
}
