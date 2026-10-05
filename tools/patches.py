import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import zlib
import numpy as np

def digest(data):
    return hashlib.sha256(data).hexdigest()

def safe_rel(value):
    rel = PurePosixPath(value)
    if rel.is_absolute() or ".." in rel.parts or "\\" in value or ":" in value:
        raise ValueError(f"Unsafe relative path: {value}")
    return Path(*rel.parts)

def delta(original, translated):
    a = np.zeros(len(translated), dtype=np.uint8)
    n = min(len(original), len(translated))
    a[:n] = np.frombuffer(original[:n], np.uint8)
    return np.bitwise_xor(a, np.frombuffer(translated, np.uint8)).tobytes()

def export(args):
    source = args.original.resolve()
    translated = args.translated.resolve()
    dest = args.patch.resolve()
    if (dest / "manifest.json").exists():
        raise ValueError("Patch manifest already exists; choose a fresh export directory")
    entries = []
    for file in sorted(translated.rglob("*")):
        if not file.is_file():
            continue
        rel = file.relative_to(translated)
        native = source / rel
        b = file.read_bytes()
        added = not native.is_file()
        if added:
            allowed = {"hud.txt", "labels.txt", "loading-hide.txt", "loading-now", "own-text.off", "rects.txt", "subs.txt"}
            if rel.as_posix() not in allowed or len(b) > 2_000_000:
                raise ValueError(f"No approved native baseline/metadata type: {rel}")
            b.decode("utf-8")
        a = b"" if added else native.read_bytes()
        if a == b and not added:
            continue
        payload = zlib.compress(delta(a, b), 9)
        key = rel.as_posix()
        blob = key + ".xor.zlib"
        target = dest / safe_rel(blob)
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(payload)
        entries.append({"file":key, "original_sha256":digest(a), "translated_sha256":digest(b),
                        "original_size":len(a), "translated_size":len(b), "added_metadata":added, "delta":blob,
                        "delta_sha256":digest(payload)})
    dest.mkdir(parents=True, exist_ok=True)
    (dest / "manifest.json").write_text(json.dumps({"format":"refrain-xor-v1", "entries":entries},
                                                ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"Exported {len(entries)} changed files")

def apply(args):
    source, dest = args.original.resolve(), args.output.resolve()
    if dest == source or source in dest.parents:
        raise ValueError("Output must be outside the original game directory")
    manifest = json.loads((args.patch / "manifest.json").read_text("utf-8"))
    if manifest.get("format") != "refrain-xor-v1":
        raise ValueError("Unknown patch format")
    verified = []
    for item in manifest["entries"]:
        rel = safe_rel(item["file"])
        target = (dest / rel).resolve()
        if dest not in target.parents:
            raise ValueError(f"Output path outside destination: {target}")
        if target.exists() and (not args.replace or not target.is_file()):
            raise ValueError(f"Output already exists: {target}")
        a = b"" if item.get("added_metadata") else (source / rel).read_bytes()
        if len(a) != item["original_size"] or digest(a) != item["original_sha256"]:
            raise ValueError(f"Game version/hash mismatch: {rel}")
        payload = (args.patch / safe_rel(item["delta"])).read_bytes()
        if digest(payload) != item["delta_sha256"]:
            raise ValueError(f"Corrupt delta: {rel}")
        patch = zlib.decompress(payload)
        if len(patch) != item["translated_size"]:
            raise ValueError(f"Corrupt decoded size: {rel}")
        b = delta(a, patch)
        if digest(b) != item["translated_sha256"]:
            raise ValueError(f"Result hash mismatch: {rel}")
        verified.append((target, b))
    for target, data in verified:
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
    print(f"Applied {len(verified)} verified resources")

if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)
    for command in ["export", "apply"]:
        p = sub.add_parser(command)
        p.add_argument("--original", type=Path, required=True)
        p.add_argument("--patch", type=Path, required=True)
        p.add_argument("--translated" if command == "export" else "--output", type=Path, required=True)
        if command == "apply":
            p.add_argument("--replace", action="store_true")
    args = parser.parse_args()
    (export if args.command == "export" else apply)(args)
