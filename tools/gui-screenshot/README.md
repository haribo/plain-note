# GTK screenshot check

A deterministic, headless screenshot test for the GTK editor — the desktop
counterpart of the Android Roborazzi goldens. It renders the app in a pinned
**Docker + Xvfb** environment (isolated from your host session; nothing touches
your real display) and diffs the result against a committed golden.

## Usage

```sh
tools/gui-screenshot/run.sh verify   # fail on a visual diff (default)
tools/gui-screenshot/run.sh record   # regenerate the golden after an intended change
```

Requires Docker. The first run builds the image and the app (subsequent runs are
faster).

## Why local, not per-PR CI

Like the Android Compose UI tests, this needs a display environment and is run
locally before GTK changes, not on every PR. The pure logic (`inline_spans`,
`toggle_wrap`, …) stays covered by `cargo test` in CI.

## How it works

`capture.sh` (inside the container) seeds a fixed note, starts the GUI on Xvfb,
opens the note, and screenshots to `/out/editor.png`. `run.sh` compares it to
`golden/editor.png` with a small fuzz tolerance.
