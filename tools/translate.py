import argparse
import json
from pathlib import Path
import csvgame
import lse
from patches import safe_rel


def apply(source, output):
    grouped = {}
    for key, value in json.loads(source.read_text(encoding='utf-8')).items():
        name, row, col = key.rsplit(':', 2)
        rel = safe_rel(name)
        if rel.parts[0] != 'csv_en' or rel.suffix != '.lse' or not isinstance(value, str):
            raise ValueError(f'Invalid translation field: {key}')
        grouped.setdefault(rel, {})[(int(row), int(col))] = value
    verified = []
    root = output.resolve()
    for rel, edits in sorted(grouped.items()):
        file = (root / rel).resolve()
        if root not in file.parents:
            raise ValueError(f'Resource outside output: {rel}')
        raw = file.read_bytes()
        plain = lse.decrypt(raw)
        try:
            text = plain.decode('utf-8')
            encoding = 'utf-8'
        except UnicodeDecodeError:
            text = plain.decode('cp932')
            encoding = 'cp932'
        cut = text.rfind('\n') + 1
        body, tail = text[:cut], text[cut:]
        rows = csvgame.parse(body)
        if csvgame.render(rows) != body:
            raise ValueError(f'CSV round-trip failed: {rel}')
        for (row, col), value in edits.items():
            if row < 0 or col < 0 or row >= len(rows) or col >= len(rows[row][0]):
                raise ValueError(f'Invalid cell: {rel}:{row}:{col}')
            rows[row][0][col] = (value, rows[row][0][col][1])
        data = lse.encrypt((csvgame.render(rows) + tail).encode(encoding), raw[-1])
        verified.append((file, raw, data))
    changed = 0
    for file, raw, data in verified:
        if data != raw:
            file.write_bytes(data)
            changed += 1
    print(f'Translation: {len(grouped)} resources verified, {changed} updated')


if __name__ == '__main__':
    root = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser()
    parser.add_argument('--source', type=Path, default=root / 'translations/ru.json')
    parser.add_argument('--output', type=Path, default=root / 'build/refrain-ru')
    args = parser.parse_args()
    apply(args.source, args.output)
