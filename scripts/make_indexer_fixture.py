#!/usr/bin/env python3
"""
Синтетический Excel-индексатор для проверки импорта (спека 2026-10-02, п. 4).

Листы «1», «2», «3» и «МК» — в раскладке индексатора Романа: строка 1 —
шапка, строка 2 — подписи, с третьей — записи; числа — числами, даты —
текстом «05.01.1889», последней строкой — «Проиндексировал: …». Данные
выдуманы целиком — люди, причт, места, фонд и опись: настоящих записей
заказчика в репозитории нет и быть не должно.

Файл собирается одной стандартной библиотекой (xlsx — это zip с XML) и
лежит в репозитории: db/fixtures/indexer.xlsx. Его читает тест крейта
(src-tauri/core/src/import.rs, imports_fixture) и сквозная проверка на
Windows. Поменял раскладку — пересобери и поправь ожидания теста:

    python3 scripts/make_indexer_fixture.py
"""

import sys
import zipfile
from pathlib import Path
from xml.sax.saxutils import escape

OUT = Path(__file__).resolve().parent.parent / "db" / "fixtures" / "indexer.xlsx"

HEAD1 = {1: "№ пп", 2: "Архив", 3: "Источник - Ф, О, Д", 4: "МК", 5: "Год", 6: "Часть", 7: "Стр.",
         8: "Счет", 9: "Счет", 10: "Дата", 11: "Дата",
         13: "НП", 14: "Звание", 15: "ИОФ", 16: "Вероисповедания", 17: "Прим.", 18: "Лет",
         19: "НП", 20: "Звание", 21: "ИОФ", 22: "Вероисповедния", 23: "Прим.", 24: "Лет",
         **{c + k: v for c in (31, 35, 39, 43) for k, v in enumerate(("ИОФ", "НП", "Звание", "Прим."))},
         **{c + k: v for c in (47, 50, 53) for k, v in enumerate(("ИОФ", "Звание", "Прим."))}}
HEAD2 = {
    "1": {1: 1, 8: "родившихся М", 9: "родившихся Ж", 10: "рождения", 11: "крещения", 12: "Имя родившегося"},
    "2": {1: 2, 8: "бракосочетавшихся", 10: "бракосочетания", 17: "каким браком", 23: "каким браком"},
    "3": {1: 3, 8: "умерших М", 9: "умерших Ж", 10: "смерти", 11: "погребения", 17: "от чего умер", 23: "отец, супруг"},
}

ARCHIVE = "ГА Тестовой области"
MK = "Церковь: Никольская Село: Никольское Уезд: Тестовский Губерния: Тестовская"
FOD = {1889: "Ф.1 Оп.2 Д.11", 1890: "Ф.1 Оп.2 Д.12"}
PRIEST = {47: "Иоанн Преображенский", 48: "священник"}


def base(n, year, part, page):
    return {1: n, 2: ARCHIVE, 3: FOD[year], 4: MK, 5: year, 6: part, 7: page}


def person(col, place, rank, iof, confession=None, note=None, extra=None):
    out = {col: place, col + 1: rank, col + 2: iof, col + 3: confession, col + 4: note, col + 5: extra}
    return {k: v for k, v in out.items() if v is not None}


BIRTHS = [
    {**base(1, 1889, 1, 873), 9: 1, 10: "05.01.1889", 11: "06.01.1889", 12: "Татьяна",
     **person(13, "Тестово Малое", "крестьянин", "Никита Алексеев", "православного"),
     **person(19, "Тестово Малое", "законная жена его", "Евлампия Васильева (Сидорова)", "православного"),
     31: "Александр Арсеньев", 32: "Новое Тестово", 33: "крестьянский сын", 34: "того же дому",
     **PRIEST, 50: "Пётр Троицкий", 51: "пономарь"},
    {**base(2, 1890, 1, 874), 8: 1, 10: "?.06.1890", 11: "?.06.1890", 12: "Григорий",
     **person(13, "Верхнее Тестово", "крестьянин", "Иван Семенов", "православного"),
     **person(19, "Верхнее Тестово", "законная жена его", "Елизавета Георгиева", "православного"), **PRIEST},
    {**base(3, 1890, 1, 874), 8: 2, 10: "10.07.1890", 11: "11.07.1890", 12: "Наум",
     **person(19, "Верхнее Тестово", "солдатская жена", "Анна Григорьева", "православного", "незаконнорожденный сын"),
     **PRIEST},
    {**base(4, 1890, 1, 875), 9: 1, 10: "12.07.1890", 11: "13.07.1890", 12: "Мария",
     **person(13, "Верхнее Тестово", "крестьянин", "Жданко Иванов", "православного"), **PRIEST},
    # Заготовка без единого имени — импорт её пропускает и говорит об этом.
    {**base(5, 1890, 1, 875)},
]

MARRIAGES = [
    {**base(1, 1889, 2, 894), 8: 1, 10: "31.01.1889",
     **person(13, "Нижнее Тестово", "крестьянский сын", "Михаил Дмитриев", "православного", "Первым браком", 22),
     **person(19, "Тестовка", "крестьянская дочь-девица", "Евдокия Савельева", "православного", "Первым браком", 23),
     25: "отец", 28: "отец", 29: "Савелий Петров", 30: "крестьянин",
     31: "Иван Константинов", 32: "Нижнее Тестово", 33: "крестьянин", 34: "по жениху",
     35: "Терентий Николаев", 36: "Тестовка", 37: "крестьянин", 38: "по невесте", **PRIEST},
    {**base(2, 1890, 2, 895), 8: 1, 10: "07.02.1890",
     **person(13, "Верхнее Тестово", "крестьянин", "Павел Яковлев", "православного", "Вторым браком", 30),
     **person(19, "Нижнее Тестово", "крестьянская вдова", "Мария Макарова", "православного", "Вторым браком"),
     25: "отец", 28: "мать",
     31: "Николай Васильев", 32: "Верхнее Тестово", 33: "крестьянин", 34: "по жениху", **PRIEST},
]

DEATHS = [
    {**base(1, 1889, 3, 900), 9: 1, 10: "01.01.1889", 11: "03.01.1889",
     **person(13, "Нижнее Тестово", "крестьянская вдова", "Акилина Сергеева", None, "старость", 64), **PRIEST},
    {**base(2, 1889, 3, 900), 9: 2, 10: "09.01.1889", 11: "11.01.1889",
     **person(13, "Верхнее Тестово", "дочь младенец", "Васса", None, "удушье", "2 мес"),
     **person(19, "Верхнее Тестово", "крестьянская жена", "Дарья Иванова"), **PRIEST},
    {**base(3, 1890, 3, 901), 8: 1, 10: "01.03.1890", 11: "03.03.1890",
     14: "тело неизвестного человека", 17: "утонул", **PRIEST},
    {**base(4, 1890, 3, 901), 8: 2, 10: "07.03.1890", 11: "09.03.1890",
     **person(13, "Верхнее Тестово", "сын младенец", "Иван", None, "понос", "3 нед"),
     **person(19, "Верхнее Тестово", "крестьянин", "Евдоким Иванов", None, "проживающий в селе"), **PRIEST},
]

FOOTER = {4: "Проиндексировал: Тест Тестов 2026/8/24"}

# Лист «МК»: импорт берёт из него только родство родственника умершего —
# колонка D («Роль»), N — № пп записи, S — часть.
MK_SHEET = [
    {1: "№", 2: "Год", 3: "Код", 4: "Роль", 14: "Раздел / № пп", 19: "Часть"},
    {14: "1 >"}, {14: "2 >"}, {14: "3 >"},
    {1: 1, 2: 1889, 3: 12, 4: "умерший", 14: 2, 19: 3},
    {1: 2, 2: 1889, 3: 13, 4: "умершего мать", 14: 2, 19: 3},
    {4: "Проиндексировал: Тест Тестов 2026/8/24"},
]


def letters(n: int) -> str:
    s = ""
    while n:
        n, r = divmod(n - 1, 26)
        s = chr(65 + r) + s
    return s


class Strings:
    def __init__(self):
        self.index, self.items = {}, []

    def add(self, text: str) -> int:
        if text not in self.index:
            self.index[text] = len(self.items)
            self.items.append(text)
        return self.index[text]


def sheet_xml(rows, strings: Strings) -> str:
    out = ['<?xml version="1.0" encoding="UTF-8" standalone="yes"?>',
           '<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>']
    for r, row in enumerate(rows, 1):
        cells = []
        for c in sorted(row):
            ref, v = f"{letters(c)}{r}", row[c]
            if isinstance(v, (int, float)):
                cells.append(f'<c r="{ref}"><v>{v}</v></c>')
            else:
                cells.append(f'<c r="{ref}" t="s"><v>{strings.add(str(v))}</v></c>')
        out.append(f'<row r="{r}">{"".join(cells)}</row>')
    out.append("</sheetData></worksheet>")
    return "".join(out)


# Лист «НП» — справочник пунктов индексатора: колонка 10 — название для ввода
# (оно стоит в записях), 1 — чистое, 2–5 — тип, губерния, уезд, волость,
# 6–7 — краткое и полное место, 8 — ссылка Familio. Импорт берёт отсюда
# подробности пунктов (с 08.10.2026 в поставке программы пунктов нет).
# «Дальнее Тестово» в записях не встречается — переносится из справочника.
NP_SHEET = [
    {1: "Справочник населённых пунктов"},
    {1: "Название", 2: "Тип", 3: "Губерния", 4: "Уезд", 5: "Волость", 6: "Кратко", 7: "Полностью",
     8: "Familio", 10: "НП для ввода МК"},
    {1: "Тестово Малое", 2: "д.", 3: "Тестовская", 4: "Тестовский", 5: "Тестовская", 6: "д. Тестово Малое",
     7: "д. Тестово Малое, Тестовская волость, Тестовский уезд, Тестовская губерния",
     8: "https://familio.org/settlements/00000000-0000-0000-0000-000000000001", 10: "Тестово Малое"},
    {1: "Верхнее Тестово", 2: "с.", 3: "Тестовская", 4: "Тестовский", 6: "с. Верхнее Тестово",
     7: "с. Верхнее Тестово, Тестовский уезд, Тестовская губерния", 10: "Верхнее Тестово"},
    {1: "Дальнее Тестово", 2: "д.", 3: "Тестовская", 4: "Соседний", 5: "Дальняя", 6: "д. Дальнее Тестово",
     7: "д. Дальнее Тестово, Дальняя волость, Соседний уезд, Тестовская губерния",
     10: "д.Дальнее Тестово, Дальняя волость"},
]


def main() -> int:
    strings = Strings()
    sheets = [("1", [HEAD1, HEAD2["1"], *BIRTHS, FOOTER]),
              ("2", [HEAD1, HEAD2["2"], *MARRIAGES]),
              ("3", [HEAD1, HEAD2["3"], *DEATHS, FOOTER]),
              ("МК", MK_SHEET), ("НП", NP_SHEET)]
    parts = {f"xl/worksheets/sheet{i}.xml": sheet_xml(rows, strings) for i, (_, rows) in enumerate(sheets, 1)}
    parts["xl/sharedStrings.xml"] = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        f'<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="{len(strings.items)}" '
        f'uniqueCount="{len(strings.items)}">'
        + "".join(f"<si><t>{escape(s)}</t></si>" for s in strings.items) + "</sst>")
    parts["xl/workbook.xml"] = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        '<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" '
        'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>'
        + "".join(f'<sheet name="{escape(name)}" sheetId="{i}" r:id="rId{i}"/>' for i, (name, _) in enumerate(sheets, 1))
        + "</sheets></workbook>")
    parts["xl/_rels/workbook.xml.rels"] = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        + "".join(f'<Relationship Id="rId{i}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" '
                  f'Target="worksheets/sheet{i}.xml"/>' for i in range(1, len(sheets) + 1))
        + f'<Relationship Id="rId{len(sheets) + 1}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" '
          'Target="sharedStrings.xml"/></Relationships>')
    parts["_rels/.rels"] = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" '
        'Target="xl/workbook.xml"/></Relationships>')
    parts["[Content_Types].xml"] = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
        '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
        '<Default Extension="xml" ContentType="application/xml"/>'
        '<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>'
        + "".join(f'<Override PartName="/xl/worksheets/sheet{i}.xml" '
                  'ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>'
                  for i in range(1, len(sheets) + 1))
        + '<Override PartName="/xl/sharedStrings.xml" '
          'ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/></Types>')

    OUT.parent.mkdir(parents=True, exist_ok=True)
    # Фиксированная дата в zip: файл не меняется от запуска к запуску.
    with zipfile.ZipFile(OUT, "w", zipfile.ZIP_DEFLATED) as z:
        for name in sorted(parts):
            info = zipfile.ZipInfo(name, date_time=(2026, 10, 2, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, parts[name].encode("utf-8"))
    print(f"Собран {OUT} — рождений {len(BIRTHS)}, браков {len(MARRIAGES)}, смертей {len(DEATHS)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
