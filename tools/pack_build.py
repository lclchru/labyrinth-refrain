import io
import os
import struct
import sys

import zstandard

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.join(HERE, "..")
SRC = os.path.join(ROOT, "build", "refrain-ru")
CRATE = os.path.join(ROOT, "platforms", "windows", "runtime")


TARGET = os.environ.get("CARGO_TARGET_DIR") or os.path.join(CRATE, "target")
DLL = os.path.join(TARGET, "i686-pc-windows-msvc", "release", "dinput8.dll")
OUT = os.path.join(ROOT, "dist", "dinput8.dll")
DIRS = ("csv_en", "font", "model", "model_en", "texture", "texture_en")
MAGIC = b"RFRUPAK1"


def files():
    for d in DIRS:
        base = os.path.join(SRC, d)
        for root, _, names in os.walk(base):
            for n in sorted(names):
                p = os.path.join(root, n)
                key = os.path.relpath(p, SRC).replace("\\", "/").lower()
                yield key, p


def main():
    dll = open(DLL, "rb").read()
    if dll[-16:-8] == MAGIC:
        raise SystemExit("в исходной DLL уже есть пак")
    cctx = zstandard.ZstdCompressor(level=19)
    out = io.BytesIO()
    out.write(dll)
    index = []
    raw_total = comp_total = 0
    for key, p in sorted(files()):
        raw = open(p, "rb").read()
        comp = cctx.compress(raw)
        method = 1
        if len(comp) >= len(raw):
            comp, method = raw, 0
        index.append((key, method, out.tell(), len(comp), len(raw)))
        out.write(comp)
        raw_total += len(raw)
        comp_total += len(comp)
    idx = io.BytesIO()
    idx.write(struct.pack("<I", len(index)))
    for key, method, off, clen, rlen in index:
        k = key.encode("utf-8")
        idx.write(struct.pack("<H", len(k)) + k + struct.pack("<BIII", method, off, clen, rlen))
    index_off = out.tell()
    out.write(idx.getvalue())
    out.write(MAGIC + struct.pack("<II", index_off, len(idx.getvalue())))
    data = out.getvalue()
    if len(data) >= 1 << 32:
        raise SystemExit("пак больше 4 ГБ")
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    open(OUT, "wb").write(data)
    print(f"записей {len(index)}, исходно {raw_total / 2**20:.1f} МБ, сжато {comp_total / 2**20:.1f} МБ")
    print(f"-> {os.path.abspath(OUT)}: {len(data) / 2**20:.1f} МБ")

    if "--check" in sys.argv:
        dctx = zstandard.ZstdDecompressor()
        bad = 0
        for key, method, off, clen, rlen in index:
            blob = data[off:off + clen]
            got = blob if method == 0 else dctx.decompress(blob, max_output_size=rlen)
            src = open(os.path.join(SRC, key.replace("/", os.sep)), "rb").read() if os.path.exists(os.path.join(SRC, key)) else None
            if got != src and got != open(dict(files())[key], "rb").read():
                bad += 1
                print("не сходится:", key)
        print("проверка:", "все записи сходятся" if not bad else f"расхождений {bad}")
        if bad:
            raise SystemExit(1)


if __name__ == "__main__":
    main()
