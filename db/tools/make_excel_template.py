#!/usr/bin/env python3
"""
Шаблон выгрузки в Excel «для себя» — из файла индексатора.

Зачем. Роман 30.09.2026: «Структура колонок, порядок полей и формат вывода
данных на всех листах без исключения должны быть реализованы в точности
(идентично) как в существующем файле-примере (индексаторе Excel)». И 01.10,
после первой выгрузки: «полностью слетает визуальное оформление … ширина
колонок, высота строк, перенос текста по словам, цвета заливки ячеек должно
быть полностью идентично». Листы «1», «2», «3» переименованы в «Рождения»,
«Браки», «Смерти»; лист «МК» — как есть.

Что переносится из индексатора: стили книги и тема целиком (styles.xml,
theme1.xml), а по каждому листу — шапка со своими стилями и высотами строк,
ширины и группировка колонок, закрепление, цвет ярлыка и одна строка-образец
данных без значений: по ней программа (src-tauri/src/xlsx.rs) берёт высоту
строки и стиль ячейки каждой колонки. Стиль колонки ставится таким же, как у
ячейки образца, — пустые ячейки строки выглядят как заполненные (рамки,
перенос), и их не нужно писать в файл. Ни одной строки данных не переносится.

Как пользоваться (у Mike, не у Романа; результат — в репозиторий):

    python3 db/tools/make_excel_template.py "путь/к/Индексатор_МК_….xlsm" db/export/excel_template.xlsx

Сам .xlsm в репозиторий не попадает никогда.
"""

import re
import sys
import zipfile
from pathlib import Path
from xml.sax.saxutils import escape, unescape

# Лист индексатора → (имя в выгрузке, сколько строк шапки). У «МК» шапка —
# строка названий и три строки пояснений (разделы 1, 2, 3).
SHEETS = [
    ("МК", "МК", 4),
    ("1", "Рождения", 2),
    ("2", "Браки", 2),
    ("3", "Смерти", 2),
]

# Чего у индексатора нет, а у программы есть.
# Восприемники 3 и 4: у индексатора колонки AM…AT помечены «-» и скрыты
# (ширина 0,6), в программе их до четырёх — подписать и показать, как AE…AL.
# Поручители 5 и 6 (Роман 27.09.2026 просил до шести; лист «2» хранит четыре)
# — за BC, оформление как у поручителя 1 (AE…AH).
RELABEL = {
    "Рождения": {2: dict(zip("AM AN AO AP AQ AR AS AT".split(),
                             ["Восприемник 3"] * 4 + ["Восприемник 4"] * 4))},
}
LIKE = {  # колонка → с какой колонки взять ширину и стили
    "Рождения": dict(zip("AM AN AO AP AQ AR AS AT".split(), "AE AF AG AH AE AF AG AH".split())),
    "Браки": dict(zip("BD BE BF BG BH BI BJ BK".split(), "AE AF AG AH AE AF AG AH".split())),
}
EXTRA_TEXT = {
    "Браки": {1: dict(zip("BD BE BF BG BH BI BJ BK".split(), ["ИОФ", "НП", "Звание", "Прим."] * 2)),
              2: dict(zip("BD BE BF BG BH BI BJ BK".split(), ["Поручитель 5"] * 4 + ["Поручитель 6"] * 4))},
}


def col_index(letters: str) -> int:
    n = 0
    for ch in letters:
        n = n * 26 + ord(ch) - 64
    return n


def col_letters(n: int) -> str:
    out = ""
    while n:
        n, r = divmod(n - 1, 26)
        out = chr(65 + r) + out
    return out


def attr(tag: str, name: str):
    m = re.search(r'(?:^|\s)' + re.escape(name) + r'="([^"]*)"', tag)
    return m.group(1) if m else None


def shared_strings(z):
    out = []
    if "xl/sharedStrings.xml" in z.namelist():
        xml = z.read("xl/sharedStrings.xml").decode("utf-8")
        for si in re.findall(r"<si>(.*?)</si>|<si/>", xml, re.S):
            out.append(unescape("".join(re.findall(r"<t[^>]*>(.*?)</t>", si, re.S))))
    return out


def parse_row(row_xml: str, shared):
    """Строка листа → (атрибуты тега, {колонка: (стиль, текст или None)})."""
    tag = row_xml[:row_xml.index(">") + 1]
    cells = {}
    for c in re.findall(r"<c [^>]*?(?:/>|>.*?</c>)", row_xml, re.S):
        ctag = c[:c.index(">") + 1]
        letters = re.match(r"[A-Z]+", attr(ctag, "r")).group(0)
        text = None
        v = re.search(r"<v>(.*?)</v>", c, re.S)
        if attr(ctag, "t") == "s" and v:
            text = shared[int(v.group(1))]
        elif attr(ctag, "t") == "inlineStr":
            text = unescape("".join(re.findall(r"<t[^>]*>(.*?)</t>", c, re.S)))
        elif v:
            text = unescape(v.group(1))
        cells[letters] = (attr(ctag, "s"), text)
    return tag, cells


def build_sheet(xml: str, name: str, head: int, shared) -> tuple:
    start = xml.index("<sheetData")
    end = xml.index("</sheetData>")
    before, data = xml[:start], xml[start:end]
    rows = {}
    for r in re.finditer(r"<row [^>]*?(?:/>|>.*?</row>)", data, re.S):
        n = int(attr(r.group(0)[:r.group(0).index(">") + 1], "r"))
        if n > head + 1:
            break
        rows[n] = parse_row(r.group(0), shared)
    sample_tag, sample = rows[head + 1]
    like = LIKE.get(name, {})
    last = max(max(col_index(c) for c in sample), max((col_index(c) for c in like), default=0))

    # Ширины и группировка колонок — по одной записи на колонку; стиль
    # колонки — как у ячейки образца.
    widths = {}
    cols = re.search(r"<cols>(.*?)</cols>", before, re.S)
    for c in re.findall(r"<col [^>]*/>", cols.group(1) if cols else ""):
        for i in range(int(attr(c, "min")), min(int(attr(c, "max")), last) + 1):
            widths[i] = {k: attr(c, k) for k in ("width", "customWidth", "outlineLevel", "hidden") if attr(c, k)}
    style_of = {col_index(c): s for c, (s, _) in sample.items() if s}
    for c, src in like.items():
        widths[col_index(c)] = dict(widths.get(col_index(src), {}))
        style_of[col_index(c)] = style_of.get(col_index(src))
    cols_xml = "<cols>"
    for i in range(1, last + 1):
        w = widths.get(i, {})
        cols_xml += f'<col min="{i}" max="{i}"'
        cols_xml += f' width="{w.get("width", "8.875")}"'
        if style_of.get(i):
            cols_xml += f' style="{style_of[i]}"'
        for k in ("customWidth", "outlineLevel", "hidden"):
            if w.get(k):
                cols_xml += f' {k}="{w[k]}"'
        cols_xml += "/>"
    cols_xml += "</cols>"

    def cell(letters, n, style, text):
        s = f' s="{style}"' if style else ""
        if text is None or text == "":
            return f'<c r="{letters}{n}"{s}/>'
        if re.fullmatch(r"[1-9][0-9]{0,8}", text):
            return f'<c r="{letters}{n}"{s}><v>{text}</v></c>'
        return (f'<c r="{letters}{n}"{s} t="inlineStr"><is><t xml:space="preserve">'
                f'{escape(text)}</t></is></c>')

    body = ""
    for n in range(1, head + 1):
        tag, cells = rows[n]
        tag = re.sub(r'\sspans="[^"]*"', "", tag)
        for c, src in like.items():  # новые колонки — стиль шапки соседа
            cells[c] = (cells.get(src, (None, None))[0], cells.get(c, (None, None))[1])
        for c, text in RELABEL.get(name, {}).get(n, {}).items():
            cells[c] = (cells.get(c, (None, None))[0], text)
        for c, text in EXTRA_TEXT.get(name, {}).get(n, {}).items():
            cells[c] = (cells.get(c, (None, None))[0], text)
        body += tag + "".join(cell(c, n, *cells[c]) for c in sorted(cells, key=col_index)) + "</row>"
    # Строка-образец: высота и стили, без значений. Стиль строки убран, чтобы
    # у пустых ячеек работал стиль колонки.
    ht = attr(sample_tag, "ht")
    body += f'<row r="{head + 1}"' + (f' ht="{ht}"' if ht else "") + ">"
    for i in range(1, last + 1):
        if style_of.get(i):
            body += f'<c r="{col_letters(i)}{head + 1}" s="{style_of[i]}"/>'
    body += "</row>"

    last_letters = col_letters(last)
    root = re.match(r"<\?xml.*?\?>\s*<worksheet[^>]*>", before, re.S).group(0)
    tab = re.search(r"<sheetPr[^>]*?(?:/>|>.*?</sheetPr>)", before, re.S)
    pane = re.search(r"<pane [^>]*/>", before)
    fmt = re.search(r"<sheetFormatPr [^>]*/>", before)
    out = root
    if tab:
        out += re.sub(r'\scodeName="[^"]*"', "", tab.group(0))
    out += f'<dimension ref="A1:{last_letters}{head + 1}"/>'
    # Закрепление — как у индексатора, но прокрутка — в начало данных: у него
    # в файле лист «МК» сохранён прокрученным к строке 6971, и выгрузка из 20
    # записей открылась бы пустым экраном (проверяющий #40).
    view = re.search(r"<sheetView [^>]*>", before)
    zoom = "".join(f' {k}="{attr(view.group(0), k)}"' for k in ("zoomScale", "zoomScaleNormal")
                   if view and attr(view.group(0), k))
    pane_xml = ""
    if pane:
        x, y = int(attr(pane.group(0), "xSplit") or 0), int(attr(pane.group(0), "ySplit") or 0)
        top = f"{col_letters(x + 1)}{y + 1}"
        pane_xml = (f'<pane xSplit="{x}" ySplit="{y}" topLeftCell="{top}" activePane="bottomRight" state="frozen"/>'
                    f'<selection pane="topRight"/><selection pane="bottomLeft"/>'
                    f'<selection pane="bottomRight" activeCell="{top}" sqref="{top}"/>')
    out += f'<sheetViews><sheetView{zoom} workbookViewId="0">{pane_xml}</sheetView></sheetViews>' 
    out += fmt.group(0) if fmt else '<sheetFormatPr defaultRowHeight="12.75"/>'
    out += cols_xml + "<sheetData>" + body + "</sheetData>"
    out += f'<autoFilter ref="A{head}:{last_letters}{head + 1}"/>'
    out += '<pageMargins left="0.59" right="0.59" top="0.59" bottom="0.59" header="0.2" footer="0.2"/>'
    out += "</worksheet>"
    return out, last_letters


def build(xlsm: Path, target: Path) -> None:
    z = zipfile.ZipFile(xlsm)
    shared = shared_strings(z)
    wb = z.read("xl/workbook.xml").decode("utf-8")
    rels = z.read("xl/_rels/workbook.xml.rels").decode("utf-8")
    targets = {attr(r, "Id"): attr(r, "Target") for r in re.findall(r"<Relationship [^>]*/>", rels)}
    paths = {}
    for s in re.findall(r"<sheet [^>]*/>", wb):
        t = targets[attr(s, "r:id")].lstrip("/")
        paths[unescape(attr(s, "name"))] = t if t.startswith("xl/") else "xl/" + t

    sheets = []
    for src, name, head in SHEETS:
        xml, last = build_sheet(z.read(paths[src]).decode("utf-8"), name, head, shared)
        sheets.append((name, head, last, xml))
        print(f"  {name}: шапка {head} стр., колонки A…{last}")

    ct = ['<?xml version="1.0" encoding="UTF-8" standalone="yes"?>',
          '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">',
          '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>',
          '<Default Extension="xml" ContentType="application/xml"/>',
          '<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>',
          '<Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>',
          '<Override PartName="/xl/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/>']
    wbx = ['<?xml version="1.0" encoding="UTF-8" standalone="yes"?>',
           '<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" '
           'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>']
    wbrels = ['<?xml version="1.0" encoding="UTF-8" standalone="yes"?>',
              '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">']
    defined = []
    for i, (name, head, last, _) in enumerate(sheets):
        ct.append(f'<Override PartName="/xl/worksheets/sheet{i + 1}.xml" '
                  'ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>')
        wbx.append(f'<sheet name="{escape(name)}" sheetId="{i + 1}" r:id="rId{i + 1}"/>')
        wbrels.append(f'<Relationship Id="rId{i + 1}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{i + 1}.xml"/>')
        defined.append(f'<definedName name="_xlnm._FilterDatabase" localSheetId="{i}" hidden="1">'
                       f"'{escape(name)}'!$A${head}:${last}${head + 1}</definedName>")
    ct.append("</Types>")
    n = len(sheets)
    wbx.append("</sheets><definedNames>" + "".join(defined) + '</definedNames><calcPr calcId="191029"/></workbook>')
    wbrels.append(f'<Relationship Id="rId{n + 1}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>')
    wbrels.append(f'<Relationship Id="rId{n + 2}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="theme/theme1.xml"/>')
    wbrels.append("</Relationships>")
    root_rels = ('<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
                 '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
                 '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>'
                 '</Relationships>')

    target.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(target, "w", zipfile.ZIP_DEFLATED) as out:
        # Постоянная дата в архиве: файл не меняется от пересборки к пересборке.
        def put(name, data):
            info = zipfile.ZipInfo(name, date_time=(2026, 10, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            out.writestr(info, data if isinstance(data, bytes) else data.encode("utf-8"))
        put("[Content_Types].xml", "".join(ct))
        put("_rels/.rels", root_rels)
        put("xl/workbook.xml", "".join(wbx))
        put("xl/_rels/workbook.xml.rels", "".join(wbrels))
        put("xl/styles.xml", z.read("xl/styles.xml"))
        put("xl/theme/theme1.xml", z.read("xl/theme/theme1.xml"))
        for i, (_, _, _, xml) in enumerate(sheets):
            put(f"xl/worksheets/sheet{i + 1}.xml", xml)


if __name__ == "__main__":
    if len(sys.argv) != 3:
        print(__doc__)
        raise SystemExit(2)
    build(Path(sys.argv[1]), Path(sys.argv[2]))
