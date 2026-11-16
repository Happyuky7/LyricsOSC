#!/usr/bin/env bash
# LyricsOSC - voice engine setup (Linux). Run by install.sh; safe to re-run.
set -euo pipefail
INSTALL_DIR="${1:-$(cd "$(dirname "$0")/.." && pwd)}"
MODEL="${MODEL:-small}"
PY="$INSTALL_DIR/python"; VENV="$PY/.venv"; TOOLS="$INSTALL_DIR/tools"; UV="$TOOLS/uv"
export UV_PYTHON_INSTALL_DIR="$PY/runtime" UV_CACHE_DIR="$PY/uv-cache" HF_HOME="$INSTALL_DIR/models"
step() { printf '\n\033[35m==> %s\033[0m\n' "$1"; }
mkdir -p "$TOOLS" "$PY"
if [ ! -x "$UV" ]; then
  step "Downloading uv (Python manager)"
  curl -fsSL "https://github.com/astral-sh/uv/releases/latest/download/uv-x86_64-unknown-linux-gnu.tar.gz" \
    | tar -xz -C "$TOOLS" --strip-components=1
fi
[ -x "$VENV/bin/python" ] || { step "Creating Python 3.12 environment"; "$UV" venv --python 3.12 "$VENV"; }
step "Installing faster-whisper"
"$UV" pip install --python "$VENV/bin/python" -r "$PY/requirements.txt"
if command -v nvidia-smi >/dev/null 2>&1 && nvidia-smi -L >/dev/null 2>&1; then
  step "NVIDIA GPU detected: installing CUDA libraries"
  "$UV" pip install --python "$VENV/bin/python" nvidia-cublas-cu12 "nvidia-cudnn-cu12==9.*" || echo "CUDA libs failed; CPU will be used"
else
  step "No NVIDIA GPU detected: Whisper will use the CPU"
fi
step "Downloading Whisper model '$MODEL'"
"$VENV/bin/python" -c "from faster_whisper.utils import download_model; download_model('$MODEL')"
step "Done"
