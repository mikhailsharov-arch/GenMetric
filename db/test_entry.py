#!/usr/bin/env python3
"""
Проверка сохранения записи о рождении.

Запросы берутся из db/statements.sql — того же файла, что читает приложение.
Иначе проверялась бы похожая копия, а не то, что работает у человека: код
на Rust в песочнице не собирается.

Что проверяется:
  1. запись со всеми персонами сохраняется и читается обратно;
  2. правка записи не плодит дубликаты персон;
  3. новое звание попадает в справочник, а служебные перечни не пополняются;
  4. частота использования растёт в трёх охватах сразу — на ней держится
     порядок подсказок;
  5. населённый пункт заводится по первому упоминанию и не задваивается.

Запуск:
    python3 db/test_entry.py
"""

import re
import sqlite3
import sys
import tempfile
import unicodedata
from pathlib import Path

DB_DIR = Path(__file__).resolve().parent

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


def load_statements() -> dict:
    """Разбирает db/statements.sql на именованные блоки.

    Тот же разбор делает приложение — формат намеренно простейший."""
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


def main() -> int:
    _utf8_stdout()
    sys.path.insert(0, str(DB_DIR))
    import build_seed

    sql = load_statements()
    print(f"\n0. Запросы из statements.sql: {len(sql)} блоков")
    for required in ("case_upsert", "entry_insert", "mention_insert", "lookup_extend",
                     "usage_bump", "entry_list", "place_insert",
                     "person_remember", "person_suggest", "spouse_remember", "spouse_lookup",
                     "clergy_remember", "clergy_list"):
        check(f"блок {required} на месте", required in sql)

    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "db.sqlite"
        build_seed.build(path)
        db = sqlite3.connect(path)
        db.execute("PRAGMA foreign_keys = ON")
        one = lambda q, *a: db.execute(q, a).fetchone()

        print("\n1. Дело заводится один раз")
        case = dict(id=1, archive="ГА Костромской области", fond="56", opis="31", delo="18",
                    church="Христорождественская", village="Борисоглебское",
                    uyezd="Макарьевский", guberniya="Костромская", year=1893,
                    parish_key="Христорождественская|Борисоглебское|Макарьевский|Костромская",
                    indexer="Роман Чистов")
        db.execute(sql["case_upsert"], case)
        check("дело сохранено", one("SELECT year FROM mk_case WHERE id=1")[0] == 1893)
        case["year"] = 1894
        db.execute(sql["case_upsert"], case)
        check("повторное сохранение не плодит дела", one("SELECT count(*) FROM mk_case")[0] == 1)
        check("год обновился", one("SELECT year FROM mk_case WHERE id=1")[0] == 1894)

        print("\n2. Населённый пункт заводится по первому упоминанию")
        # «Чертеж Малый» теперь есть в поставке: справочник НП по Борисоглебскому
        # приходу вшит, иначе поле НП нечем подсказывать на первой же странице.
        # Проверяем поэтому не общее число пунктов, а что задвоения не случилось.
        # «Пустошка» в поставке отсутствует — на ней видно, что новый пункт
        # действительно заводится.
        seeded = one("SELECT count(*) FROM place")[0]
        check("справочник НП пришёл из поставки", seeded > 100, f"{seeded} пунктов")
        for name in ("Чертеж Малый", "Чертеж Малый", "Пустошка", "Пустошка"):
            found = db.execute(sql["place_find"], {"name_norm": norm(name)}).fetchone()
            if not found:
                db.execute(sql["place_insert"], {"name": name, "name_norm": norm(name)})
        check("известный пункт не задваивается",
              one("SELECT count(*) FROM place WHERE name_norm = ?",
                  norm("Чертеж Малый"))[0] == 1)
        check("новый пункт заведён один раз",
              one("SELECT count(*) FROM place WHERE name_norm = ?",
                  norm("Пустошка"))[0] == 1)
        check("поставку не тронули", one("SELECT count(*) FROM place")[0] == seeded + 1)
        place_id = one("SELECT id FROM place WHERE name_norm = ?", norm("Чертеж Малый"))[0]

        print("\n3. Запись о рождении со всеми персонами")
        db.execute(sql["entry_insert"], dict(
            case_id=1, section=1, page="909", no_male=None, no_female=2,
            event_day=5, event_month=1, event_year=1893,
            rite_day=7, rite_month=1, rite_year=1893,
            note=None, uncertain=None, created_by="Роман Чистов"))
        entry_id = one("SELECT id FROM entry")[0]

        # Порядок полей и состав персон — как читается запись в книге:
        # ребёнок, затем отец с его НП и званием, затем мать, затем восприемники.
        persons = [
            dict(role_code="child", sort_order=10, first_name="Татьяна",
                 patronymic="Никитична", gender="Ж"),
            dict(role_code="father", sort_order=20, first_name="Никита",
                 patronymic="Алексеев", patronymic_modern="Алексеевич",
                 gender="М", rank="крестьянин", place_id=place_id,
                 birth_year_from=1838, birth_year_to=1876),
            # У матери НП и звание почти всегда наследуются от отца: на данных
            # Романа совпадение 292 из 294, а звание «законная жена его» — 293
            # из 294. Поэтому форма подставляет их сама.
            dict(role_code="mother", sort_order=30, first_name="Евлампия",
                 patronymic="Васильева", patronymic_modern="Васильевна",
                 gender="Ж", rank="законная жена его", place_id=place_id),
            dict(role_code="godparent1", sort_order=40, first_name="Александр",
                 patronymic="Арсеньев", patronymic_modern="Арсениевич",
                 gender="М", rank="крестьянский сын", place_id=place_id),
        ]
        blank = dict(surname=None, first_name=None, patronymic=None, surname_modern=None,
                     first_name_modern=None, patronymic_modern=None, maiden_surname=None,
                     gender=None, rank=None, confession=None, place_id=None, note=None,
                     uncertain=None, birth_year_from=None, birth_year_to=None)
        for p in persons:
            db.execute(sql["mention_insert"], {**blank, **p, "entry_id": entry_id})
        check("персоны сохранены", one("SELECT count(*) FROM person_mention")[0] == 4)
        check("роли из справочника ролей",
              one("SELECT count(*) FROM person_mention m LEFT JOIN role r "
                  "ON r.code = m.role_code WHERE r.code IS NULL")[0] == 0)

        print("\n4. Правка записи не плодит персон")
        db.execute(sql["mentions_clear"], {"entry_id": entry_id})
        for p in persons:
            db.execute(sql["mention_insert"], {**blank, **p, "entry_id": entry_id})
        check("персон по-прежнему 4", one("SELECT count(*) FROM person_mention")[0] == 4)
        check("запись одна", one("SELECT count(*) FROM entry")[0] == 1)

        print("\n5. Справочник пополняется сам")
        before = one("SELECT count(*) FROM lookup WHERE kind='rank_m'")[0]
        db.execute(sql["lookup_extend"], {"kind": "rank_m", "value": "бобыль деревни Кнышево",
                                          "value_norm": norm("бобыль деревни Кнышево")})
        check("новое звание добавлено",
              one("SELECT count(*) FROM lookup WHERE kind='rank_m'")[0] == before + 1)
        db.execute(sql["lookup_extend"], {"kind": "rank_m", "value": "бобыль деревни Кнышево",
                                          "value_norm": norm("бобыль деревни Кнышево")})
        check("повторное сохранение не задваивает",
              one("SELECT count(*) FROM lookup WHERE kind='rank_m'")[0] == before + 1)
        check("значение помечено как пользовательское",
              one("SELECT origin FROM lookup WHERE value='бобыль деревни Кнышево'")[0] == "user")
        # Перечни, которые пополнять нельзя: окончания фамилий, «каким браком».
        db.execute(sql["lookup_extend"], {"kind": "surname_end_m", "value": "ъъъ",
                                          "value_norm": "ъъъ"})
        check("служебный перечень не пополняется",
              one("SELECT count(*) FROM lookup WHERE value='ъъъ'")[0] == 0)

        print("\n6. Частота использования растёт в трёх охватах")
        for scope, key in (("case", "1"), ("parish", case["parish_key"]), ("global", "")):
            db.execute(sql["usage_bump"], {"kind": "rank_m", "scope": scope, "scope_key": key,
                                           "value": "крестьянин", "value_norm": "крестьянин"})
        check("три записи статистики", one("SELECT count(*) FROM usage_stat")[0] == 3)
        for scope, key in (("case", "1"), ("parish", case["parish_key"]), ("global", "")):
            db.execute(sql["usage_bump"], {"kind": "rank_m", "scope": scope, "scope_key": key,
                                           "value": "крестьянин", "value_norm": "крестьянин"})
        check("повторный ввод увеличивает счётчик, а не плодит строки",
              one("SELECT count(*) FROM usage_stat")[0] == 3)
        check("счётчик стал 2",
              one("SELECT count FROM usage_stat WHERE scope='case'")[0] == 2)

        print("\n7. Список записей для возврата и правки")
        rows = db.execute(sql["entry_list"], {"case_id": 1, "section": 1}).fetchall()
        check("запись видна в списке", len(rows) == 1)
        check("в списке имя ребёнка", rows[0][7] == "Татьяна Никитична", rows[0][7])

        print("\n8. Память о персонах: выбор заполняет три поля разом")
        # Главное требование заказчика от 17.08.2026. Раньше он набирал ИОФ,
        # населённый пункт и звание по отдельности для каждой персоны.
        for iof, place_name, rank, gender in [
            ("Никита Алексеев", "Чертеж Малый", "крестьянин", "М"),
            ("Никита Алексеев", "Чертеж Малый", "крестьянин", "М"),
            ("Никита Алексеев", "Кнышево", "крестьянин", "М"),
            ("Никифор Иванов", "Чертеж Малый", "солдат", "М"),
        ]:
            db.execute(sql["person_remember"], {"iof": iof, "iof_norm": norm(iof),
                                                "place": place_name, "rank": rank, "gender": gender})
        check("персоны запомнены без задвоения",
              one("SELECT count(*) FROM person_index")[0] == 3, "три разные связки ИОФ+НП+звание")
        check("повторный ввод увеличил счётчик",
              one("SELECT uses FROM person_index WHERE place='Чертеж Малый' AND iof='Никита Алексеев'")[0] == 2)

        rows = db.execute(sql["person_suggest"],
                          {"prefix": norm("Ник") + "%", "limit": 8}).fetchall()
        check("подсказка находит персон по началу строки", len(rows) == 3, f"{len(rows)} персон")
        check("первым идёт тот, кого вводили чаще", rows[0][0] == "Никита Алексеев" and rows[0][4] == 2,
              f"{rows[0][0]}, {rows[0][1]}, {rows[0][4]} раз")
        check("вместе с ИОФ приходят населённый пункт и звание",
              rows[0][1] == "Чертеж Малый" and rows[0][2] == "крестьянин")
        check("однофамильцы из разных мест различимы",
              {r[1] for r in rows} == {"Чертеж Малый", "Кнышево"})

        print("\n9. Память о супругах: выбор отца заполняет мать")
        db.execute(sql["spouse_remember"], {
            "husband_norm": norm("Никита Алексеев"), "wife_iof": "Евлампия Васильева",
            "wife_place": "Чертеж Малый", "wife_rank": "законная жена его"})
        db.execute(sql["spouse_remember"], {
            "husband_norm": norm("Никита Алексеев"), "wife_iof": "Евлампия Васильева",
            "wife_place": "Чертеж Малый", "wife_rank": "законная жена его"})
        wife = db.execute(sql["spouse_lookup"],
                          {"husband_norm": norm("Никита Алексеев")}).fetchone()
        check("жена находится по мужу", wife is not None and wife[0] == "Евлампия Васильева")
        check("с ней приходят её населённый пункт и звание",
              wife[1] == "Чертеж Малый" and wife[2] == "законная жена его")
        check("пара не задвоилась", one("SELECT count(*) FROM spouse_index")[0] == 1)
        check("у незнакомого мужа жены нет",
              db.execute(sql["spouse_lookup"], {"husband_norm": norm("Иван Петров")}).fetchone() is None)

        print("\n10. Целостность")
        check("integrity_check", one("PRAGMA integrity_check")[0] == "ok")
        check("foreign_key_check", len(db.execute("PRAGMA foreign_key_check").fetchall()) == 0)
        db.commit()
        db.close()

    print(f"\nИтог: успешно {ok_count}, ошибок {fail_count}\n")
    return 1 if fail_count else 0


if __name__ == "__main__":
    raise SystemExit(main())
