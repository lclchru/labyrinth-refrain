from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:
    import tomli as tomllib

ROOT = Path(__file__).resolve().parent.parent


def main():
    with (ROOT / "translations/subtitles.toml").open("rb") as source:
        entries = tomllib.load(source)["subtitles"]
    lines = []
    voices = set()
    for entry in entries:
        voice = entry["voice"]
        duration = entry["duration"]
        text = entry["text"]
        if not isinstance(voice, str) or not voice or voice in voices:
            raise ValueError("Invalid or duplicate voice")
        if isinstance(duration, bool) or not isinstance(duration, (int, float)) or not 0 < duration < float("inf"):
            raise ValueError("Invalid duration")
        if not isinstance(text, str) or not text:
            raise ValueError("Invalid subtitle text")
        fields = [voice, str(duration), text]
        if "position" in entry:
            fields.append(entry["position"])
        if any(not isinstance(value, str) or any(c in value for c in "\t\r\n") for value in fields):
            raise ValueError("Invalid subtitle field")
        voices.add(voice)
        lines.append("\t".join(fields))
    data = ("\n".join(lines) + "\n").encode("utf-8")
    for path in (ROOT / "build/refrain-ru/subs.txt", ROOT / "platforms/windows/runtime/assets/subs.txt"):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
    print(f"Subtitles: {len(lines)}")


if __name__ == "__main__":
    main()
