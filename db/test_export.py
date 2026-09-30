#!/usr/bin/env python3
"""
Проверка выгрузки в Familio и в Excel (сборка #39, Роман 30.09.2026).

Запросы — из db/statements.sql, те же, что выполняет приложение
(src-tauri/src/export.rs): export_prepare, familio_*, excel_*. Здесь
проверяется, что в каждой колонке листа стоит то, что должно, — по буквам
колонок образца Familio (db/export/familio_template.xlsx) и шапке листов
индексатора (db/export/excel_template.xlsx). Что пишется в сам файл xlsx,
проверяют тесты xlsx.rs (cargo test) и сквозная проверка на Windows.

Правила колонок сняты с рабочего файла Романа: листы «f - 1…3» его
индексатора строятся из листов «1…3» макросом; где макрос ошибается
(причт по номеру, поручители по номеру, «отец» в звании), выгрузка делает
по заголовкам образца — spec/2026-09-30-sborka-39.md.

Запуск:
    python3 db/test_export.py
"""

import json
import re
import sqlite3
import sys
import tempfile
import zipfile
import xml.etree.ElementTree as ET
from pathlib import Path

DB_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(DB_DIR))
from test_entry import load_statements, norm  # noqa: E402

ok_count = 0
fail_count = 0


def check(title, condition, detail=""):
    global ok_count, fail_count
    if condition:
        ok_count += 1
        print(f"  [ок]     {title}" + (f" — {detail}" if detail else ""))
    else:
        fail_count += 1
        print(f"  [ОШИБКА] {title}" + (f" — {detail}" if detail else ""))


def _utf8_stdout() -> None:
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8", errors="replace")
        except (AttributeError, ValueError):
            pass


def col(letters: str) -> int:
    """Номер колонки с нуля: A → 0, BQ → 68."""
    n = 0
    for ch in letters:
        n = n * 26 + ord(ch) - 64
    return n - 1


def header(template: Path, sheet: str, row: int) -> dict:
    """Строка шапки листа шаблона: буквы → текст."""
    ns = {"m": "http://schemas.openxmlformats.org/spreadsheetml/2006/main"}
    rel = "{http://schemas.openxmlformats.org/officeDocument/2006/relationships}id"
    z = zipfile.ZipFile(template)
    shared = []
    if "xl/sharedStrings.xml" in z.namelist():
        for si in ET.fromstring(z.read("xl/sharedStrings.xml")).findall("m:si", ns):
            shared.append("".join(t.text or "" for t in si.iter(f"{{{ns['m']}}}t")))
    wb = ET.fromstring(z.read("xl/workbook.xml"))
    rels = {r.get("Id"): r.get("Target") for r in ET.fromstring(z.read("xl/_rels/workbook.xml.rels"))}
    target = next(rels[s.get(rel)] for s in wb.find("m:sheets", ns) if s.get("name") == sheet).lstrip("/")
    target = target if target.startswith("xl/") else "xl/" + target
    out = {}
    for r in ET.fromstring(z.read(target)).iter(f"{{{ns['m']}}}row"):
        if int(r.get("r")) != row:
            continue
        for c in r.findall("m:c", ns):
            letters = re.match(r"[A-Z]+", c.get("r")).group(0)
            if c.get("t") == "s":
                out[letters] = shared[int(c.find("m:v", ns).text)]
            elif c.get("t") == "inlineStr":
                out[letters] = "".join(t.text or "" for t in c.iter(f"{{{ns['m']}}}t"))
    return out


def last_col(template: Path, sheet: str, rows=(1, 2)) -> int:
    letters = set()
    for r in rows:
        letters |= set(header(template, sheet, r))
    return max(col(x) for x in letters) + 1


class Book:
    """Набирает записи теми же запросами, что и приложение при сохранении."""

    def __init__(self, db, sql):
        self.db, self.sql = db, sql

    def place(self, name):
        if not name:
            return None
        row = self.db.execute(self.sql["place_find"], {"name_norm": norm(name)}).fetchone()
        if row:
            return row[0]
        return self.db.execute(self.sql["place_insert"], {"name": name, "name_norm": norm(name)}).lastrowid

    def entry(self, section, persons, **e):
        base = dict(case_id=1, section=section, page=None, no_male=None, no_female=None,
                    event_day=None, event_month=None, event_year=None,
                    rite_day=None, rite_month=None, rite_year=None, note=None, uncertain=None, created_by=None)
        base.update(e)
        entry_id = self.db.execute(self.sql["entry_insert"], base).lastrowid
        for i, p in enumerate(persons):
            m = dict(entry_id=entry_id, role_code=None, sort_order=i, surname=None, first_name=None,
                     patronymic=None, surname_modern=None, first_name_modern=None, patronymic_modern=None,
                     maiden_surname=None, gender=None, rank=None, confession=None, place_id=None, note=None,
                     uncertain=None, birth_year_from=None, birth_year_to=None, age_years=None,
                     marriage_order=None, kinship=None, age_months=None, age_weeks=None, age_days=None,
                     age_text=None, death_cause=None)
            place = p.pop("place", None)
            m.update(p)
            m["place_id"] = self.place(place)
            self.db.execute(self.sql["mention_insert"], m)
        return entry_id


def fill(db, sql):
    b = Book(db, sql)
    db.execute(sql["case_upsert"], dict(
        id=1, archive="ГА Костромской области", fond="56", opis="31", delo="18",
        church="Христорождественская", village="Борисоглебское", uyezd="Макарьевский",
        guberniya="Костромская", year=1886,
        parish_key="Христорождественская|Борисоглебское|Макарьевский|Костромская", indexer="Роман Чистов"))
    priest = dict(role_code="clergy1", sort_order=100, first_name="Александр", surname="Рождественский",
                  first_name_modern="Александр", rank="священник", gender="М")
    # Рождение 1886: имя ребёнка сверено («Татіяна» в книге), у восприемника
    # примечание, третий восприемник, псаломщик вторым причтом.
    b.entry(1, [
        dict(role_code="child", sort_order=10, first_name="Татьяна", first_name_modern="Татьяна", gender="Ж"),
        dict(role_code="father", sort_order=20, first_name="Никита", patronymic="Алексеев",
             first_name_modern="Никита", patronymic_modern="Алексеевич", gender="М",
             rank="крестьянин", place="Чертеж Малый", confession="православного"),
        dict(role_code="mother", sort_order=30, first_name="Евлампия", patronymic="Васильева",
             first_name_modern="Евлампия", patronymic_modern="Васильевна", gender="Ж",
             rank="законная жена его", place="Чертеж Малый"),
        dict(role_code="godparent1", sort_order=40, first_name="Александр", patronymic="Арсеньев",
             first_name_modern="Александр", patronymic_modern="Арсениевич", gender="М",
             rank="крестьянский сын", place="Чертеж Большой", note="того же дому"),
        dict(role_code="godparent3", sort_order=60, first_name="Пётр", surname="Сидоров",
             first_name_modern="Пётр", gender="М"),
        dict(priest),
        dict(role_code="clergy2", sort_order=110, first_name="Василий", patronymic="Васильев",
             surname="Промтов", first_name_modern="Василий", patronymic_modern="Васильевич",
             rank="псаломщик", gender="М"),
    ], page="873", no_female=1, event_day=5, event_month=1, event_year=1886,
       rite_day=6, rite_month=1, rite_year=1886, note="Имя в документе: Татіяна")
    # Рождение 1887 — для выбора годов.
    b.entry(1, [dict(role_code="child", sort_order=10, first_name="Иван", first_name_modern="Иван", gender="М")],
            no_male=1, event_day=2, event_month=1, event_year=1887, rite_day=3, rite_month=1, rite_year=1887)
    # Брак 1886: отец жениха, брат невесты, шесть поручителей — четверо по
    # жениху (четвёртому не хватает блока своей стороны), причт трёх званий.
    witnesses = [("Иван Константинов", "по жениху"), ("Терентий Николаев", "по жениху"),
                 ("Алексей Степанов", "по невесте"), ("Семён Павлов", "по невесте; сосед"),
                 ("Кузьма Егоров", "по жениху"), ("Фёдор Ильин", "по жениху")]
    ws = []
    for i, (iof, note) in enumerate(witnesses):
        first, patr = iof.split()
        ws.append(dict(role_code=f"witness{i + 1}", sort_order=[60, 70, 80, 90, 92, 94][i],
                       first_name=first, patronymic=patr, first_name_modern=first,
                       # «Ильин» словарь не знает — современной формы нет (None),
                       # выгрузка берёт как в книге (ревью #39: раньше тест
                       # подставлял её сам и проверял сам себя).
                       patronymic_modern=(patr[:-2] + {"ов": "ович", "ев": "евич"}[patr[-2:]]
                                          if patr[-2:] in ("ов", "ев") else None),
                       gender="М", rank="крестьянин", note=note))
    b.entry(2, [
        dict(role_code="groom", sort_order=10, first_name="Михаил", patronymic="Дмитриев",
             first_name_modern="Михаил", patronymic_modern="Дмитриевич", gender="М",
             rank="крестьянский сын", place="Поселихино", marriage_order="Первым браком", age_years=22),
        dict(role_code="bride", sort_order=20, first_name="Евдокия", patronymic="Савельева",
             first_name_modern="Евдокия", patronymic_modern="Савельевна", gender="Ж",
             rank="крестьянская девица", place="Поселихино", marriage_order="Первым браком", age_years=19),
        dict(role_code="groom_relative", sort_order=30, first_name="Дмитрий", patronymic="Иванов",
             first_name_modern="Дмитрий", patronymic_modern="Иванович", gender="М",
             rank="крестьянин", kinship="отец"),
        dict(role_code="bride_relative", sort_order=50, first_name="Иван", patronymic="Савельев",
             first_name_modern="Иван", patronymic_modern="Савельевич", gender="М", kinship="брат"),
        *ws,
        dict(priest),
        dict(role_code="clergy2", sort_order=110, first_name="Павел", surname="Ильинский",
             first_name_modern="Павел", rank="диакон", gender="М"),
        dict(role_code="clergy3", sort_order=120, first_name="Алексей", patronymic="Иванов",
             surname="Троицкий", first_name_modern="Алексей", patronymic_modern="Иванович",
             rank="пономарь", gender="М"),
    ], page="894", no_male=1, event_day=31, event_month=1, event_year=1886)
    # Смерть 1886: младенец «1,5 мес», отец, два священника, один исповедовал.
    b.entry(3, [
        dict(role_code="deceased", sort_order=10, first_name="Анна", first_name_modern="Анна", gender="Ж",
             place="Кнышево", age_text="1,5 мес", age_months=1, age_days=15, death_cause="понос"),
        dict(role_code="deceased_relative", sort_order=40, first_name="Иван", patronymic="Петров",
             first_name_modern="Иван", patronymic_modern="Петрович", gender="М",
             rank="крестьянин", place="Кнышево", kinship="отец"),
        dict(priest, note="исповедовал"),
        dict(role_code="clergy2", sort_order=110, first_name="Иоанн", surname="Смирнов",
             first_name_modern="Иоанн", rank="священник Воскресенской церкви", gender="М"),
    ], page="900", no_female=3, event_day=1, event_month=2, event_year=1886,
       rite_day=3, rite_month=2, rite_year=1886)
    # Смерть 1886: личность не установлена (Роман 30.09.2026).
    b.entry(3, [
        dict(role_code="deceased", sort_order=10, gender="М", rank="тело неизвестного человека мужеского пола",
             place="Починок Смыгарев", death_cause="утонул в Волге"),
    ], page="901", no_male=5, event_day=10, event_month=4, event_year=1886,
       rite_day=17, rite_month=4, rite_year=1886)


def rows(db, sql, block, years="[]"):
    text = sql[block]
    params = {"years": years} if ":years" in text else {}
    return [list(r) for r in db.execute(text, params).fetchall()]


def main() -> int:
    _utf8_stdout()
    import build_seed

    sql = load_statements()
    fam = DB_DIR / "export" / "familio_template.xlsx"
    exl = DB_DIR / "export" / "excel_template.xlsx"

    print("\n0. Блоки и шаблоны на месте")
    for name in ("export_prepare", "export_years", "familio_birth", "familio_marriage", "familio_death",
                 "familio_location", "excel_births", "excel_marriages", "excel_deaths", "excel_mk"):
        check(f"блок {name}", name in sql)
    check("образец Familio в репозитории", fam.is_file())
    check("шаблон Excel в репозитории", exl.is_file())

    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "db.sqlite"
        build_seed.build(path)
        db = sqlite3.connect(path)
        fill(db, sql)
        db.executescript(sql["export_prepare"])

        print("\n1. Число колонок — как в образце")
        for block, template, sheet, heads in (
                ("familio_birth", fam, "РОЖДЕНИЕ", (1, 2)), ("familio_marriage", fam, "БРАК", (1, 2)),
                ("familio_death", fam, "СМЕРТЬ", (1, 2)), ("familio_location", fam, "location", (1, 2)),
                ("excel_births", exl, "Рождения", (1, 2)), ("excel_marriages", exl, "Браки", (1, 2)),
                ("excel_deaths", exl, "Смерти", (1, 2)), ("excel_mk", exl, "МК", (1, 2, 3, 4))):
            n = len(db.execute(sql[block], {"years": "[]"} if ":years" in sql[block] else {}).description)
            want = last_col(template, sheet, heads)
            check(f"{block}: {n} колонок, в листе «{sheet}» {want}", n == want)

        print("\n2. Годы для окна выгрузки")
        years = db.execute(sql["export_years"]).fetchall()
        check("1886: 1 рождение, 1 брак, 2 смерти; 1887: 1 рождение",
              years == [(1886, 1, 1, 2), (1887, 1, 0, 0)], str(years))

        print("\n3. Familio, РОЖДЕНИЕ")
        birth = rows(db, sql, "familio_birth")
        check("весь приход — две записи", len(birth) == 2, str(len(birth)))
        r = birth[0]
        v = lambda c: r[col(c)]  # noqa: E731
        check("A № п/п — 1", v("A") == 1)
        check("B–F архив, фонд, опись, дело, лист",
              [v("B"), v("C"), v("D"), v("E"), v("F")] == ["ГА Костромской области", "56", "31", "18", "873"],
              str([v("B"), v("C"), v("D"), v("E"), v("F")]))
        check("G Н.П. события — название без типа, как в location!A", v("G") == "Борисоглебское", str(v("G")))
        check("H full_location села из справочника", "Борисоглебское" in (v("H") or "")
              and "губерния" in (v("H") or ""), str(v("H")))
        check("I церковь, J «рождение», K № из женского счёта",
              [v("I"), v("J"), v("K")] == ["Христорождественская", "рождение", 1])
        check("L пол «жен»", v("L") == "жен")
        check("M–R даты числами", [v(c) for c in "MNOPQR"] == [5, 1, 1886, 6, 1, 1886])
        check("S имя ребёнка — словарное (не «Татіяна»)", v("S") == "Татьяна", str(v("S")))
        check("T/V/Y/Z отец: НП, звание, имя, отчество современное",
              [v("T"), v("V"), v("Y"), v("Z")] == ["Чертеж Малый", "крестьянин", "Никита", "Алексеевич"],
              str([v("T"), v("V"), v("Y"), v("Z")]))
        check("U person_location отца — полное место", (v("U") or "").startswith("д. Чертеж Малый"), str(v("U")))
        check("AA/AD/AE мать", [v("AA"), v("AD"), v("AE")] == ["законная жена его", "Евлампия", "Васильевна"])
        check("AI восприемник 1 — ФИО, отчество современное", v("AI") == "Александр Арсениевич", str(v("AI")))
        check("AF/AG статус и НП восприемника", [v("AF"), v("AG")] == ["крестьянский сын", "Чертеж Большой"])
        check("AQ восприемник 3 — «Фамилия Имя»", v("AQ") == "Сидоров Пётр", str(v("AQ")))
        check("AM восприемник 2 пуст", v("AM") is None)
        check("BH/BI священник как в книге", [v("BH"), v("BI")] == ["священник", "Александр Рождественский"])
        check("BL/BM дьякона нет — пусто", v("BL") is None and v("BM") is None)
        check("BN/BO псаломщик по званию, не по номеру; фамилия вперёд",
              [v("BN"), v("BO")] == ["псаломщик", "Промтов Василий Васильев"], str([v("BN"), v("BO")]))
        check("BP пометки «ИОФ: примечание»", v("BP") == "Александр Арсеньев: того же дому", str(v("BP")))
        check("BQ авторский комментарий — все персоны как в книге, с причтом",
              v("BQ") == "Татіяна; Никита Алексеев; Евлампия Васильева; Александр Арсеньев; Пётр Сидоров; "
                         "Александр Рождественский; Василий Васильев Промтов", str(v("BQ")))
        only = rows(db, sql, "familio_birth", json.dumps([1887]))
        check("годы [1887] — одна запись, № 1", len(only) == 1 and only[0][0] == 1 and only[0][col("S")] == "Иван")

        print("\n4. Familio, БРАК")
        r = rows(db, sql, "familio_marriage")[0]
        check("J «брак», K №, L–N дата", [v("J"), v("K"), v("L"), v("M"), v("N")] == ["брак", 1, 31, 1, 1886])
        check("O/Q/T/U жених", [v("O"), v("Q"), v("T"), v("U")] == ["Поселихино", "крестьянский сын", "Михаил", "Дмитриевич"])
        check("V № брака «Первым браком» → 1, W лет 22", [v("V"), v("W")] == [1, 22], str([v("V"), v("W")]))
        check("AC звание отца жениха — его звание, не слово «отец»", v("AC") == "крестьянин", str(v("AC")))
        check("AF/AG отец жениха", [v("AF"), v("AG")] == ["Дмитрий", "Иванович"])
        check("AL/AM/AN/AO невеста", [v("AL"), v("AM"), v("AN"), v("AO")] == ["Евдокия", "Савельевна", 1, 19])
        check("AS–AY отца невесты нет (брат — не отец)", all(v(c) is None for c in ("AU", "AX", "AY")))
        check("CD–CI брат невесты — родственник невесты",
              [v("CD"), v("CE"), v("CI")] == ["невесты", "брат", "Иван Савельевич"], str([v("CD"), v("CE"), v("CI")]))
        check("BX родственника жениха нет", v("BX") is None)
        check("BC поручитель №1 по жениху", v("BC") == "Иван Константинович", str(v("BC")))
        check("BG №2 по жениху", v("BG") == "Терентий Николаевич", str(v("BG")))
        check("BK №3 по жениху — пятый по счёту, он по жениху", v("BK") == "Кузьма Егорович", str(v("BK")))
        check("BO №1 по невесте — третий по счёту", v("BO") == "Алексей Степанович", str(v("BO")))
        check("BS №2 по невесте", v("BS") == "Семён Павлович", str(v("BS")))
        check("BW четвёртый по жениху — в свободный блок; отчество не из словаря — как в книге",
              v("BW") == "Фёдор Ильин",
              str(v("BW")))
        check("CJ/CK священник, CN/CO диакон, CP/CQ пономарь в колонке псаломщика",
              [v("CJ"), v("CN"), v("CO"), v("CP"), v("CQ")]
              == ["священник", "диакон", "Павел Ильинский", "пономарь", "Троицкий Алексей Иванов"],
              str([v("CJ"), v("CN"), v("CO"), v("CP"), v("CQ")]))
        check("CR пометки — только своё примечание, без стороны", v("CR") == "Семён Павлов: сосед", str(v("CR")))
        check("CS комментарий начинается с жениха и невесты",
              (v("CS") or "").startswith("Михаил Дмитриев; Евдокия Савельева; Дмитрий Иванов; Иван Савельев"),
              str(v("CS")))

        print("\n5. Familio, СМЕРТЬ")
        deaths = rows(db, sql, "familio_death")
        r = deaths[0]
        check("L «жен», X имя, S НП", [v("L"), v("X"), v("S")] == ["жен", "Анна", "Кнышево"])
        check("Z–AC возраст «1,5 мес» — 1 мес 15 дн", [v("Z"), v("AA"), v("AB"), v("AC")] == [None, 1, None, 15])
        check("AD родство — «отец», AJ/AK отец", [v("AD"), v("AJ"), v("AK")] == ["отец", "Иван", "Петрович"])
        check("AL причина", v("AL") == "понос")
        check("AM/AN исповедовал — у кого «исповед» в примечании",
              [v("AM"), v("AN")] == ["священник", "Александр Рождественский"], str([v("AM"), v("AN")]))
        check("AQ/AR погребение — священник", [v("AQ"), v("AR")] == ["священник", "Александр Рождественский"])
        check("второй священник — в свободную колонку (дьякона)",
              v("AU") == "священник Воскресенской церкви" and v("AV") == "Иоанн Смирнов", str([v("AU"), v("AV")]))
        check("AZ пометки: примечание причта", v("AZ") == "Александр Рождественский: исповедовал", str(v("AZ")))
        r = deaths[1]
        check("без имени: X пусто, U звание, AL причина, L «муж»",
              v("X") is None and v("U") == "тело неизвестного человека мужеского пола"
              and v("AL") == "утонул в Волге" and v("L") == "муж", str([v("X"), v("U"), v("AL"), v("L")]))
        check("без имени: авторского комментария нет", v("BA") is None, str(v("BA")))

        print("\n6. Familio, location")
        loc = {r[0]: r for r in rows(db, sql, "familio_location")}
        check("места всех персон и село дела",
              {"Борисоглебское", "Чертеж Малый", "Чертеж Большой", "Поселихино", "Кнышево", "починок Смыгарев"}
              <= set(loc), str(sorted(loc)))
        r = loc["Борисоглебское"]
        check("C губерния со словом, D уезд со словом",
              (r[2] or "").endswith("губерния") and (r[3] or "").endswith("уезд"), str(r[2:5]))
        check("F кратко с типом", (r[5] or "").endswith("Борисоглебское") and r[5] != "Борисоглебское", str(r[5]))
        only = {r[0] for r in rows(db, sql, "familio_location", json.dumps([1887]))}
        check("за 1887 — только село дела (у ребёнка НП нет)", only == {"Борисоглебское"}, str(only))

        print("\n7. Excel: листы индексатора")
        r = rows(db, sql, "excel_births")[0]
        check("A–I", [v(c) for c in "ABCEFGHI"] ==
              [1, "ГА Костромской области", "Ф.56 Оп.31 Д.18", 1886, 1, "873", None, 1],
              str([v(c) for c in "ABCEFGHI"]))
        check("D «Церковь: … Село: … Уезд: … Губерния: …»",
              v("D") == "Церковь: Христорождественская Село: Борисоглебское Уезд: Макарьевский Губерния: Костромская")
        check("J/K даты текстом «05.01.1886»", [v("J"), v("K")] == ["05.01.1886", "06.01.1886"])
        check("L имя ребёнка как в книге", v("L") == "Татіяна", str(v("L")))
        check("M–P отец: НП, звание, ИОФ как в книге, вероисповедание",
              [v("M"), v("N"), v("O"), v("P")] == ["Чертеж Малый", "крестьянин", "Никита Алексеев", "православного"])
        check("AE–AH восприемник 1 с примечанием",
              [v("AE"), v("AF"), v("AG"), v("AH")] == ["Александр Арсеньев", "Чертеж Большой", "крестьянский сын", "того же дому"])
        check("AM восприемник 3", v("AM") == "Пётр Сидоров")
        check("AU–AY причт по номеру, как у индексатора",
              [v("AU"), v("AV"), v("AX"), v("AY")] == ["Александр Рождественский", "священник", "Василий Васильев Промтов", "псаломщик"])
        r = rows(db, sql, "excel_marriages")[0]
        check("брак: H счёт, J дата, I/K пусты", [v("H"), v("I"), v("J"), v("K")] == [1, None, "31.01.1886", None])
        check("Q «каким браком», R лет, W, X — невеста",
              [v("Q"), v("R"), v("W"), v("X")] == ["Первым браком", 22, "Первым браком", 19])
        check("Y–AD родственники жениха и невесты",
              [v("Y"), v("Z"), v("AA"), v("AB"), v("AC")] == ["отец", "Дмитрий Иванов", "крестьянин", "брат", "Иван Савельев"])
        check("AH «Прим.» поручителя — сторона", v("AH") == "по жениху")
        check("AT «Прим.» поручителя 4 — сторона и своё", v("AT") == "по невесте; сосед", str(v("AT")))
        check("BD–BK поручители 5 и 6", [v("BD"), v("BG"), v("BH")] == ["Кузьма Егоров", "по жениху", "Фёдор Ильин"])
        d = rows(db, sql, "excel_deaths")
        r = d[0]
        check("смерть: Q причина, R возраст как в книге, W родство",
              [v("Q"), v("R"), v("W")] == ["понос", "1,5 мес", "отец"], str([v("Q"), v("R"), v("W")]))
        r = d[1]
        check("смерть без имени: O пусто, N звание, H счёт М",
              v("O") is None and v("N") == "тело неизвестного человека мужеского пола" and v("H") == 5)

        print("\n8. Excel: лист «МК» — строка на персону")
        mk = rows(db, sql, "excel_mk")
        check("персон 19 (причт не персона)", len(mk) == 19, str(len(mk)))
        r = mk[0]
        check("ребёнок: код 1, роль, дата рождения",
              [v("A"), v("B"), v("C"), v("D"), v("E")] == [1, 1886, 1, "ребенок", "05.01.1886"],
              str([v("A"), v("B"), v("C"), v("D"), v("E")]))
        check("ребёнок: отчество по имени отца, «дочь», НП отца",
              [v("G"), v("K"), v("L"), v("M")] == ["Татьяна Никитична", "Никитична", "дочь", "Чертеж Малый"],
              str([v("G"), v("K"), v("L"), v("M")]))
        check("N…BP — строка листа записи", r[col("N"):col("BP") + 1] == rows(db, sql, "excel_births")[0])
        check("BS ID записи, BU церковь, BV село", [v("BS"), v("BU"), v("BV")] ==
              ["GM-1", "Христорождественская", "Борисоглебское"], str([v("BS"), v("BU"), v("BV")]))
        r = mk[1]
        check("отец: коридор рождения 1831-1869, ФИО современное",
              [v("C"), v("E"), v("G")] == [2, "1831-1869", "Никита Алексеевич"], str([v("C"), v("E"), v("G")]))
        dead = [x for x in mk if x[col("C")] == 12]
        check("умерший: F дата смерти, E год по возрасту", dead and dead[0][col("F")] == "01.02.1886"
              and dead[0][col("E")] == 1886, str(dead[0][col("E"):col("G")] if dead else None))
        rel = [x for x in mk if x[col("C")] == 13]
        check("отец умершего — код 13, «умершего отец»", rel and rel[0][col("D")] == "умершего отец")
        kin = [x for x in mk if x[col("C")] == 18]
        check("брат невесты — код 18", kin and kin[0][col("D")] == "родственник невесты брат")
        check("H К-во: одинаковые ФИО считаются",
              all(x[col("H")] == sum(1 for y in mk if y[col("G")] == x[col("G")]) for x in mk if x[col("G")]))

        # Для проверки файла, который пишет Rust (export.rs, тест export_real):
        # GENMETRIC_KEEP_DB=путь — сохранить набранную здесь базу.
        import os
        keep = os.environ.get("GENMETRIC_KEEP_DB")
        if keep:
            db.commit()
            Path(keep).unlink(missing_ok=True)
            db.execute("VACUUM INTO ?", (keep,))

        print("\n9. Повторная выгрузка в том же соединении")
        db.executescript(sql["export_prepare"])
        check("временные таблицы пересобираются без ошибки", len(rows(db, sql, "familio_birth")) == 2)
        # Запись без года (до 22.09 год не был обязателен): со всем приходом
        # выгружается, по списку годов — нет, и окно это показывает.
        Book(db, sql).entry(1, [dict(role_code="child", sort_order=10, first_name="Ольга",
                                     first_name_modern="Ольга", gender="Ж")], no_female=9)
        db.executescript(sql["export_prepare"])
        years = db.execute(sql["export_years"]).fetchall()
        check("без года — отдельной строкой, последней", years[-1] == (None, 1, 0, 0), str(years))
        check("весь приход ([]) — запись без года выгружается", len(rows(db, sql, "familio_birth")) == 3)
        check("по списку годов — нет", len(rows(db, sql, "familio_birth", json.dumps([1886, 1887]))) == 2)
        check("в базе пользователя временных таблиц нет",
              db.execute("SELECT count(*) FROM sqlite_master WHERE name LIKE 'x_%'").fetchone()[0] == 0)

    print(f"\nИтог: успешно {ok_count}, ошибок {fail_count}")
    return 1 if fail_count else 0


if __name__ == "__main__":
    raise SystemExit(main())
