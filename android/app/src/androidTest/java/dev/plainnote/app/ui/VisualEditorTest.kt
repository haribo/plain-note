package dev.plainnote.app.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.isToggleable
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test

/**
 * Instrumented Compose UI tests (L2) for the WYSIWYG editor gestures. They run
 * on a device/emulator: the UniFFI native lib is packaged in the test APK, so
 * `markdownToDoc`/`docToMarkdown` work. Hosts `VisualEditor` in isolation.
 */
class VisualEditorTest {

    @get:Rule
    val rule = createComposeRule()

    private fun host(initial: String, onBody: (String) -> Unit) {
        rule.setContent {
            MaterialTheme {
                VisualEditor(initialMarkdown = initial, onBodyChange = onBody)
            }
        }
    }

    @Test
    fun typing_updates_body() {
        var body = ""
        host("") { body = it }
        rule.onNode(hasSetTextAction()).performTextInput("hello")
        rule.runOnIdle { assertEquals("hello", body) }
    }

    @Test
    fun heading_button_makes_the_current_line_a_heading() {
        var body = ""
        host("hello") { body = it }
        rule.onNodeWithContentDescription("Titre").performClick()
        rule.runOnIdle { assertEquals("# hello", body) }
    }

    @Test
    fun bullet_button_makes_a_list_item() {
        var body = ""
        host("hello") { body = it }
        rule.onNodeWithContentDescription("Liste à puces").performClick()
        rule.runOnIdle { assertEquals("- hello", body) }
    }

    @Test
    fun checkbox_toggles_the_task() {
        var body = ""
        host("- [ ] a") { body = it }
        rule.onNode(isToggleable()).performClick()
        rule.runOnIdle { assertEquals("- [x] a", body) }
    }

    @Test
    fun heading_button_cycles_levels() {
        var body = ""
        host("hello") { body = it }
        val btn = rule.onNodeWithTag("heading-level")
        btn.performClick(); rule.runOnIdle { assertEquals("# hello", body) }
        btn.performClick(); rule.runOnIdle { assertEquals("## hello", body) }
        btn.performClick(); rule.runOnIdle { assertEquals("### hello", body) }
        btn.performClick(); rule.runOnIdle { assertEquals("hello", body) }
    }

    @Test
    fun newline_splits_into_two_paragraphs() {
        var body = ""
        host("") { body = it }
        rule.onNode(hasSetTextAction()).performTextInput("a\nb")
        rule.runOnIdle { assertEquals("a\n\nb", body) }
    }
}
