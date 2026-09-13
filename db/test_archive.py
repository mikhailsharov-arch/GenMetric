#!/usr/bin/env python3
"""
Проверка переноса архива подсказок из Excel.

Файла Романа в репозитории нет и не будет, поэтому архив здесь собирается
из синтетических строк той же формы, что листы «1», «2», «3» его файла, —
через ту же функцию harvest_rows(), которой пользуется инструмент. Слияние
идёт тем же db/import_archive.sql, который выполняет приложение.

Что проверяется:
  1. из строк собираются персоны, жёны, причт, места, статистика;
  2. пол берётся из роли, а где роли нет — из словаря имён;
  3. архив сливается в базу, и подсказки начинают его видеть;
  4. набранное в программе до загрузки цело;
  5. повторное слияние не задваивает строки;
  6. мусорные ячейки («0», «-») не становятся названиями.

Запуск:
    python3 db/test_archive.py
"""

import sqlite3
import sys
import tempfile
from pathlib import Path

DB_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(DB_DIR))
sys.path.insert(0, str(DB_DIR / "tools"))

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


def row(**cells):
    """Строка листа: словарь «номер колонки → значение» в список из 55 ячеек."""
    r = [None] * 55
    r[0] = cells.pop("no", 1)
    for col, v in cells.items():
        r[int(col[1:]) - 1] = v
    return r


def main() -> int:
    _utf8_stdout()
    import build_seed
    import build_archive as ba

    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        seed = tmp / "seed.sqlite"
        build_seed.build(seed)
        dictionary = sqlite3.connect(seed)

        print("\n1. Сбор памяти из строк листов")
        births = [
            row(no=1, c13="Чертеж Малый", c14="крестьянин", c15="Никита Алексеев",
                c19="Чертеж Малый", c20="законная жена его", c21="Евлампия Васильева",
                c31="Александр Арсеньев", c32="Чертеж Малый", c33="крестьянский сын",
                c47="Александр Рождественский", c48="священник",
                c50="Василий Промтов", c51="псаломщик"),
            row(no=2, c13="Чертеж Малый", c14="крестьянин", c15="Никита Алексеев",
                c19="Чертеж Малый", c20="законная жена его", c21="Евлампия Васильева",
                c31="Дария Трофимова", c32="Бухарино", c33="крестьянская дочь девица",
                c47="Александр Рождественский", c48="священник"),
            row(no=3, c13="0", c14="-", c15="Иван Захаров",
                c47="Александр Рождественский", c48="священник"),
        ]
        marriages = [
            row(no=1, c13="Костино", c14="крестьянин", c15="Кузьма Петров",
                c19="Ушаково", c20="крестьянская дочь девица", c21="Марфа Павлова",
                c31="Гавриил Петров", c32="Костино", c33="крестьянин"),
        ]
        h = ba.Harvest()
        n1 = ba.harvest_rows("1", births, dictionary, h)
        n2 = ba.harvest_rows("2", marriages, dictionary, h)
        check("строки посчитаны", (n1, n2) == (3, 1), f"{n1}, {n2}")
        persons = {k[0]: v for k, v in h.persons.items()}
        check("отец запомнен с местом и званием",
              persons["никита алексеев"][1] == "Чертеж Малый" and persons["никита алексеев"][2] == "крестьянин")
        check("повтор отца сложился в частоту", persons["никита алексеев"][4] == 2)
        check("пол отца — из роли", persons["никита алексеев"][3] == "М")
        check("пол матери — из роли", persons["евлампия васильева"][3] == "Ж")
        check("пол восприемницы — из словаря имён", persons["дария трофимова"][3] == "Ж")
        check("пол восприемника — из словаря имён", persons["александр арсеньев"][3] == "М")
        check("жена по мужу из рождений",
              ("никита алексеев", "евлампия васильева") in h.spouses)
        check("невеста по жениху из браков",
              ("кузьма петров", "марфа павлова") in h.spouses)
        check("причт собран со званием",
              h.clergy[("александр рождественский", "священник")][2] == 3)
        check("«0» и «-» не стали местом и званием",
              "0" not in {v[0] for v in h.places.values()} and persons["иван захаров"][1] is None)
        check("частота имени считается", h.usage[("first_name", "никита")][1] == 2)
        check("звание женщины ушло в женский перечень", ("rank_f", "крестьянская дочь девица") in h.usage)

        print("\n2. Архив пишется")
        archive = tmp / "архив.sqlite"
        stats = ba.write_archive(h, archive, "тест.xlsm")
        check("архив собран", archive.exists() and stats["персон"] == len(h.persons), str(stats))

        print("\n3. Слияние в базу пользователя")
        user = tmp / "user.sqlite"
        build_seed.build(user)
        db = sqlite3.connect(user, isolation_level=None)
        one = lambda q, *a: db.execute(q, a).fetchone()[0]
        # Набранное в программе до загрузки — терять нельзя.
        db.execute("INSERT INTO person_index (iof, iof_norm, place, rank, gender, uses, last_used_at) "
                   "VALUES ('Никита Алексеев','никита алексеев','Чертеж Малый','крестьянин','М',5,datetime('now'))")
        db.execute("INSERT INTO person_index (iof, iof_norm, place, rank, gender, uses, last_used_at) "
                   "VALUES ('Своя Персона','своя персона','Кнышево','крестьянин','Ж',1,datetime('now'))")
        places_before = one("SELECT count(*) FROM place")

        db.execute("ATTACH DATABASE ? AS archive", (str(archive),))
        db.executescript((DB_DIR / "import_archive.sql").read_text(encoding="utf-8"))
        db.execute("DETACH DATABASE archive")

        check("персоны из архива появились", one("SELECT count(*) FROM person_index") >= len(h.persons))
        check("своя персона цела", one("SELECT uses FROM person_index WHERE iof='Своя Персона'") == 1)
        check("частота совпавшей персоны сложилась: 5 своих + 2 из архива",
              one("SELECT uses FROM person_index WHERE iof='Никита Алексеев'") == 7)
        check("жена по мужу доступна подсказке",
              one("SELECT wife_iof FROM spouse_index WHERE husband_norm='никита алексеев'") == "Евлампия Васильева")
        check("причт доступен списку",
              one("SELECT uses FROM clergy_index WHERE iof='Александр Рождественский'") == 3)
        check("новое место добавилось, известное не задвоилось",
              one("SELECT count(*) FROM place WHERE name_norm='ушаково'") == 1
              and one("SELECT count(*) FROM place WHERE name_norm='чертеж малый'") == 1)
        check("места из поставки не тронуты", one("SELECT count(*) FROM place") >= places_before)
        check("статистика имён перенесена",
              one("SELECT count FROM usage_stat WHERE kind='first_name' AND value_norm='никита'") == 2)
        check("отметка о загрузке записана",
              "тест.xlsm" in one("SELECT value FROM setting WHERE key='archive_loaded'"))

        print("\n4. Повторное слияние не задваивает строки")
        before = (one("SELECT count(*) FROM person_index"), one("SELECT count(*) FROM place"),
                  one("SELECT count(*) FROM clergy_index"), one("SELECT count(*) FROM spouse_index"))
        db.execute("ATTACH DATABASE ? AS archive", (str(archive),))
        db.executescript((DB_DIR / "import_archive.sql").read_text(encoding="utf-8"))
        db.execute("DETACH DATABASE archive")
        after = (one("SELECT count(*) FROM person_index"), one("SELECT count(*) FROM place"),
                 one("SELECT count(*) FROM clergy_index"), one("SELECT count(*) FROM spouse_index"))
        check("число строк не изменилось", before == after, f"{before} → {after}")
        check("integrity_check", one("PRAGMA integrity_check") == "ok")

        print("\n5. Подсказки видят архив")
        # Тем же запросом, что и приложение.
        sql = {}
        name, buf = None, []
        for line in (DB_DIR / "statements.sql").read_text(encoding="utf-8").splitlines():
            m = line.strip()
            if m.startswith("-- @"):
                if name: sql[name] = "\n".join(buf).strip()
                name, buf = m[4:].strip(), []
            elif name is not None:
                buf.append(line)
        if name: sql[name] = "\n".join(buf).strip()
        got = [r[0] for r in db.execute(sql["person_suggest"],
                                        {"prefix": "кузьма%", "limit": 5, "gender": "М"})]
        check("персона из архива предлагается", "Кузьма Петров" in got, ", ".join(got))
        got = db.execute(sql["spouse_lookup"], {"husband_norm": "кузьма петров"}).fetchone()
        check("невеста из архива подставляется по жениху", got and got[0] == "Марфа Павлова")

        db.close()
        dictionary.close()

    print(f"\nИтог: успешно {ok_count}, ошибок {fail_count}")
    return 1 if fail_count else 0


if __name__ == "__main__":
    raise SystemExit(main())
