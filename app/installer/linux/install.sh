#!/usr/bin/env bash
# LyricsOSC — Linux installer. Asks for the install folder, copies the app,
# sets up the voice engine automatically and adds a menu entry.
#   ./install.sh                 (interactive)
#   ./install.sh /opt/lyricsosc  (non-interactive)
set -euo pipefail
SRC="$(cd "$(dirname "$0")" && pwd)"
DEFAULT="$HOME/.local/share/lyricsosc"
DEST="${1:-}"
if [ -z "$DEST" ]; then
  read -r -p "Install folder [$DEFAULT]: " DEST
  DEST="${DEST:-$DEFAULT}"
fi
DEST="$(realpath -m "$DEST")"
echo "Installing LyricsOSC into $DEST"
mkdir -p "$DEST"
cp -r "$SRC/lyricsosc" "$SRC/python" "$SRC/scripts" "$SRC/assets" "$SRC/LICENSE" "$SRC/README.md" "$SRC/THIRD_PARTY_NOTICES.md" "$DEST/"
chmod +x "$DEST/lyricsosc" "$DEST/scripts/setup_voice.sh"

for dep in pactl parec; do
  command -v "$dep" >/dev/null || echo "⚠ '$dep' not found: install pulseaudio-utils (or pipewire-pulse) for voice capture."
done

read -r -p "Install the voice engine (Python + Whisper, ~0.5 GB)? [Y/n] " yn
case "${yn:-Y}" in [Nn]*) ;; *) "$DEST/scripts/setup_voice.sh" "$DEST" ;; esac

APPS="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
mkdir -p "$APPS"
cat > "$APPS/lyricsosc.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=LyricsOSC
Comment=Synced lyrics for the VRChat chatbox
Exec="$DEST/lyricsosc"
Path=$DEST
Icon=$DEST/assets/icon.png
Categories=AudioVideo;Audio;
Terminal=false
EOF
cat > "$DEST/uninstall.sh" <<EOF
#!/usr/bin/env bash
rm -rf "$DEST" "$APPS/lyricsosc.desktop"
echo "LyricsOSC removed."
EOF
chmod +x "$DEST/uninstall.sh"
echo "✔ Done. Launch 'LyricsOSC' from your menu or run: $DEST/lyricsosc"
