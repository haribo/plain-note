#!/bin/bash
# Runs INSIDE the container. Seeds a fixed note, launches the GUI on a virtual
# display, opens the note, and screenshots the whole screen to /out/editor.png.
set -euo pipefail
BIN=/work/target/debug
work=$(mktemp -d)
export HOME="$work" DISPLAY=:99 PN_STORE="$work/store.automerge"

# A fixed note exercising the inline marks + an unterminated marker.
cat > "$work/ed" <<'BODY'
#!/bin/sh
cat > "$1" <<'TXT'
# Réunion produit
## Sous-titre
### Section

Un point **très important** et de l'*italique* et du `code`, plus ~~barré~~.

un **marqueur pas fermé
TXT
BODY
chmod +x "$work/ed"
EDITOR="$work/ed" "$BIN/pn" new --title "Aperçu WYSIWYG" --edit >/dev/null

Xvfb :99 -screen 0 1280x900x24 -nolisten tcp >/dev/null 2>&1 &
sleep 2
"$BIN/plain-note-gui" >/dev/null 2>&1 &
sleep 6
# Open the (only) note from the sidebar, then let it render.
xdotool mousemove 124 124 click 1 || true
sleep 3
import -window root /out/editor.png
