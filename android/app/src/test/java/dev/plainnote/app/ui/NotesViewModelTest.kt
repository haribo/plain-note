package dev.plainnote.app.ui

import android.app.Application
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import dev.plainnote.app.data.NoteRepository
import dev.plainnote.core.FolderInfo
import dev.plainnote.core.NoteContent
import dev.plainnote.core.NoteSummary
import dev.plainnote.core.NoteVersionInfo
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/**
 * In-memory [NoteRepository] for ViewModel tests: counts the calls that matter
 * and lets a test choose the enrollment / failure behaviour. No native lib.
 */
private class FakeRepo(
    var enrolled: Boolean = false,
    val failSync: Boolean = false,
) : NoteRepository {
    var syncCount = 0
    var pairCount = 0
    var setBodyCount = 0
    var restoreCount = 0
    val versions = mutableListOf<NoteVersionInfo>()

    override fun listNotes(folder: String?): List<NoteSummary> = emptyList()
    override fun search(query: String): List<NoteSummary> = emptyList()
    override fun listFolders(): List<FolderInfo> = emptyList()
    override fun createFolder(name: String, parent: String?): String = "f"
    override fun renameFolder(id: String, name: String) {}
    override fun moveFolder(id: String, parent: String?) {}
    override fun deleteFolder(id: String) {}
    override fun createNote(): String = "n"
    override fun moveNote(id: String, folder: String?) {}
    override fun setPinned(id: String, pinned: Boolean) {}
    override fun addTag(id: String, tag: String) {}
    override fun removeTag(id: String, tag: String) {}
    override fun trash(id: String) {}
    override fun getNote(id: String): NoteContent =
        NoteContent(id, "", "", "", emptyList(), false)

    override fun history(id: String): List<NoteVersionInfo> = versions.toList()
    override fun noteAt(id: String, versionId: String): NoteContent =
        NoteContent(id, "titre@$versionId", "corps@$versionId", "", emptyList(), false)

    override fun restoreVersion(id: String, versionId: String) {
        restoreCount++
    }

    override fun setTitle(id: String, title: String) {}
    override fun setBody(id: String, text: String) {
        setBodyCount++
    }

    override fun delete(id: String) {}
    override fun isEnrolled(): Boolean = enrolled
    override fun pair(blob: String) {
        pairCount++
        enrolled = true
    }

    override fun sync(): ULong {
        if (failSync) throw RuntimeException("offline")
        syncCount++
        return syncCount.toULong()
    }
}

@OptIn(ExperimentalCoroutinesApi::class)
@RunWith(AndroidJUnit4::class)
@Config(sdk = [34])
class NotesViewModelTest {

    private val dispatcher = StandardTestDispatcher()

    @Before
    fun setUp() = Dispatchers.setMain(dispatcher)

    @After
    fun tearDown() = Dispatchers.resetMain()

    private fun viewModel(repo: NoteRepository) =
        NotesViewModel(ApplicationProvider.getApplicationContext<Application>(), repo, dispatcher)

    @Test
    fun manual_sync_when_not_enrolled_reports_status_and_skips_sync() = runTest(dispatcher) {
        val repo = FakeRepo(enrolled = false)
        val vm = viewModel(repo)
        vm.sync()
        advanceUntilIdle()
        assertEquals("Appareil non associé", vm.status.value)
        assertEquals(0, repo.syncCount)
    }

    @Test
    fun manual_sync_when_enrolled_updates_state_and_status() = runTest(dispatcher) {
        val repo = FakeRepo(enrolled = true)
        val vm = viewModel(repo)
        vm.sync()
        advanceUntilIdle()
        assertEquals(SyncState.UpToDate, vm.syncState.value)
        assertEquals(1, repo.syncCount)
        assertTrue(vm.status.value!!.startsWith("Synchronisé"))
    }

    @Test
    fun sync_failure_sets_offline() = runTest(dispatcher) {
        val repo = FakeRepo(enrolled = true, failSync = true)
        val vm = viewModel(repo)
        vm.sync()
        advanceUntilIdle()
        assertEquals(SyncState.Offline, vm.syncState.value)
    }

    @Test
    fun pair_reports_association() = runTest(dispatcher) {
        val repo = FakeRepo(enrolled = false)
        val vm = viewModel(repo)
        vm.pair("blob")
        advanceUntilIdle()
        assertEquals("Appareil associé", vm.status.value)
        assertEquals(1, repo.pairCount)
    }

    @Test
    fun history_preview_and_restore() = runTest(dispatcher) {
        val repo = FakeRepo()
        repo.versions.add(NoteVersionInfo("v-new", 2000))
        repo.versions.add(NoteVersionInfo("v-old", 1000))
        val vm = viewModel(repo)
        vm.open("n")
        advanceUntilIdle()

        vm.openHistory()
        advanceUntilIdle()
        assertEquals(2, vm.history.value?.size)

        vm.previewVersion("v-old")
        advanceUntilIdle()
        assertEquals("n", vm.versionPreview.value?.id)

        vm.restoreVersion("v-old")
        advanceUntilIdle()
        assertEquals(1, repo.restoreCount)
        assertNull("history closes after restore", vm.history.value)
        assertNull("preview closes after restore", vm.versionPreview.value)
    }

    @Test
    fun rapid_edits_debounce_to_a_single_auto_sync() = runTest(dispatcher) {
        val repo = FakeRepo(enrolled = true)
        val vm = viewModel(repo)
        vm.saveBody("id", "a")
        vm.saveBody("id", "ab")
        vm.saveBody("id", "abc")
        advanceUntilIdle() // three saves + the debounce window + one sync
        assertEquals(3, repo.setBodyCount)
        assertEquals(1, repo.syncCount)
    }
}
