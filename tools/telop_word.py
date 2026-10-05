import os
import struct
import sys

import numpy as np
from PIL import Image, ImageDraw, ImageFont

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import tex

MEDIA = os.environ["REFRAIN_MEDIA"]
ASSETS = os.path.join(HERE, "..", "platforms", "windows", "runtime", "assets")
FONT = os.path.join(HERE, "..", "assets", "fonts", "PTSerif-Bold.ttf")

TILE = 112
STEP = 128
TILES = [(x, 656) for x in (0, 128, 256, 384, 512)] + [(x, 784) for x in (0, 128, 256, 384, 512, 640)]

GOLD = [(247, 236, 214), (236, 214, 170), (214, 186, 132), (188, 156, 104), (150, 120, 76)]
RED = [(232, 120, 110), (214, 88, 78), (188, 60, 52), (156, 40, 34), (120, 26, 22)]


def atlas():
    p = os.path.join(MEDIA, "texture_en", "model", "ui", "Battle_telop.dds.phyre")
    hdr, fmt, w, h, px = tex.split(open(p, "rb").read(), p)
    return tex.decode(px, fmt, w, h)


def clean_tile(a):
    stack = np.stack([a[y:y + TILE, x:x + TILE].astype(np.int16) for x, y in TILES])
    out = stack.min(axis=0)
    out[..., 3] = stack[..., 3].max(axis=0)
    return out.astype(np.uint8)


def gradient(colors, h):
    t = np.linspace(0, 1, h)
    xs = np.linspace(0, 1, len(colors))
    c = np.array(colors, float)
    return np.stack([np.interp(t, xs, c[:, i]) for i in range(3)], -1)


def letter(tile, ch, colors, scale):
    size = TILE * scale
    base = Image.fromarray(tile, "RGBA").resize((size, size), Image.LANCZOS)
    for cap in range(int(size * 0.72), 8, -2):
        font = ImageFont.truetype(FONT, cap)
        box = font.getbbox(ch)
        if box[2] - box[0] <= size * 0.78 and box[3] - box[1] <= size * 0.72:
            break
    mask = Image.new("L", (size, size), 0)
    d = ImageDraw.Draw(mask)
    box = font.getbbox(ch)
    d.text(((size - (box[2] + box[0])) / 2, (size - (box[3] + box[1])) / 2), ch, font=font, fill=255)
    m = np.asarray(mask).astype(float) / 255.0
    ys = np.nonzero(m.max(axis=1) > 0.2)[0]
    grad = np.zeros((size, 3))
    if len(ys):
        g = gradient(colors, ys[-1] - ys[0] + 1)
        grad[ys[0]:ys[-1] + 1] = g
        grad[:ys[0]] = g[0]
        grad[ys[-1] + 1:] = g[-1]
    out = np.asarray(base).astype(float).copy()
    col = np.repeat(grad[:, None, :], size, axis=1)

    sh = np.asarray(mask.filter(__import__("PIL.ImageFilter", fromlist=["ImageFilter"]).GaussianBlur(size * 0.03)))
    sa = (sh.astype(float) / 255.0 * 0.8)[..., None]
    out[..., :3] *= 1 - sa[..., 0][..., None] * 0.85
    out[..., :3] = out[..., :3] * (1 - m[..., None]) + col * m[..., None]
    out[..., 3] = np.maximum(out[..., 3], m * 255)
    return out.clip(0, 255).astype(np.uint8)


def word(a, text, colors, scale=2):
    tile = clean_tile(a)
    n = len(text)
    w = (n - 1) * STEP * scale + TILE * scale
    h = TILE * scale
    canvas = np.zeros((h, w, 4), np.uint8)
    for i, ch in enumerate(text):
        x = i * STEP * scale
        canvas[:, x:x + TILE * scale] = letter(tile, ch, colors, scale)
    return canvas


def save(name, img):
    os.makedirs(ASSETS, exist_ok=True)
    h, w = img.shape[:2]
    a = img[..., 3:4].astype(np.uint16)
    bgra = np.zeros_like(img)
    bgra[..., 0] = (img[..., 2].astype(np.uint16) * a[..., 0] // 255).astype(np.uint8)
    bgra[..., 1] = (img[..., 1].astype(np.uint16) * a[..., 0] // 255).astype(np.uint8)
    bgra[..., 2] = (img[..., 0].astype(np.uint16) * a[..., 0] // 255).astype(np.uint8)
    bgra[..., 3] = img[..., 3]
    p = os.path.join(ASSETS, f"telop_{name}.bin")
    with open(p, "wb") as f:
        f.write(struct.pack("<II", w, h))
        f.write(bgra.tobytes())
    Image.fromarray(img, "RGBA").save(os.path.join(ASSETS, f"telop_{name}.png"))
    print(f"{name}: {w}x{h} -> {p}")


if __name__ == "__main__":
    a = atlas()
    save("victory", word(a, "ПОБЕДА", GOLD))
    save("lose", word(a, "ПОРАЖЕНИЕ", RED))
