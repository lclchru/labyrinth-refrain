import os
import re
import struct

import numpy as np

FMT_RE = re.compile(rb"DXT[135]")
HEADER_MIN = 0x80


class UnsupportedFormat(Exception):
    pass


def info(raw, name=None):
    who = name or "<нет имени>"
    i = raw.rfind(b"PTexture2D\x00")
    if i < 8:
        raise UnsupportedFormat(
            f"{who}: в контейнере нет метки PTexture2D (файл {len(raw)} байт) - "
            f"это, похоже, не текстура вовсе")
    w, h = struct.unpack_from("<II", raw, i - 8)
    length = struct.unpack_from("<I", raw, 0x50)[0]
    off = len(raw) - length
    if not (HEADER_MIN <= off <= len(raw)):
        off = None
    m = FMT_RE.search(raw[: i + 4096])
    if m is None:
        px = None if off is None else len(raw) - off
        kind = ""
        if px is not None and px == w * h * 4:
            kind = (f"; пиксели занимают {px} = w*h*4 байт, то есть это непожатая "
                    f"32-битная текстура (BGRA8888)")
        elif px is not None and px == w * h:
            kind = f"; пиксели занимают {px} = w*h байт, похоже на 8-битный формат"
        raise UnsupportedFormat(
            f"{who}: формат не DXT1/DXT3/DXT5 (в контейнере нет строки формата). "
            f"Файл {len(raw)} байт, размеры {w}x{h}, данных "
            f"{'неизвестно' if px is None else px} байт{kind}")
    return m.group().decode(), w, h, off


def split(raw, name=None):
    fmt, w, h, off = info(raw, name)
    return raw[:off], fmt, w, h, raw[off:]


def join(header, w, h, pixels):
    hdr = bytearray(header)
    i = hdr.rfind(b"PTexture2D\x00")
    struct.pack_into("<I", hdr, 0x50, len(pixels))
    struct.pack_into("<I", hdr, i - 16, max(w, h).bit_length() - 1)
    struct.pack_into("<II", hdr, i - 8, w, h)
    return bytes(hdr) + pixels


def _rgb565(c):
    r = ((c >> 11) & 31) * 255 // 31
    g = ((c >> 5) & 63) * 255 // 63
    b = (c & 31) * 255 // 31
    return np.stack([r, g, b], -1).astype(np.uint16)


def _colour(block, bx, by, punchthrough):
    c0 = block[:, :, 0].astype(np.uint16) | (block[:, :, 1].astype(np.uint16) << 8)
    c1 = block[:, :, 2].astype(np.uint16) | (block[:, :, 3].astype(np.uint16) << 8)
    bits = np.zeros((by, bx), np.uint32)
    for k in range(4):
        bits |= block[:, :, 4 + k].astype(np.uint32) << (8 * k)

    a, b = _rgb565(c0), _rgb565(c1)
    lut = np.zeros((by, bx, 4, 3), np.uint16)
    lut[:, :, 0], lut[:, :, 1] = a, b
    wide = (c0 > c1) | (not punchthrough)
    lut[:, :, 2] = np.where(wide[..., None], (2 * a + b) // 3, (a + b) // 2)
    lut[:, :, 3] = np.where(wide[..., None], (a + 2 * b) // 3, 0)

    idx = np.stack([(bits >> (2 * i)) & 3 for i in range(16)], -1)
    rgb = np.take_along_axis(lut, idx[:, :, :, None], 2)
    alpha = np.where(punchthrough & ~wide[..., None] & (idx == 3), 0, 255)
    return rgb.astype(np.uint8), alpha.astype(np.uint8)


def decode(pixels, fmt, w, h):
    bx, by = w // 4, h // 4
    stride = 8 if fmt == "DXT1" else 16
    a = np.frombuffer(pixels, np.uint8).reshape(by, bx, stride)

    if fmt == "DXT1":
        rgb, alpha = _colour(a, bx, by, True)
    else:
        rgb, _ = _colour(a[:, :, 8:], bx, by, False)
        if fmt == "DXT3":
            al = a[:, :, :8]
            alpha = np.zeros((by, bx, 16), np.uint8)
            alpha[:, :, 0::2] = al & 0xF
            alpha[:, :, 1::2] = al >> 4
            alpha = alpha * 17
        else:
            alpha = _bc4(a[:, :, :8], bx, by)

    out = np.concatenate([rgb, alpha[:, :, :, None]], -1)
    out = out.reshape(by, bx, 4, 4, 4).transpose(0, 2, 1, 3, 4).reshape(h, w, 4)
    return out[::-1]


def _bc4(block, bx, by):
    a0 = block[:, :, 0].astype(np.int32)
    a1 = block[:, :, 1].astype(np.int32)
    bits = np.zeros((by, bx), np.uint64)
    for k in range(6):
        bits |= block[:, :, 2 + k].astype(np.uint64) << np.uint64(8 * k)

    lut = np.zeros((by, bx, 8), np.int32)
    lut[:, :, 0], lut[:, :, 1] = a0, a1
    wide = a0 > a1
    for i in range(1, 7):
        lut[:, :, 1 + i] = np.where(wide, ((6 - i) * a0 + i * a1) // 7, 0)
    for i in range(1, 5):
        lut[:, :, 1 + i] = np.where(wide, lut[:, :, 1 + i], ((5 - i) * a0 + i * a1) // 5)
    lut[:, :, 6] = np.where(wide, lut[:, :, 6], 0)
    lut[:, :, 7] = np.where(wide, lut[:, :, 7], 255)

    idx = np.stack([((bits >> np.uint64(3 * i)) & np.uint64(7)).astype(np.int64) for i in range(16)], -1)
    return np.take_along_axis(lut, idx, 2).astype(np.uint8)


def _fit(rgb, mask):
    pts = rgb.astype(np.float32)
    mean = pts.mean(0)
    d = pts - mean
    _, _, v = np.linalg.svd(d, full_matrices=False)
    t = d @ v[0]
    return pts[t.argmin()], pts[t.argmax()]


def _to565(c):
    r, g, b = [int(round(x)) for x in c]
    return (min(31, r * 31 // 255) << 11) | (min(63, g * 63 // 255) << 5) | min(31, b * 31 // 255)


def encode(img, fmt):
    img = np.ascontiguousarray(img[::-1])
    h, w = img.shape[:2]
    bx, by = w // 4, h // 4
    blocks = img.reshape(by, 4, bx, 4, 4).transpose(0, 2, 1, 3, 4).reshape(by, bx, 16, 4)
    out = bytearray()
    for r in range(by):
        for c in range(bx):
            out += _block(blocks[r, c], fmt)
    return bytes(out)


def _block(blk, fmt):
    out = b""
    if fmt == "DXT3":
        al = np.clip((blk[:, 3].astype(np.uint16) + 8) // 17, 0, 15).astype(np.uint8)
        out = bytes([al[i] | (al[i + 1] << 4) for i in range(0, 16, 2)])
    elif fmt == "DXT5":
        out = _enc_bc4(blk[:, 3])
    return out + _enc_colour(blk[:, :3], fmt == "DXT1")


def _refine(rgb, a, b, rounds=8):
    pts = rgb.astype(np.float32)
    best = (None, None, np.inf)
    for _ in range(rounds):
        pal = np.stack([a, b, (2 * a + b) / 3, (a + 2 * b) / 3])
        d = ((pts[:, None, :] - pal[None]) ** 2).sum(-1)
        idx = d.argmin(1)
        err = float(d.min(1).sum())
        if err < best[2]:
            best = (a, b, err)

        wa = np.array([1.0, 0.0, 2 / 3, 1 / 3])[idx]
        wb = 1.0 - wa
        s_aa = float((wa * wa).sum())
        s_bb = float((wb * wb).sum())
        s_ab = float((wa * wb).sum())
        det = s_aa * s_bb - s_ab * s_ab
        if abs(det) < 1e-6:
            break
        t_a = (wa[:, None] * pts).sum(0)
        t_b = (wb[:, None] * pts).sum(0)
        na = np.clip((s_bb * t_a - s_ab * t_b) / det, 0, 255)
        nb = np.clip((s_aa * t_b - s_ab * t_a) / det, 0, 255)
        if np.allclose(na, a, atol=0.5) and np.allclose(nb, b, atol=0.5):
            break
        a, b = na, nb
    return best[0], best[1]


def _enc_colour(rgb, punchthrough):
    lo, hi = _fit(rgb, None)
    hi, lo = _refine(rgb, hi.astype(np.float32), lo.astype(np.float32))
    c0, c1 = _to565(hi), _to565(lo)


    if c0 < c1:
        c0, c1 = c1, c0
    if c0 == c1:


        if c1 > 0:
            c1 -= 1
        else:
            c0 += 1
    a, b = _rgb565(np.uint16(c0)).astype(np.float32), _rgb565(np.uint16(c1)).astype(np.float32)
    pal = np.stack([a, b, (2 * a + b) / 3, (a + 2 * b) / 3])
    d = ((rgb.astype(np.float32)[:, None, :] - pal[None]) ** 2).sum(-1)
    idx = d.argmin(1)
    bits = 0
    for i in range(16):
        bits |= int(idx[i]) << (2 * i)
    return struct.pack("<HHI", c0, c1, bits)


def _enc_bc4(a):
    lo, hi = int(a.min()), int(a.max())
    if hi == lo:
        hi = min(255, lo + 1)
    pal = np.array([hi, lo] + [((6 - i) * hi + i * lo) // 7 for i in range(1, 7)], np.int32)
    idx = np.abs(a.astype(np.int32)[:, None] - pal[None]).argmin(1)
    bits = 0
    for i in range(16):
        bits |= int(idx[i]) << (3 * i)
    return bytes([hi, lo]) + bits.to_bytes(6, "little")

def encode_patch(orig, img_old, img_new, fmt, grow=0):
    stride = 8 if fmt == "DXT1" else 16
    new = bytearray(orig)
    h, w = img_old.shape[:2]


    diff = (img_old != img_new).any(-1)[::-1]
    blk = diff.reshape(h // 4, 4, w // 4, 4).any(axis=(1, 3))
    if grow:


        from scipy import ndimage as _nd
        blk = _nd.binary_dilation(blk, iterations=int(grow))
    blk = blk.reshape(-1)


    flipped = np.ascontiguousarray(img_new[::-1])
    bx = w // 4
    for i in np.nonzero(blk)[0]:
        r, c = divmod(int(i), bx)
        cell = flipped[r * 4 : r * 4 + 4, c * 4 : c * 4 + 4].reshape(16, 4)
        new[i * stride : (i + 1) * stride] = _block(cell, fmt)
    return bytes(new)
