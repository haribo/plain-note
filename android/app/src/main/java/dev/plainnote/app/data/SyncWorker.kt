package dev.plainnote.app.data

import android.content.Context
import androidx.work.CoroutineWorker
import androidx.work.WorkerParameters
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * Periodic background sync. Runs in the app process and reuses the shared
 * [NoteRepository] from [AppGraph], so store access stays serialized by the same
 * in-process lock as the foreground app. No-op when the device isn't enrolled.
 */
class SyncWorker(context: Context, params: WorkerParameters) :
    CoroutineWorker(context, params) {

    override suspend fun doWork(): Result = withContext(Dispatchers.IO) {
        val repo = AppGraph.repository(applicationContext)
        try {
            if (repo.isEnrolled()) repo.sync()
            Result.success()
        } catch (e: Exception) {
            // Transient (offline, relay down) — let WorkManager back off and retry.
            Result.retry()
        }
    }
}
