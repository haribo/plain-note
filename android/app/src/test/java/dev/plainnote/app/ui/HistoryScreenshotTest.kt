package dev.plainnote.app.ui

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.RobolectricDeviceQualifiers
import com.github.takahirom.roborazzi.captureRoboImage
import dev.plainnote.core.NoteVersionInfo
import java.util.TimeZone
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * Screenshot regression (L3): render the read-only [HistoryTimeline] through
 * Robolectric and diff against golden PNGs. The timezone is pinned to UTC so the
 * formatted dates are deterministic across machines.
 *
 * Record: `./gradlew :app:recordRoborazziDebug`.
 */
@RunWith(AndroidJUnit4::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [34], qualifiers = RobolectricDeviceQualifiers.Pixel5)
class HistoryScreenshotTest {

    @get:Rule
    val compose = createComposeRule()

    private val sample = listOf(
        NoteVersionInfo("v5", 1_712_150_640_000L),
        NoteVersionInfo("v4", 1_712_064_240_000L),
        NoteVersionInfo("v3", 1_711_900_000_000L),
        NoteVersionInfo("v2", 1_711_000_000_000L),
        NoteVersionInfo("v1", 1_709_460_000_000L),
    )

    @Before
    fun pinTimezone() {
        TimeZone.setDefault(TimeZone.getTimeZone("UTC"))
    }

    private fun capture(dark: Boolean, path: String) {
        compose.setContent {
            MaterialTheme(colorScheme = if (dark) DarkColors else LightColors) {
                Surface {
                    HistoryTimeline(
                        sample,
                        onSelect = {},
                        Modifier.fillMaxWidth().height(360.dp),
                    )
                }
            }
        }
        compose.onRoot().captureRoboImage(path)
    }

    @Test
    fun history_light() = capture(dark = false, path = "src/test/roborazzi/history_light.png")

    @Test
    fun history_dark() = capture(dark = true, path = "src/test/roborazzi/history_dark.png")
}
