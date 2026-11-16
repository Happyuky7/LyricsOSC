# Third-party notices

LyricsOSC's source code is licensed under the GNU GPL v3.0 or later. Its mascot and
artwork have a separate licence: [app/assets/LICENSE-ART.md](app/assets/LICENSE-ART.md). It uses the following
third-party software and services. Their licences are compatible with the GPL.

## Bundled / linked (Rust crates)

| Component | Licence |
|---|---|
| egui / eframe | MIT OR Apache-2.0 |
| windows-rs | MIT OR Apache-2.0 |
| wasapi | MIT |
| zbus | MIT |
| ureq | MIT OR Apache-2.0 |
| serde, serde_json | MIT OR Apache-2.0 |
| regex | MIT OR Apache-2.0 |
| sysinfo | MIT |
| png | MIT OR Apache-2.0 |
| sys-locale | MIT OR Apache-2.0 |

Run `cargo about generate` or `cargo license` for the full, exact list of
transitive dependencies of a given release.

## Installed by the setup script (voice engine)

| Component | Licence |
|---|---|
| Python | PSF License |
| uv (Astral) | MIT OR Apache-2.0 |
| faster-whisper | MIT |
| CTranslate2 | MIT |
| OpenAI Whisper models (weights) | MIT |
| NumPy | BSD-3-Clause |
| NVIDIA cuBLAS / cuDNN (optional, NVIDIA GPUs only) | NVIDIA proprietary EULA — downloaded on the user's machine from PyPI, **not redistributed** with LyricsOSC |

## Axis (optional, Windows)

- **AxisOpenApi.dll** — Axis's public API library, used only when "Send to Axis (addon mode)" is enabled. It is loaded at runtime and is **not part of LyricsOSC's source code**; it belongs to the Axis developers and is distributed under their terms. If you redistribute LyricsOSC, check that you are allowed to include this DLL, or ship without it and let users copy it from their Axis installation.

## Online services

- **LRCLIB** (https://lrclib.net) — free lyrics database. Lyrics belong to their respective rights holders; LyricsOSC only displays them to the user and caches them locally.

## Fonts

LyricsOSC does not ship fonts. It uses fonts already installed on the system
(Microsoft YaHei, Malgun Gothic, Leelawadee UI, Segoe UI Emoji on Windows;
Noto fonts on Linux).
