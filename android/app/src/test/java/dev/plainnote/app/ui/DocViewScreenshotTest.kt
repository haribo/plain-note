package dev.plainnote.app.ui

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onRoot
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.RobolectricDeviceQualifiers
import com.github.takahirom.roborazzi.captureRoboImage
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * Screenshot regression tests (L3): render the read-only [DocLines] through
 * Robolectric (JVM, deterministic) and diff against committed golden PNGs.
 *
 * Uses hand-built lines so no UniFFI FFI call happens — the native `.so` is an
 * Android ELF and cannot load on the JVM host.
 *
 * Record: `./gradlew :app:recordRoborazziDebug`.
 * Verify: `./gradlew :app:verifyRoborazziDebug`.
 */
@RunWith(AndroidJUnit4::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [34], qualifiers = RobolectricDeviceQualifiers.Pixel5)
class DocViewScreenshotTest {

    @get:Rule
    val compose = createComposeRule()

    private val sample = listOf(
        Line(0, LineKind.Heading, "Réunion produit", level = 1),
        Line(1, LineKind.Paragraph, "Un point **très important** et de l'*italique*."),
        Line(2, LineKind.Task, "préparer le slide", checked = false),
        Line(3, LineKind.Task, "envoyer l'invitation", checked = true),
        Line(4, LineKind.Bullet, "ordre du jour"),
        Line(5, LineKind.Quote, "Penser au budget"),
        Line(6, LineKind.Code, "let x = 1"),
    )

    private fun capture(dark: Boolean, path: String) {
        compose.setContent {
            MaterialTheme(colorScheme = if (dark) darkColorScheme() else lightColorScheme()) {
                Surface { DocLines(sample, Modifier.fillMaxWidth()) }
            }
        }
        compose.onRoot().captureRoboImage(path)
    }

    @Test
    fun docview_light() = capture(dark = false, path = "src/test/roborazzi/docview_light.png")

    @Test
    fun docview_dark() = capture(dark = true, path = "src/test/roborazzi/docview_dark.png")
}
