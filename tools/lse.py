import sys


def xor(data: bytes, seed: int) -> bytes:
    return bytes(b ^ ((seed + i) & 0xFF) for i, b in enumerate(data))


def decrypt(raw: bytes) -> bytes:
    if not raw:
        return b""
    return xor(raw[:-1], raw[-1])


def encrypt(plain: bytes, seed: int = 0x84) -> bytes:
    return xor(plain, seed) + bytes([seed])


def text(raw: bytes) -> str:
    p = decrypt(raw)
    try:
        return p.decode("utf-8")
    except UnicodeDecodeError:
        return p.decode("cp932")


if __name__ == "__main__":
    sys.stdout.buffer.write(decrypt(open(sys.argv[1], "rb").read()))
