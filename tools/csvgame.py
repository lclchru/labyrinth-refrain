ESC = "&,&"
ESC_QUOTE = '&"&'


def parse(txt: str):
    rows, row, field = [], [], []
    quoted = in_quotes = False
    i, n = 0, len(txt)
    while i < n:
        c = txt[i]
        if in_quotes:
            if c == '"':
                if i + 1 < n and txt[i + 1] == '"':
                    field.append('"')
                    i += 1
                else:
                    in_quotes = False
            else:
                field.append(c)
        elif txt.startswith(ESC, i) or txt.startswith(ESC_QUOTE, i):


            field.append(txt[i : i + 3])
            i += 2
        elif c == '"' and not field:
            in_quotes = quoted = True
        elif c == ",":
            row.append(("".join(field), quoted))
            field, quoted = [], False
        elif c in "\r\n":
            eol = "\r\n" if c == "\r" and i + 1 < n and txt[i + 1] == "\n" else c
            row.append(("".join(field), quoted))
            rows.append((row, eol))
            row, field, quoted = [], [], False
            i += len(eol) - 1
        else:
            field.append(c)
        i += 1
    if field or row:
        row.append(("".join(field), quoted))
        rows.append((row, ""))
    return rows


def render(rows) -> str:
    out = []
    for row, eol in rows:
        cells = []
        for value, quoted in row:


            bare = value.replace(ESC, "").replace(ESC_QUOTE, "")
            need = any(ch in bare for ch in (",", "\r", "\n")) or value.startswith('"')
            if quoted or need:
                cells.append('"' + value.replace('"', '""') + '"')
            else:
                cells.append(value)
        out.append(",".join(cells) + eol)
    return "".join(out)
