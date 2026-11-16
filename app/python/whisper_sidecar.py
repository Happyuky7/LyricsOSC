"""LyricsOSC voice engine (started by the Rust app).

Protocol:
  stdin  : binary blocks  [u32 LE sample count][f32 LE * n]   (16 kHz mono)
           a block with count 0 = new song (detect the language again)
  stdout : one JSON line per event
           {"ready": true, "device": "cuda:0"}
           {"text": "...", "lang": "es"}
           {"error": "..."}

Usage: python whisper_sidecar.py --model small --device auto --languages zh,en,es
"""
import argparse
import json
import struct
import sys
import warnings

import numpy as np

warnings.filterwarnings("ignore")


def emit(**kw):
    sys.stdout.write(json.dumps(kw, ensure_ascii=False) + "\n")
    sys.stdout.flush()


# Phrases Whisper tends to hallucinate on silence / instrumentals
HALLUCINATIONS = (
    "thanks for watching", "thank you for watching", "subscribe", "like and subscribe",
    "gracias por ver", "suscríbete", "subtítulos", "amara.org", "字幕", "请不吝点赞",
    "订阅", "感谢观看", "明镜", "點贊", "訂閱",
)


def is_hallucination(text):
    t = text.lower().strip(" .!?。！")
    if len(t) < 2:
        return True
    if any(h in t for h in HALLUCINATIONS):
        return True
    words = t.split()
    return len(words) >= 6 and len(set(words)) <= 2      # "la la la la la la"


def read_exact(stream, n):
    buf = bytearray()
    while len(buf) < n:
        chunk = stream.read(n - len(buf))
        if not chunk:
            return None
        buf += chunk
    return bytes(buf)


def add_cuda_dlls():
    """Makes the pip-installed nvidia-cublas/cudnn DLLs visible to CTranslate2."""
    import glob
    import os
    import site
    for sp in site.getsitepackages():
        for d in glob.glob(os.path.join(sp, "nvidia", "*", "bin")):
            try:
                os.add_dll_directory(d)
                os.environ["PATH"] = d + os.pathsep + os.environ.get("PATH", "")
            except OSError:
                pass


def load_model(name, device, gpu=0, compute="auto"):
    add_cuda_dlls()
    from faster_whisper import WhisperModel

    def ct(d):
        return compute if compute != "auto" else ("float16" if d == "cuda" else "int8")

    tries = ["cuda", "cpu"] if device == "auto" else [device]
    last = None
    for dev in tries:
        try:
            m = WhisperModel(name, device=dev, device_index=gpu if dev == "cuda" else 0,
                             compute_type=ct(dev))
            # warm-up: catches missing CUDA libs now instead of on the first chunk
            list(m.transcribe(np.zeros(16000, dtype=np.float32), beam_size=1)[0])
            return m, dev
        except Exception as e:  # no CUDA, missing cuBLAS/cuDNN, etc.
            last = e
    raise last


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", default="small")
    ap.add_argument("--device", default="auto")
    ap.add_argument("--gpu", type=int, default=0)
    ap.add_argument("--compute-type", default="auto")
    ap.add_argument("--beam", type=int, default=1)
    ap.add_argument("--vad", action="store_true")
    ap.add_argument("--languages", default="zh,en,es")
    args = ap.parse_args()
    langs = [l.strip() for l in args.languages.split(",") if l.strip()]

    try:
        model, dev = load_model(args.model, args.device, args.gpu, args.compute_type)
    except Exception as e:
        emit(error=f"could not load Whisper: {e}")
        return 1
    emit(ready=True, device=f"{dev}:{args.gpu}" if dev == "cuda" else dev)

    stdin = sys.stdin.buffer
    locked = None          # language locked once detected with confidence
    while True:
        head = read_exact(stdin, 4)
        if head is None:
            return 0
        (n,) = struct.unpack("<I", head)
        if n == 0:         # reset (new song): detect the language again
            locked = None
            continue
        data = read_exact(stdin, n * 4)
        if data is None:
            return 0
        audio = np.frombuffer(data, dtype=np.float32)

        def run():
            segs, inf = model.transcribe(
                audio, language=locked, beam_size=max(1, args.beam),
                # No VAD by default: singing over music doesn't look like "speech" to the VAD and would be lost.
                # Silence/instrumentals are filtered with no_speech_prob and is_hallucination.
                vad_filter=args.vad,
                vad_parameters={"threshold": 0.2} if args.vad else None,
                condition_on_previous_text=False,
                initial_prompt="以下是普通话的歌词。" if locked == "zh" else None,
            )
            return list(segs), inf

        try:
            segments, info = run()
        except Exception as e:
            msg = str(e).lower()
            if dev == "cuda" and any(k in msg for k in ("cublas", "cudnn", "cuda")):
                # GPU found but CUDA libraries are missing: fall back to CPU
                model, dev = load_model(args.model, "cpu", 0, "int8")
                emit(ready=True, device="cpu (CUDA libraries missing)")
                try:
                    segments, info = run()
                except Exception as e2:
                    emit(error=str(e2))
                    continue
            else:
                emit(error=str(e))
                continue
        if locked is None:
            if info.language not in langs:
                continue   # unsupported language / instrumental
            if info.language_probability > 0.6:
                locked = info.language
        text = " ".join(s.text.strip() for s in segments if s.no_speech_prob < 0.6).strip()
        if text and not is_hallucination(text):
            emit(text=text, lang=info.language)


if __name__ == "__main__":
    sys.exit(main() or 0)
