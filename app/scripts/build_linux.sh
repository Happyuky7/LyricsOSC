#!/usr/bin/env bash
# Builds the Linux package: dist/LyricsOSC-<ver>-linux-x86_64.tar.gz (contains install.sh)
#
# On Linux:            ./scripts/build_linux.sh
# From Windows/macOS:  ./scripts/build_linux_docker.sh   (builds inside a Rust container)
set -euo pipefail
cd "${APP_DIR:-$(dirname "$0")/..}"
VER=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
TARGET_DIR="${CARGO_TARGET_DIR:-target}"
cargo build --release
D=dist/linux/LyricsOSC-$VER
rm -rf dist/linux && mkdir -p "$D/python" "$D/scripts" "$D/assets"
cp "$TARGET_DIR/release/lyricsosc" "$D/"
cp python/whisper_sidecar.py python/requirements.txt "$D/python/"
cp scripts/setup_voice.sh "$D/scripts/"
cp assets/banner.png assets/logo.png assets/icon.png assets/LICENSE-ART.md "$D/assets/"  # runtime artwork only
cp ../LICENSE ../README.md ../THIRD_PARTY_NOTICES.md installer/linux/install.sh "$D/"
chmod +x "$D/lyricsosc" "$D/install.sh" "$D/scripts/setup_voice.sh"
tar -czf "dist/LyricsOSC-$VER-linux-x86_64.tar.gz" -C dist/linux "LyricsOSC-$VER"
echo "-> dist/LyricsOSC-$VER-linux-x86_64.tar.gz"
