"""Builds all app/installer/store images from assets/source/.
  assets/source/mascot-chibi.png -> assets/logo.png (512), assets/icon.png (256), assets/icon.ico
  assets/source/keyart-idol.png  -> docs/keyart.png (1920x1080), docs/steam-capsule.png (460x215),
                                    docs/steam-header.png (920x430)
Requires Pillow (pip install pillow)."""
import os
from PIL import Image, ImageOps

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
A, D = os.path.join(ROOT, "assets"), os.path.join(ROOT, "..", "docs")

m = Image.open(os.path.join(A, "source", "mascot-chibi.png")).convert("RGBA")
corner = m.getpixel((2, 2))
print("mascot corner pixel (alpha should be 0):", corner)
m.resize((512, 512), Image.LANCZOS).save(os.path.join(A, "logo.png"), optimize=True)
m.resize((256, 256), Image.LANCZOS).save(os.path.join(A, "icon.png"), optimize=True)
m.save(os.path.join(A, "icon.ico"), sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])

k = Image.open(os.path.join(A, "source", "keyart-idol.png")).convert("RGB")
ImageOps.fit(k, (1920, 1080), Image.LANCZOS).save(os.path.join(D, "keyart.png"), optimize=True)
# Steam capsules: keep the character (right side) in frame
ImageOps.fit(k, (920, 430), Image.LANCZOS, centering=(0.6, 0.4)).save(os.path.join(D, "steam-header.png"), optimize=True)
ImageOps.fit(k, (460, 215), Image.LANCZOS, centering=(0.6, 0.4)).save(os.path.join(D, "steam-capsule.png"), optimize=True)
print("assets built")
