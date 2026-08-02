package dev.plainnote.app

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.work.Constraints
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.NetworkType
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import dev.plainnote.app.data.SyncWorker
import java.util.concurrent.TimeUnit
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import dev.plainnote.app.ui.AppRoot
import dev.plainnote.app.ui.NotesViewModel

class MainActivity : ComponentActivity() {

    private val vm: NotesViewModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Draw behind the system bars so the format toolbar can float above the keyboard.
        enableEdgeToEdge()
        // Automatic sync when the app comes to the foreground (open / resume).
        lifecycle.addObserver(
            LifecycleEventObserver { _, event ->
                if (event == Lifecycle.Event.ON_START) vm.onForeground()
            },
        )
        scheduleBackgroundSync()
        setContent {
            val colors = if (isSystemInDarkTheme()) darkColorScheme() else lightColorScheme()
            MaterialTheme(colorScheme = colors) {
                Surface {
                    AppRoot(vm)
                }
            }
        }
    }

    /** Enqueue the periodic background sync once (kept across launches). */
    private fun scheduleBackgroundSync() {
        val work = PeriodicWorkRequestBuilder<SyncWorker>(15, TimeUnit.MINUTES)
            .setConstraints(
                Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build(),
            )
            .build()
        WorkManager.getInstance(this).enqueueUniquePeriodicWork(
            "auto-sync",
            ExistingPeriodicWorkPolicy.KEEP,
            work,
        )
    }
}
