#!/usr/bin/env bash
# Headless GTK screenshot check (local; the GUI can't be captured in CI).
#   tools/gui-screenshot/run.sh record   # regenerate the golden
#   tools/gui-screenshot/run.sh verify   # fail on a visual diff (default)
# Renders the editor in a pinned Docker + Xvfb environment, isolated from the
# host session. See README.md.
set -euo pipefail
cd "$(dirname "$0")/../.."
mode="${1:-verify}"
here="tools/gui-screenshot"

docker build -t pn-gui-test "$here" >/dev/null
cargo build -p plain-note-gui -p note-cli
out="$(mktemp -d)"
docker run --rm -v "$PWD:/work:ro" -v "$out:/out" pn-gui-test bash "/work/$here/capture.sh"

if [ "$mode" = record ]; then
  cp "$out/editor.png" "$here/golden/editor.png"
  echo "golden recorded: $here/golden/editor.png"
  exit 0
fi

# Fuzzy compare tolerates sub-pixel AA noise but catches real changes.
# `compare -metric AE` prints e.g. "0" or "123 (0.001)"; keep the leading count.
raw="$(compare -metric AE -fuzz 4% "$here/golden/editor.png" "$out/editor.png" "$out/diff.png" 2>&1 || true)"
diff="${raw%% *}"
echo "differing pixels: $diff"
# The count may be a float on HDRI ImageMagick builds (e.g. "78.8863"), so a
# shell integer test would error and flag every nonzero diff. Compare as a float.
if ! [[ "$diff" =~ ^[0-9]+([.][0-9]+)?$ ]]; then
  echo "could not parse compare output: $raw"
  exit 1
fi
if ! awk -v d="$diff" 'BEGIN { exit (d <= 500) ? 0 : 1 }'; then
  echo "VISUAL REGRESSION (diff=$diff, threshold 500). Diff at $out/diff.png"
  echo "If the change is intended: tools/gui-screenshot/run.sh record"
  exit 1
fi
echo "golden OK"
