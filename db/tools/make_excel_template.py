#!/usr/bin/env python3
"""
Шаблон выгрузки в Excel «для себя» (сборка #39) — из файла индексатора.

Зачем. Роман 30.09.2026: «Структура колонок, порядок полей и формат вывода
данных на всех листах без исключения должны быть реализованы в точности
(идентично) как в существующем файле-примере (индексаторе Excel)». Листы
«1», «2», «3» переименованы: «Рождения», «Браки», «Смерти»; лист «МК» — как
есть. Этот инструмент переносит из индексатора только шапку листов (строки
заголовка), ширины колонок и закрепление шапки — без единой строки данных.
Программа кладёт свои строки под шапку (src-tauri/src/xlsx.rs).

Как пользоваться (у Mike, не у Романа; результат — в репозиторий):

    python3 db/tools/make_excel_template.py "путь/к/Индексатор_МК_….xlsm" db/export/excel_template.xlsx

Сам .xlsm в репозиторий не попадает никогда.
"""

import re
import sys
import zipfile
import xml.etree.ElementTree as ET
from pathlib import Path
from xml.sax.saxutils import escape

NS = {"m": "http://schemas.openxmlformats.org/spreadsheetml/2006/main"}
REL = "{http://schemas.openxmlformats.org/officeDocument/2006/relationships}id"

# Лист индексатора → (имя в выгрузке, сколько строк шапки). У «МК» шапка —
# строка названий и три строки пояснений (разделы 1, 2, 3).
SHEETS = [
    ("МК", "МК", 4),
    ("1", "Рождения", 2),
    ("2", "Браки", 2),
    ("3", "Смерти", 2),
]


# Колонки, которых у индексатора нет: поручители 5 и 6 (Роман 27.09.2026
# просил до шести, лист «2» индексатора хранит четыре). Встают за BC.
EXTRA = {
    "Браки": {
        1: {c: t for c, t in zip(["BD", "BE", "BF", "BG", "BH", "BI", "BJ", "BK"],
                                 ["ИОФ", "НП", "Звание", "Прим."] * 2)},
        2: {c: t for c, t in zip(["BD", "BE", "BF", "BG", "BH", "BI", "BJ", "BK"],
                                 ["Поручитель 5"] * 4 + ["Поручитель 6"] * 4)},
    },
    # Восприемники 3 и 4: у индексатора колонки AM…AT помечены «-» и скрыты
    # (ширина 0,6), в программе их до четырёх — подписать и показать.
    "Рождения": {
        2: {c: t for c, t in zip(["AM", "AN", "AO", "AP", "AQ", "AR", "AS", "AT"],
                                 ["Восприемник 3"] * 4 + ["Восприемник 4"] * 4)},
    },
}
# Ширины открытых колонок — как у восприемников 1–2 (AE…AL): ИОФ шире.
SHOW = {"Рождения": [("AM", 12.6), ("AN", 8.9), ("AO", 8.9), ("AP", 13.1),
                     ("AQ", 12.6), ("AR", 8.9), ("AS", 8.9), ("AT", 13.1)]}


def read_sheets(xlsm: Path):
    z = zipfile.ZipFile(xlsm)
    shared = []
    if "xl/sharedStrings.xml" in z.namelist():
        for si in ET.fromstring(z.read("xl/sharedStrings.xml")).findall("m:si", NS):
            shared.append("".join(t.text or "" for t in si.iter(f"{{{NS['m']}}}t")))
    wb = ET.fromstring(z.read("xl/workbook.xml"))
    rels = {r.get("Id"): r.get("Target") for r in ET.fromstring(z.read("xl/_rels/workbook.xml.rels"))}
    paths = {}
    for s in wb.find("m:sheets", NS):
        t = rels[s.get(REL)].lstrip("/")
        paths[s.get("name")] = t if t.startswith("xl/") else "xl/" + t
    out = {}
    for src, name, head in SHEETS:
        root = ET.fromstring(z.read(paths[src]))
        rows = []
        for row in root.iter(f"{{{NS['m']}}}row"):
            r = int(row.get("r"))
            if r > head:
                break
            cells = {}
            for c in row.findall("m:c", NS):
                col = re.match(r"[A-Z]+", c.get("r")).group(0)
                v = c.find("m:v", NS)
                if c.get("t") == "s" and v is not None:
                    cells[col] = shared[int(v.text)]
                elif c.get("t") == "inlineStr":
                    cells[col] = "".join(t.text or "" for t in c.iter(f"{{{NS['m']}}}t"))
                elif v is not None and v.text:
                    cells[col] = v.text
            rows.append((r, cells))
        widths = []
        cols = root.find("m:cols", NS)
        if cols is not None:
            for col in cols.findall("m:col", NS):
                if col.get("width"):
                    widths.append((int(col.get("min")), int(col.get("max")), float(col.get("width"))))
        dim = root.find("m:dimension", NS)
        last_col = re.match(r"[A-Z]+[0-9]*:([A-Z]+)", dim.get("ref")).group(1) if dim is not None else "A"
        for r, cells in rows:
            cells.update(EXTRA.get(name, {}).get(r, {}))
        for letters, w in SHOW.get(name, []):
            i = col_index(letters)
            # Разрезать диапазон ширины, покрывающий колонку, и вставить свою.
            split = []
            for lo, hi, width in widths:
                if lo <= i <= hi:
                    if lo < i:
                        split.append((lo, i - 1, width))
                    if i < hi:
                        split.append((i + 1, hi, width))
                else:
                    split.append((lo, hi, width))
            split.append((i, i, w))
            widths = sorted(split)
        if name in EXTRA:
            last_col = max([last_col] + [c for r in EXTRA[name].values() for c in r], key=col_index)
        out[name] = (head, rows, widths, last_col)
    return out


def col_index(letters: str) -> int:
    n = 0
    for ch in letters:
        n = n * 26 + ord(ch) - 64
    return n


def sheet_xml(head, rows, widths, last_col):
    parts = ['<?xml version="1.0" encoding="UTF-8" standalone="yes"?>',
             '<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">',
             f'<dimension ref="A1:{last_col}{head}"/>',
             # Шапка закреплена: при прокрутке тысяч строк она на месте.
             f'<sheetViews><sheetView workbookViewId="0"><pane ySplit="{head}" topLeftCell="A{head + 1}" '
             'activePane="bottomLeft" state="frozen"/></sheetView></sheetViews>',
             '<sheetFormatPr defaultRowHeight="15"/>']
    if widths:
        parts.append("<cols>")
        for lo, hi, w in widths:
            parts.append(f'<col min="{lo}" max="{hi}" width="{w}" customWidth="1"/>')
        parts.append("</cols>")
    parts.append("<sheetData>")
    for r, cells in rows:
        parts.append(f'<row r="{r}">')
        for col in sorted(cells, key=col_index):
            # Стиль 1 — жирный: шапка.
            parts.append(f'<c r="{col}{r}" s="1" t="inlineStr"><is><t xml:space="preserve">'
                         f'{escape(cells[col])}</t></is></c>')
        parts.append("</row>")
    parts.append("</sheetData>")
    parts.append(f'<autoFilter ref="A{head}:{last_col}{head}"/>')
    parts.append('<pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/>')
    parts.append("</worksheet>")
    return "".join(parts)


def build(xlsm: Path, target: Path) -> None:
    sheets = read_sheets(xlsm)
    names = list(sheets)
    ct = ['<?xml version="1.0" encoding="UTF-8" standalone="yes"?>',
          '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">',
          '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>',
          '<Default Extension="xml" ContentType="application/xml"/>',
          '<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>',
          '<Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>']
    for i in range(len(names)):
        ct.append(f'<Override PartName="/xl/worksheets/sheet{i + 1}.xml" '
                  'ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>')
    ct.append("</Types>")
    rels = ('<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
            '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
            '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>'
            '</Relationships>')
    wb = ['<?xml version="1.0" encoding="UTF-8" standalone="yes"?>',
          '<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" '
          'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>']
    wbrels = ['<?xml version="1.0" encoding="UTF-8" standalone="yes"?>',
              '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">']
    defined = []
    for i, name in enumerate(names):
        wb.append(f'<sheet name="{escape(name)}" sheetId="{i + 1}" r:id="rId{i + 1}"/>')
        wbrels.append(f'<Relationship Id="rId{i + 1}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{i + 1}.xml"/>')
        head, _, _, last_col = sheets[name]
        defined.append(f'<definedName name="_xlnm._FilterDatabase" localSheetId="{i}" hidden="1">'
                       f"'{escape(name)}'!$A${head}:${last_col}${head}</definedName>")
    wb.append("</sheets><definedNames>" + "".join(defined) + "</definedNames><calcPr calcId=\"191029\"/></workbook>")
    n = len(names)
    wbrels.append(f'<Relationship Id="rId{n + 1}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>')
    wbrels.append("</Relationships>")
    styles = ('<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
              '<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">'
              '<fonts count="2"><font><sz val="8"/><name val="Calibri"/></font>'
              '<font><b/><sz val="8"/><name val="Calibri"/></font></fonts>'
              '<fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills>'
              '<borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders>'
              '<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>'
              '<cellXfs count="2"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/>'
              '<xf numFmtId="0" fontId="1" fillId="0" borderId="0" xfId="0" applyFont="1">'
              '<alignment wrapText="1" vertical="top"/></xf></cellXfs>'
              '<cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles>'
              '</styleSheet>')
    target.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(target, "w", zipfile.ZIP_DEFLATED) as z:
        # Постоянная дата в архиве: файл не меняется от пересборки к пересборке.
        def put(name, text):
            info = zipfile.ZipInfo(name, date_time=(2026, 9, 30, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, text.encode("utf-8"))
        put("[Content_Types].xml", "".join(ct))
        put("_rels/.rels", rels)
        put("xl/workbook.xml", "".join(wb))
        put("xl/_rels/workbook.xml.rels", "".join(wbrels))
        put("xl/styles.xml", styles)
        for i, name in enumerate(names):
            head, rows, widths, last_col = sheets[name]
            put(f"xl/worksheets/sheet{i + 1}.xml", sheet_xml(head, rows, widths, last_col))
    for name in names:
        head, rows, _, last_col = sheets[name]
        print(f"  {name}: шапка {head} стр., колонки A…{last_col}")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        print(__doc__)
        raise SystemExit(2)
    build(Path(sys.argv[1]), Path(sys.argv[2]))
