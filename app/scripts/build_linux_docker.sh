#!/usr/bin/env bash
# Builds the Linux package inside a Rust container (works from Windows, macOS or Linux).
# Needs Docker. Output: dist/LyricsOSC-<ver>-linux-x86_64.tar.gz
#
# Debian bookworm (glibc 2.36) is used so the binary also runs on older distros.
set -euo pipefail
cd "$(dirname "$0")/.."
APP_DIR="$(pwd)"
# Git Bash on Windows rewrites /paths: disable that for the docker arguments
export MSYS_NO_PATHCONV=1

docker run --rm \
  -v "$APP_DIR/..:/repo" \
  -v lyricsosc-cargo-registry:/usr/local/cargo/registry \
  -v lyricsosc-target-linux:/target-linux \
  -e CARGO_TARGET_DIR=/target-linux \
  -e APP_DIR=/repo/app \
  -w /repo/app \
  rust:1-bookworm \
  bash -c '
    set -e
    apt-get update -qq
    apt-get install -y -qq pkg-config libxkbcommon-dev libwayland-dev libgl1-mesa-dev libx11-dev >/dev/null
    tr -d "\r" < scripts/build_linux.sh > /tmp/build_linux.sh
    bash /tmp/build_linux.sh
  '
