package dev.plainnote.app.ui

import android.app.Application
import androidx.compose.material3.MaterialTheme
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.RobolectricDeviceQualifiers
import com.github.takahirom.roborazzi.captureRoboImage
import dev.plainnote.app.data.NoteRepository
import dev.plainnote.core.FolderInfo
import dev.plainnote.core.NoteContent
import dev.plainnote.core.NoteSummary
import dev.plainnote.core.NoteVersionInfo
import kotlinx.coroutines.Dispatchers
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * Scenario captures (issue #206 pilot): drive [AppRoot] through the
 * "create a note" use case and snapshot each step as a committed golden, so a
 * reviewer can follow the flow from images instead of replaying it by hand.
 *
 * Steps: initial list → editor open (empty) → text typed → back to the list.
 * Goldens live under `src/test/roborazzi/scenarios/`, named
 * `<use-case>__stepN-<state>_<theme>.png`.
 *
 * The store is a stateful in-memory [NoteRepository]; the editor surfaces
 * (DocView, VisualEditor) cross the real UniFFI boundary via the host build of
 * `libplain_note_mobile.so` (see `cargoBuildHost` in `app/build.gradle.kts`).
 *
 * Record: `./gradlew :app:recordRoborazziDebug`.
 * Verify: `./gradlew :app:verifyRoborazziDebug`.
 */
@RunWith(AndroidJUnit4::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [34], qualifiers = RobolectricDeviceQualifiers.Pixel5)
class CreateNoteScenarioTest {

    @get:Rule
    val compose = createComposeRule()

    /** In-memory store faithful to the contract the list screen relies on. */
    private class ScenarioRepo : NoteRepository {
        private class Note(
            var title: String = "",
            var body: String = "",
            var folder: String = "",
            val tags: MutableList<String> = mutableListOf(),
            var pinned: Boolean = false,
        )

        private val notes = LinkedHashMap<String, Note>()
        private var nextId = 0

        fun seed(title: String, body: String, tags: List<String> = emptyList(), pinned: Boolean = false): String {
            val id = createNote()
            notes.getValue(id).let {
                it.title = title
                it.body = body
                it.tags += tags
                it.pinned = pinned
            }
            return id
        }

        // Pinned first, like the real store.
        override fun listNotes(folder: String?): List<NoteSummary> =
            notes.entries
                .map { (id, n) -> NoteSummary(id, n.title, n.folder, n.tags.toList(), n.pinned, 0L) }
                .sortedByDescending { it.pinned }

        override fun search(query: String): List<NoteSummary> =
            listNotes(null).filter { it.title.contains(query, ignoreCase = true) }

        override fun listFolders(): List<FolderInfo> = emptyList()
        override fun createFolder(name: String, parent: String?): String = "f"
        override fun renameFolder(id: String, name: String) {}
        override fun moveFolder(id: String, parent: String?) {}
        override fun deleteFolder(id: String) {}

        override fun createNote(): String {
            val id = "n${nextId++}"
            notes[id] = Note()
            return id
        }

        override fun moveNote(id: String, folder: String?) {}
        override fun setPinned(id: String, pinned: Boolean) {
            notes.getValue(id).pinned = pinned
        }

        override fun addTag(id: String, tag: String) {
            notes.getValue(id).tags += tag
        }

        override fun removeTag(id: String, tag: String) {
            notes.getValue(id).tags -= tag
        }

        override fun trash(id: String) {
            notes.remove(id)
        }

        override fun getNote(id: String): NoteContent =
            notes.getValue(id).let { NoteContent(id, it.title, it.body, it.folder, it.tags.toList(), it.pinned) }

        override fun history(id: String): List<NoteVersionInfo> = emptyList()
        override fun noteAt(id: String, versionId: String): NoteContent = getNote(id)
        override fun restoreVersion(id: String, versionId: String) {}

        override fun setTitle(id: String, title: String) {
            notes.getValue(id).title = title
        }

        override fun setBody(id: String, text: String) {
            notes.getValue(id).body = text
        }

        override fun delete(id: String) {
            notes.remove(id)
        }

        override fun isEnrolled(): Boolean = false
        override fun pair(blob: String) {}
        override fun sync(): ULong = 0uL
    }

    private fun runScenario(dark: Boolean) {
        val theme = if (dark) "dark" else "light"
        val repo = ScenarioRepo().apply {
            // Realistic pre-existing data, including edge cases: a pinned note,
            // tags, and a title long enough to test truncation.
            seed("Idées cadeaux", "- livre\n- plante", tags = listOf("perso"), pinned = true)
            seed("Compte-rendu de la réunion copropriété du 12 mars", "Travaux votés.")
            seed("Recette pancakes", "Farine, œufs, lait.", tags = listOf("cuisine", "brunch"))
        }
        // Unconfined io: repository calls are in-memory and instant, so state
        // updates land before the next frame without a test-dispatcher pump.
        val vm = NotesViewModel(
            ApplicationProvider.getApplicationContext<Application>(),
            repo,
            Dispatchers.Unconfined,
        )

        fun snap(step: String) {
            compose.waitForIdle()
            compose.onRoot()
                .captureRoboImage("src/test/roborazzi/scenarios/create-note__${step}_$theme.png")
        }

        compose.setContent {
            MaterialTheme(colorScheme = if (dark) DarkColors else LightColors) { AppRoot(vm) }
        }

        snap("step1-list")

        compose.onNodeWithContentDescription("Nouvelle note").performClick()
        compose.waitForIdle()
        compose.onNodeWithContentDescription("Éditer").performClick()
        snap("step2-editor-open")

        compose.onNodeWithText("Titre").performTextInput("Courses du samedi")
        // The body is typed in the raw-Markdown editor: the pilot validates the
        // flow, not the WYSIWYG surface (covered by VisualEditor tests).
        compose.onNodeWithContentDescription("Éditeur visuel / Markdown").performClick()
        compose.waitForIdle()
        // Two editable fields on screen: [0] the title, [1] the Markdown body.
        compose.onAllNodes(hasSetTextAction())[1]
            .performTextInput("- pommes\n- lait\n\nPasser au **marché** avant midi.")
        snap("step3-text-typed")

        compose.onNodeWithContentDescription("Terminé").performClick()
        compose.waitForIdle()
        compose.onNodeWithContentDescription("Retour").performClick()
        snap("step4-back-to-list")
    }

    @Test
    fun create_note_light() = runScenario(dark = false)

    @Test
    fun create_note_dark() = runScenario(dark = true)
}
