#!/usr/bin/env python3
"""
Проверка сверки имён со справочником и карточки населённого пункта
(spec/2026-09-23-sverka-imen-kartochka-np-shirina.md).

Разбор ИОФ живёт в Rust (parse_iof_in) и в песочнице не собирается, поэтому
здесь повторены его запросы к словарю и правило normalize_name — с проверкой
текста main.rs, что правило там то же (грабли 21.09: тест, который не ходит
через Rust, не видит Rust).

Что проверяется:
  1. конечный «ъ» отбрасывается: «Иванъ» известен, современное «Иван»;
     «Петровъ» — отчество;
  2. «Пискарь» словарю неизвестен; после alias_save → «Кесарь» — известен,
     пол М, современное «Кесарь»; «новое имя» без цели — известно с полом;
  3. похожие: для «Пискарь» среди первых пяти «Кесарь»; для «Букарина» —
     «Бухарино» первым; расстояние Дамерау — Левенштейна по символам;
  4. карточка НП: place_save собирает short/full_location, place_find
     находит, повторное сохранение не задваивает;
  5. схема 5: таблица name_alias в поставке, UNIQUE(kind, form_norm).

Запуск:
    python3 db/test_parse.py
"""

import re
import sqlite3
import sys
import tempfile
import unicodedata
from pathlib import Path

DB_DIR = Path(__file__).resolve().parent
ROOT = DB_DIR.parent

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


def norm(value) -> str:
    if not value:
        return ""
    s = unicodedata.normalize("NFC", str(value)).strip().lower().replace("ё", "е")
    return re.sub(r"\s+", " ", s)


def normalize_name(word: str) -> str:
    """Копия normalize_name из main.rs."""
    n = norm(word).replace("і", "и").replace("ѣ", "е").replace("ѳ", "ф")
    return n[:-1] if n.endswith("ъ") else n


def edit_distance(a: str, b: str) -> int:
    """Копия edit_distance из main.rs (Дамерау — Левенштейн, по символам)."""
    n, m = len(a), len(b)
    d = [[0] * (m + 1) for _ in range(n + 1)]
    for i in range(n + 1):
        d[i][0] = i
    for j in range(m + 1):
        d[0][j] = j
    for i in range(1, n + 1):
        for j in range(1, m + 1):
            cost = 0 if a[i - 1] == b[j - 1] else 1
            v = min(d[i - 1][j] + 1, d[i][j - 1] + 1, d[i - 1][j - 1] + cost)
            if i > 1 and j > 1 and a[i - 1] == b[j - 2] and a[i - 2] == b[j - 1]:
                v = min(v, d[i - 2][j - 2] + 1)
            d[i][j] = v
    return d[n][m]


def rank_similar(query, items, limit, max_dist):
    """Копия rank_similar из main.rs: (norm, value) → [value]."""
    scored = []
    for nrm, value in items:
        dist = edit_distance(query, nrm)
        if dist > max_dist:
            continue
        prefix = 0
        for x, y in zip(query, nrm):
            if x != y:
                break
            prefix += 1
        scored.append((dist, -prefix, value))
    scored.sort()
    out, seen = [], set()
    for _, _, value in scored:
        if value in seen:
            continue
        seen.add(value)
        out.append(value)
    return out[:limit]


def load_statements() -> dict:
    text = (DB_DIR / "statements.sql").read_text(encoding="utf-8")
    blocks, name, buf = {}, None, []
    for line in text.splitlines():
        marker = line.strip()
        if marker.startswith("-- @"):
            if name:
                blocks[name] = "\n".join(buf).strip()
            name, buf = marker[4:].strip(), []
        elif name is not None:
            buf.append(line)
    if name:
        blocks[name] = "\n".join(buf).strip()
    return blocks


def parse(db, sql, text):
    """Повтор parse_iof_in: имя через алиас или словарь, отчество так же."""
    tokens = text.split()
    out = dict(known=False, gender=None, modern=None, patronymic=None, name_alias=None)
    if not tokens:
        return out
    word = tokens[0]
    row = db.execute(sql["alias_find"], dict(kind="name", form_norm=normalize_name(word))).fetchone()
    if row and row[0]:
        out["name_alias"] = row[0]
        word = row[0]
    elif row:
        out.update(known=True, gender=row[1], modern=tokens[0])
    if not out["known"]:
        head = db.execute(
            """SELECT d.name, d.base_name, d.gender FROM name_form f JOIN name_dict d ON d.id = f.name_id
                WHERE f.kind IN ('name','variant') AND f.form_norm = ? ORDER BY f.priority LIMIT 1""",
            (normalize_name(word),)).fetchone()
        if head:
            out.update(known=True, gender=head[2], modern=head[1] or head[0])
    if len(tokens) > 1:
        pw = tokens[1]
        row = db.execute(sql["alias_find"], dict(kind="patr", form_norm=normalize_name(pw))).fetchone()
        if row and row[0]:
            pw = row[0]
        patr = db.execute(
            """SELECT d.name FROM name_form f JOIN name_dict d ON d.id = f.name_id
                WHERE f.kind LIKE 'patr%' AND f.form_norm = ? ORDER BY f.priority LIMIT 1""",
            (normalize_name(pw),)).fetchone()
        if patr:
            out["patronymic"] = tokens[1]
    return out


def main() -> int:
    _utf8_stdout()
    sys.path.insert(0, str(DB_DIR))
    import build_seed

    sql = load_statements()
    rust = (ROOT / "src-tauri" / "src" / "main.rs").read_text(encoding="utf-8")

    print("\n0. Запросы и правило на месте")
    rust = (ROOT / "src-tauri" / "src" / "main.rs").read_text(encoding="utf-8")
    for required in ("alias_find", "alias_save", "name_headwords", "patr_forms",
                     "place_save", "place_names", "place_find", "place_get", "place_update",
                     "place_name_taken", "place_rename_persons", "place_rename_spouses",
                     "place_rename_usage"):
        check(f"блок {required} на месте", required in sql)
    check("main.rs: normalize_name отбрасывает конечный «ъ»",
          "strip_suffix('ъ')" in rust)
    check("main.rs: «Это не отчество» (соответствие без цели) больше не спрашивает",
          "Some((None, _)) => not_patr = true" in rust and "!not_patr && looks_like_patronymic(first_rest)" in rust)
    check("main.rs: отчество без фамилии тоже сверяется («Иван Пискарев»)",
          "rest.len() >= 2 && looks_like_patronymic" not in rust)
    check("main.rs: normalize_name знает «ѳ»", ".replace('ѳ', \"ф\")" in rust)
    check("main.rs: разбор ищет по normalize_name, не normalize",
          "[normalize_name(&lookup_word)]" in rust and "[normalize_name(&patr_word)]" in rust)
    check("main.rs: порог похожести имён — треть длины, не меньше 3",
          "(q.chars().count() / 3).max(3)" in rust)
    check("main.rs: порог похожести мест — четверть длины, не меньше 2",
          "(norm.chars().count() / 4).max(2)" in rust)
    form = (ROOT / "src" / "BirthForm.tsx").read_text(encoding="utf-8")
    check("BirthForm: запись с несверенным именем не сохраняется",
          "не сверено со справочником" in form and "known_name" in form)
    iof = (ROOT / "src" / "IofField.tsx").read_text(encoding="utf-8")
    check("IofField: сверка — при уходе из поля, не при наборе",
          "checkOnLeave" in iof and "onBlur" in iof)

    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "db.sqlite"
        build_seed.build(path)
        db = sqlite3.connect(path)
        db.execute("PRAGMA foreign_keys = ON")
        one = lambda q, *a: db.execute(q, a).fetchone()

        print("\n5. Схема 5: name_alias")
        check("версия схемы 6", one("SELECT max(version) FROM schema_version")[0] == 6)
        check("таблица name_alias есть и пуста", one("SELECT count(*) FROM name_alias")[0] == 0)

        print("\n1. Конечный «ъ»")
        p = parse(db, sql, "Иванъ Петровъ Сидоровъ")
        check("«Иванъ» известен", p["known"])
        check("современное — «Иван»", p["modern"] == "Иван", p["modern"])
        check("«Петровъ» — отчество", p["patronymic"] == "Петровъ", p["patronymic"])
        check("пол М", p["gender"] == "М")
        p = parse(db, sql, "Ѳома")
        check("«Ѳома» через ѳ → «Фома», известен", p["known"] and p["modern"] == "Фома", str(p))
        p = parse(db, sql, "Іоаннъ")
        check("«Іоаннъ» через і → известен", p["known"], str(p))

        print("\n2. Соответствие «Пискарь» → «Кесарь»")
        p = parse(db, sql, "Пискарь Иванов Сидоров")
        check("«Пискарь» неизвестен", not p["known"])
        db.execute(sql["alias_save"], dict(kind="name", form="Пискарь",
                                           form_norm=normalize_name("Пискарь"),
                                           target="Кесарь", gender=None))
        p = parse(db, sql, "Пискарь Иванов Сидоров")
        check("после «Запомнить» известен", p["known"])
        check("алиас в разборе", p["name_alias"] == "Кесарь", p["name_alias"])
        check("современное — «Кесарь»", p["modern"] == "Кесарь", p["modern"])
        check("пол из словаря — М", p["gender"] == "М", p["gender"])
        check("«Пискарьъ» с «ъ» — то же соответствие",
              parse(db, sql, "Пискарьъ")["known"])
        db.execute(sql["alias_save"], dict(kind="name", form="Пискарь",
                                           form_norm=normalize_name("Пискарь"),
                                           target="Цезарь", gender=None))
        check("повторное решение заменяет прежнее",
              one("SELECT count(*), max(target) FROM name_alias WHERE kind='name'") == (1, "Цезарь"))
        db.execute(sql["alias_save"], dict(kind="name", form="Ждан",
                                           form_norm=normalize_name("Ждан"),
                                           target=None, gender="М"))
        p = parse(db, sql, "Ждан Петров")
        check("«новое имя» без цели — известно, пол из соответствия",
              p["known"] and p["gender"] == "М" and p["modern"] == "Ждан", str(p))
        db.execute(sql["alias_save"], dict(kind="patr", form="Пискарев",
                                           form_norm=normalize_name("Пискарев"),
                                           target="Кесарев", gender=None))
        p = parse(db, sql, "Иван Пискарев Сидоров")
        check("соответствие отчества: «Пискарев» опознано", p["patronymic"] == "Пискарев")

        db.execute(sql["alias_save"], dict(kind="patr", form="Зорянова",
                                           form_norm=normalize_name("Зорянова"),
                                           target=None, gender=None))
        check("«Это не отчество» запомнено: соответствие без цели",
              db.execute(sql["alias_find"], dict(kind="patr", form_norm="зорянова")).fetchone() == (None, None))

        print("\n3. Похожие")
        check("расстояние букарина ↔ бухарино = 2",
              edit_distance("букарина", "бухарино") == 2)
        check("перестановка считается за 1", edit_distance("ивна", "иван") == 1)
        names = [(r[1], r[0]) for r in db.execute(sql["name_headwords"])
                 if r[2] == "М"]
        names_dist = lambda q: max(len(q) // 3, 3)
        top = rank_similar(normalize_name("Пискарь"), names, 5, names_dist("пискарь"))
        check("«Кесарь» в первой пятёрке похожих на «Пискарь»", "Кесарь" in top, str(top))
        check("«Марья» не в основах (base_name)",
              one("SELECT count(*) FROM name_dict WHERE name='Марья' AND coalesce(base_name,'')=''")[0] == 0)
        patrs = [(r[1], r[0]) for r in db.execute(sql["patr_forms"]) if r[2] == "М"]
        top = rank_similar(normalize_name("Пискарев"), patrs, 5, names_dist("пискарев"))
        check("«Кесарев» среди похожих отчеств", "Кесарев" in top, str(top))

        print("\n4. Карточка населённого пункта")
        places = [(r[1], r[0]) for r in db.execute(sql["place_names"])]
        place_dist = lambda q: max(len(q) // 4, 2)
        top = rank_similar(norm("Букарина"), places, 8, place_dist("букарина"))
        check("«Бухарино» — первое похожее на «Букарина»",
              top and "Бухарино" in top[0], str(top[:3]))
        top = rank_similar(norm("Новодеревенька"), places, 8, place_dist("новодеревенька"))
        check("на «Новодеревенька» ложных похожих нет", top == [], str(top))
        card = dict(name="Новодеревенька", name_norm=norm("Новодеревенька"), np_type="д.",
                    guberniya="Костромская", uyezd="Макарьевский", volost="Заобнорская",
                    familio_url=None)
        db.execute(sql["place_save"], card)
        row = one("SELECT short_location, full_location, origin FROM place WHERE name_norm=?",
                  card["name_norm"])
        check("short_location собран", row[0] == "д. Новодеревенька", row[0])
        check("full_location собран",
              row[1] == "д. Новодеревенька, Заобнорская волость, Макарьевский уезд, Костромская губерния",
              row[1])
        check("origin = user", row[2] == "user")
        check("place_find находит", one(sql["place_find"], card["name_norm"]) is not None)
        card2 = dict(name="Пустошь", name_norm=norm("Пустошь"), np_type=None, guberniya=None,
                     uyezd=None, volost=None, familio_url=None)
        db.execute(sql["place_save"], card2)
        row = one("SELECT short_location, full_location FROM place WHERE name_norm=?", card2["name_norm"])
        check("карточка без подробностей — без хвостов", row == ("Пустошь", "Пустошь"), str(row))
        got = one(sql["place_get"], card["name_norm"])
        check("place_get отдаёт карточку", got is not None and got[1] == "Новодеревенька" and got[3] == "Костромская", str(got))
        db.execute(sql["place_update"], dict(id=got[0], name="Новодеревенька", name_norm=norm("Новодеревенька"),
                                             np_type="с.", guberniya="Ярославская",
                                             uyezd="Любимский", volost=None, familio_url="https://familio.org/x"))
        row = one("SELECT name, np_type, guberniya, full_location, familio_url FROM place WHERE id=?", got[0])
        check("place_update: поля новые",
              row[0] == "Новодеревенька" and row[1] == "с." and row[2] == "Ярославская", str(row))
        check("place_update пересобрал full_location",
              row[3] == "с. Новодеревенька, Любимский уезд, Ярославская губерния", row[3])
        check("place_update: ссылка", row[4] == "https://familio.org/x")

        print("\n5б. Звания в старой орфографии (техдолг)")
        words = lambda v: " ".join(normalize_name(w) for w in v.split())
        check("«крестьянинъ» находит «крестьянин» в перечне",
              one(sql["lookup_by_norm"], "rank_m", words("крестьянинъ")) == ("крестьянин",))
        check("«крестьянскій сынъ» находит «крестьянский сын»",
              one(sql["lookup_by_norm"], "rank_m", words("крестьянскій сынъ")) == ("крестьянский сын",))
        check("main.rs: подсказка званий по normalize_words, remember — канонично",
              "like_prefix(&normalize_words(&prefix))" in rust and '"lookup_by_norm"' in rust)

        print("\n6. Переименование НП (Роман 25.09.2026)")
        db.execute("INSERT INTO person_index (iof, iof_norm, place, rank, gender, uses) "
                   "VALUES ('Иван Петров', 'иван петров', 'Новодеревенька', 'крестьянин', 'М', 1)")
        db.execute("INSERT INTO usage_stat (kind, scope, scope_key, value, value_norm, count) "
                   "VALUES ('place', 'global', '', 'Новодеревенька', 'новодеревенька', 3)")
        check("название занято — другой пункт находится",
              one(sql["place_name_taken"], norm("Бухарино"), got[0]) is not None)
        check("своё же название не считается занятым",
              one(sql["place_name_taken"], norm("Новодеревенька"), got[0]) is None)
        params = dict(id=got[0], name="Новая Деревенька", name_norm=norm("Новая Деревенька"),
                      np_type="д.", guberniya="Костромская", uyezd="Макарьевский", volost=None, familio_url=None)
        db.execute(sql["place_update"], params)
        db.execute(sql["place_rename_persons"], dict(name="Новая Деревенька", old_name="Новодеревенька"))
        db.execute(sql["place_rename_spouses"], dict(name="Новая Деревенька", old_name="Новодеревенька"))
        db.execute(sql["place_rename_usage"], dict(name="Новая Деревенька", name_norm=norm("Новая Деревенька"),
                                                   old_name="Новодеревенька"))
        db.execute(sql["place_renamed_remember"], dict(old_norm=norm("Новодеревенька")))
        check("прежнее название запомнено для обновлений",
              one("SELECT count(*) FROM place_renamed WHERE old_norm='новодеревенька'")[0] == 1)
        row = one("SELECT name, name_norm, short_location FROM place WHERE id=?", got[0])
        check("переименован: название, ключ поиска, краткая сборка",
              row == ("Новая Деревенька", "новая деревенька", "д. Новая Деревенька"), str(row))
        check("старое название не находится", one(sql["place_find"], norm("Новодеревенька")) is None)
        check("персона в памяти — с новым названием",
              one("SELECT place FROM person_index WHERE iof='Иван Петров'")[0] == "Новая Деревенька")
        check("частоты подсказок — с новым названием",
              one("SELECT count(*) FROM usage_stat WHERE kind='place' AND value='Новая Деревенька'")[0] == 1)
        rust = (ROOT / "src-tauri" / "src" / "main.rs").read_text(encoding="utf-8")
        check("main.rs: переименование одной транзакцией вместе с памятью",
              "unchecked_transaction" in rust and 'statement("place_rename_usage")' in rust)
        check("main.rs: «название занято» проверяется только при смене названия",
              "if renamed {" in rust)
        check("main.rs: занятое название — понятная ошибка, не UNIQUE",
              "уже есть в справочнике" in rust)
        check("place_get: не из архива первой",
              "ORDER BY (origin = 'archive'), id" in sql["place_get"])

    print(f"\nИтог: успешно {ok_count}, ошибок {fail_count}")
    return 1 if fail_count else 0


if __name__ == "__main__":
    sys.exit(main())
