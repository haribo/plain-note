# UI change workflow

See also: [git-workflow.md](git-workflow.md) for branching and PR rules,
[android/README.md](android/README.md) for the Android test layers.

Two gates protect UI-modifying changes: a **mockup** validated *before* code (a
design decision), and a **visual check** validated *before* merge (conformance to
that mockup). Scope: the **Android** app — the only actively-developed visual
surface. GTK, CLI and the relay have no gate yet (no headless display, or no
visual surface).

## Gate 1 — mockup-first (before writing code)

Before any UI-modifying change — **including additions to an existing surface** —
produce a mockup with the Artifact tool and obtain the user's **explicit
validation** before writing code.

- Static HTML of the touched surfaces **and their states**.
- **Light and dark** whenever theming is relevant.
- **Realistic data, including edge cases**: long free-form text, crowded lists,
  empty states.
- When variants are debated, show them **side by side** in the one artifact.

The HTML mockup validates **layout, flow and states** — not pixel-level Material
fidelity. Its role is to lock the design; the pixel rendering is confirmed at
Gate 2.

**Exemptions** (no mockup required):

- provably pixel-identical refactors (state the diff evidence);
- fixes restoring an existing rendering with no new surface.

## Gate 2 — visual validation (before merge)

After implementing, verify **conformance to the validated mockup** — never
discover the design here.

### Automated conformance — Roborazzi (preferred, CI-enforced)

Read-only rendering surfaces are frozen as golden PNGs, both themes, verified on
every PR (the `android tests` CI job runs `verifyRoborazziDebug`). See
[android/README.md](android/README.md).

```sh
./gradlew :app:recordRoborazziDebug   # regenerate goldens after an intended change
./gradlew :app:verifyRoborazziDebug   # fail on any visual diff
```

Add or re-record a golden for any new read-only surface in the same PR.

### Manual screenshots — interactive states (shared in the conversation)

Roborazzi renders composables in isolation, so states that need a running app
(keyboard open, bottom sheet, dialog) are captured from the emulator:

```sh
# Emulator only — never a physical device connected in parallel.
ANDROID_SERIAL=emulator-5554 adb -s emulator-5554 exec-out screencap -p > <file>.png
```

- Capture each modified surface: **initial state + the principal interaction
  state**, in **both themes** (theme-blind reasoning on a diff is unreliable).
  Add breakpoint variants only when the change is responsive-sensitive.
- Screenshots live in the **scratchpad**. **Never** put screenshot paths in the
  PR body, a commit message, or any git-tracked file — they are local and rot
  after merge.
- Share the images **in the conversation** for the user's eye, and state **what
  to check**: one line per screenshot naming the elements/states and the expected
  outcome (e.g. "vérifier que la barre de formatage reste au-dessus du clavier,
  gras/italique lisibles en dark"). For an exempt pixel-identical change, say
  explicitly there is nothing to inspect and that validation covers the proof.

## Relation to tribnest

Adapted from the tribnest project's two-gate workflow. **Not** ported here:
Playwright, `tmp/branch/` keying and the web `setTheme` helper — there is no web
frontend. The automated gate is Roborazzi (committed goldens + CI) rather than a
one-off eyeball, and screenshots are shared in the conversation rather than via
local paths.
