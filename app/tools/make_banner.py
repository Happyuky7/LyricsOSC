"""Builds the banner (key art + app name) used at the top of the README and the app.
  docs/banner.png   original size of the key art (README)
  assets/banner.png 1280x340  (app header)
Requires Pillow. Fonts: Segoe UI Black / Semibold (Windows) or DejaVu (Linux)."""
import os
from PIL import Image, ImageDraw, ImageFilter, ImageFont, ImageOps

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
SRC = os.path.join(ROOT, "assets", "source", "keyart-idol.png")
PINK, VIOLET = (255, 92, 138), (176, 76, 224)


def font(names, size):
    for n in names:
        for d in ("C:/Windows/Fonts", "/usr/share/fonts/truetype/dejavu"):
            p = os.path.join(d, n)
            if os.path.exists(p):
                return ImageFont.truetype(p, size)
    return ImageFont.load_default()


def banner(w, h, out, subtitle=True):
    src = Image.open(SRC).convert("RGB")
    if w is None:  # original size, no crop (whole character visible)
        w, h = src.size
    art = ImageOps.fit(src, (w, h), Image.LANCZOS, centering=(0.62, 0.35))
    img = art.convert("RGBA")
    # dark gradient on the left so the text is readable
    grad = Image.new("L", (w, 1))
    for x in range(w):
        grad.putpixel((x, 0), int(235 * max(0.0, 1 - x / (w * 0.62)) ** 1.4))
    shade = Image.new("RGBA", (w, h), (18, 8, 30, 255))
    shade.putalpha(grad.resize((w, h)))
    img = Image.alpha_composite(img, shade)

    d = ImageDraw.Draw(img)
    wide = w / h > 3  # app strip vs. 16:9 key art
    title_size = int(h * 0.30) if wide else int(w * 0.075)
    small_size = int(h * 0.075) if wide else int(w * 0.021)
    big = font(["seguibl.ttf", "DejaVuSans-Bold.ttf"], title_size)
    small = font(["seguisb.ttf", "DejaVuSans.ttf"], small_size)
    x, y = int(w * 0.045), int(h * (0.20 if wide else 0.30))

    # title with pink->violet gradient + glow
    text = "LyricsOSC"
    bbox = d.textbbox((x, y), text, font=big)
    tw, th = bbox[2] - bbox[0], bbox[3] - bbox[1]
    mask = Image.new("L", (w, h))
    ImageDraw.Draw(mask).text((x, y), text, font=big, fill=255)
    # dark outline so the title stands out on the violet background
    outline = Image.new("RGBA", (w, h))
    ImageDraw.Draw(outline).text((x, y), text, font=big, fill=(25, 6, 35, 230),
                                 stroke_width=max(2, h // 90), stroke_fill=(25, 6, 35, 230))
    img = Image.alpha_composite(img, outline.filter(ImageFilter.GaussianBlur(1)))
    glow = Image.new("RGBA", (w, h), PINK + (0,))
    glow.putalpha(mask.filter(ImageFilter.GaussianBlur(h * 0.03)).point(lambda a: int(a * 0.9)))
    img = Image.alpha_composite(img, glow)
    fill = Image.new("RGBA", (w, h))
    for i in range(bbox[0], bbox[2] + 1):
        t = (i - bbox[0]) / max(1, tw)
        col = tuple(int(PINK[c] + (VIOLET[c] - PINK[c]) * t) for c in range(3)) + (255,)
        ImageDraw.Draw(fill).line([(i, 0), (i, h)], fill=col)
    white_top = Image.new("RGBA", (w, h), (255, 255, 255, 255))
    lettering = Image.composite(Image.blend(fill, white_top, 0.25), Image.new("RGBA", (w, h)), mask)
    img = Image.alpha_composite(img, lettering)

    if subtitle:
        sy = y + th + int(small_size * 1.6)
        lines = ["Synced lyrics & live transcription", "for the VRChat chatbox"]
        dd = ImageDraw.Draw(img)
        tw2 = max(dd.textlength(l, font=small) for l in lines)
        pad = int(small_size * 0.55)
        box = (x + 4 - pad, sy - pad // 2, x + 4 + tw2 + pad, sy + int(small_size * 2.5) + pad)
        panel = Image.new("RGBA", (w, h))
        ImageDraw.Draw(panel).rounded_rectangle(box, radius=pad, fill=(60, 14, 70, 185),
                                                outline=PINK + (170,), width=max(1, small_size // 14))
        img = Image.alpha_composite(img, panel)
        d = ImageDraw.Draw(img)
        d.text((x + 4, sy), lines[0], font=small, fill=(255, 255, 255, 255))
        d.text((x + 4, sy + int(small_size * 1.27)), lines[1], font=small, fill=(255, 205, 228, 255))
    img.convert("RGB").save(out, optimize=True)
    print("wrote", out)


# README banner: original key-art size, whole character visible
banner(None, None, os.path.join(ROOT, "..", "docs", "banner.png"))
# App banner: title only — the tagline is drawn by the app in the UI language
banner(1280, 340, os.path.join(ROOT, "assets", "banner.png"), subtitle=False)
