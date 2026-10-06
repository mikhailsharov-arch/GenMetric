#!/usr/bin/env python3
"""
Проверка приходов: общий файл и сверка справочников.

Каждый приход — свой файл SQLite; общие справочники лежат в общем файле
(db/common.sql) и сверяются с приходом в обе стороны файлом
db/parish_sync.sql (спека 2026-10-02, п. 1–2). Программа выполняет те же два
файла (src-tauri/core/src/parish.rs), поэтому логика проверена здесь, на
настоящем SQLite.

Запуск:
    python3 db/test_parish.py
"""

import shutil
import sqlite3
import sys
import tempfile
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


def open_parish(path: Path, common: Path) -> sqlite3.Connection:
    """Как parish::open(): соединение прихода с подключённым общим файлом."""
    c = sqlite3.connect(common)
    c.executescript((DB_DIR / "common.sql").read_text(encoding="utf-8"))
    c.execute("INSERT OR IGNORE INTO parish (id, file) VALUES (1, 'genmetric.sqlite')")
    c.commit()
    c.close()
    db = sqlite3.connect(path, isolation_level=None)
    db.execute("ATTACH DATABASE ? AS common", (str(common),))
    return db


def sync(db: sqlite3.Connection) -> None:
    db.executescript("BEGIN IMMEDIATE;" + (DB_DIR / "parish_sync.sql").read_text(encoding="utf-8") + "COMMIT;")


def card(name, np_type=None, uyezd=None, guberniya=None, volost=None, url=None):
    return dict(name=name, name_norm=norm(name), np_type=np_type, guberniya=guberniya,
                uyezd=uyezd, volost=volost, familio_url=url)


def main() -> int:
    _utf8_stdout()
    import build_seed

    sql = load_statements()
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        seed = tmp / "seed.sqlite"
        build_seed.build(seed)
        common = tmp / "genmetric-общее.sqlite"
        path_a, path_b = tmp / "genmetric.sqlite", tmp / "приход-б.sqlite"
        shutil.copy(seed, path_a)
        shutil.copy(seed, path_b)

        print("\n1. Общий файл и первый приход")
        a = open_parish(path_a, common)
        one_a = lambda q, *p: a.execute(q, p).fetchone()[0]  # noqa: E731
        check("прежняя база — первый приход перечня",
              a.execute("SELECT id, file FROM common.parish").fetchall() == [(1, "genmetric.sqlite")])
        seed_places = one_a("SELECT count(DISTINCT name_norm) FROM main.place")
        sync(a)
        check("пункты поставки — в общем файле, по одному на название",
              one_a("SELECT count(*) FROM common.place") == seed_places, str(seed_places))
        check("перечни поставки в общий файл не идут — только пополненное",
              one_a("SELECT count(*) FROM common.lookup") == 0)

        print("\n2. Заведённое в приходе А")
        a.execute(sql["place_save"], card("Новосёлки Дальние", "д.", "Макарьевский", "Костромская", "Нежитинская"))
        a.execute(sql["lookup_extend"], dict(kind="rank_m", value="волостной писарь", value_norm=norm("волостной писарь")))
        a.execute(sql["alias_save"], dict(kind="name", form="Пискарь", form_norm=norm("Пискарь"), target="Кесарь", gender="М"))
        a.execute("INSERT INTO person_index (iof, iof_norm, place, rank, gender) VALUES ('Пётр Сидоров', 'петр сидоров', '', '', 'М')")
        a.execute("INSERT INTO clergy_index (iof, iof_norm, rank, uses) VALUES ('Иоанн Скворцов', 'иоанн скворцов', 'священник', 3)")
        sync(a)
        check("пункт с подробностями — в общем файле",
              a.execute("SELECT np_type, volost FROM common.place WHERE name = 'Новосёлки Дальние'").fetchone()
              == ("д.", "Нежитинская"))
        check("звание — в общем файле",
              one_a("SELECT count(*) FROM common.lookup WHERE kind = 'rank_m' AND value = 'волостной писарь'") == 1)
        check("соответствие имени — в общем файле",
              one_a("SELECT target FROM common.name_alias WHERE form = 'Пискарь'") == "Кесарь")
        tables = {r[0] for r in a.execute("SELECT name FROM common.sqlite_master WHERE type = 'table'")}
        check("персон, жён, причта и записей в общем файле нет",
              tables == {"parish", "setting", "place", "place_renamed", "lookup", "name_alias"}, str(sorted(tables)))
        before = a.execute("SELECT count(*), (SELECT count(*) FROM main.lookup), (SELECT count(*) FROM common.place) FROM main.place").fetchone()
        sync(a)
        after = a.execute("SELECT count(*), (SELECT count(*) FROM main.lookup), (SELECT count(*) FROM common.place) FROM main.place").fetchone()
        check("повторная сверка ничего не меняет", before == after, f"{before} → {after}")
        a.close()

        print("\n3. Приход Б открывается — общее в нём есть, личное — нет")
        b = open_parish(path_b, common)
        one_b = lambda q, *p: b.execute(q, p).fetchone()[0]  # noqa: E731
        check("до сверки пункта в Б нет", one_b("SELECT count(*) FROM main.place WHERE name = 'Новосёлки Дальние'") == 0)
        sync(b)
        check("пункт из А есть в Б с подробностями",
              b.execute("SELECT np_type, uyezd, volost, full_location FROM main.place WHERE name = 'Новосёлки Дальние'").fetchone()
              == ("д.", "Макарьевский", "Нежитинская",
                  "д. Новосёлки Дальние, Нежитинская волость, Макарьевский уезд, Костромская губерния"))
        check("звание из А есть в Б, пополненным и в конце перечня",
              b.execute("SELECT origin, sort_order > (SELECT max(sort_order) FROM main.lookup WHERE kind = 'rank_m' AND origin = 'seed') "
                        "FROM main.lookup WHERE kind = 'rank_m' AND value = 'волостной писарь'").fetchone() == ("user", 1))
        check("соответствие имени из А есть в Б",
              b.execute(sql["alias_find"], dict(kind="name", form_norm=norm("Пискарь"))).fetchone() == ("Кесарь", "М"))
        check("персона из А в Б не подсказывается", one_b("SELECT count(*) FROM person_index") == 0)
        check("причт из А в Б не попал", one_b("SELECT count(*) FROM clergy_index") == 0)

        print("\n4. Правка карточки в Б доезжает до А")
        place_id = one_b("SELECT id FROM main.place WHERE name = 'Новосёлки Дальние'")
        b.execute("UPDATE main.place SET updated_at = '2026-10-01 10:00:00' WHERE id = ?", (place_id,))
        b.execute("UPDATE common.place SET updated_at = '2026-10-01 10:00:00' WHERE name = 'Новосёлки Дальние'")
        b.execute(sql["place_update"], dict(card("Новосёлки Дальние", "с.", "Макарьевский", "Костромская",
                                                 "Нежитинская", "https://familio.org/place/1"), id=place_id))
        sync(b)
        # В Б человек набрал пункт без карточки (place_insert) — пустое.
        b.execute(sql["place_insert"], dict(name="Пустошка Дальняя", name_norm=norm("Пустошка Дальняя")))
        sync(b)
        b.close()
        a = open_parish(path_a, common)
        one_a = lambda q, *p: a.execute(q, p).fetchone()[0]  # noqa: E731
        a.execute(sql["place_save"], card("Пустошка Дальняя", "пуст.", "Макарьевский", "Костромская"))
        sync(a)
        check("в А — тип и ссылка, исправленные в Б",
              a.execute("SELECT np_type, familio_url, short_location FROM main.place WHERE name = 'Новосёлки Дальние'").fetchone()
              == ("с.", "https://familio.org/place/1", "с. Новосёлки Дальние"))
        check("строка пункта в А одна — правка не плодит двойника",
              one_a("SELECT count(*) FROM main.place WHERE name_norm = ?", norm("Новосёлки Дальние")) == 1)
        check("заполненная карточка А легла в общий файл поверх пустой из Б",
              one_a("SELECT np_type FROM common.place WHERE name = 'Пустошка Дальняя'") == "пуст.")

        print("\n5. Пустое не затирает заполненное; из двух заполненных — более позднее")
        a.close()
        b = open_parish(path_b, common)
        one_b = lambda q, *p: b.execute(q, p).fetchone()[0]  # noqa: E731
        sync(b)
        check("пустая строка Б получила подробности из А",
              b.execute("SELECT np_type, uyezd FROM main.place WHERE name = 'Пустошка Дальняя'").fetchone()
              == ("пуст.", "Макарьевский"))
        check("в общем файле карточка осталась заполненной",
              one_b("SELECT np_type FROM common.place WHERE name = 'Пустошка Дальняя'") == "пуст.")
        # Старая правка в Б против свежей общей: побеждает общая.
        pid = one_b("SELECT id FROM main.place WHERE name = 'Пустошка Дальняя'")
        b.execute("UPDATE main.place SET np_type = 'поч.', updated_at = '2020-01-01 00:00:00' WHERE id = ?", (pid,))
        sync(b)
        check("более ранняя правка не побеждает позднюю",
              (one_b("SELECT np_type FROM main.place WHERE id = ?", pid),
               one_b("SELECT np_type FROM common.place WHERE name = 'Пустошка Дальняя'")) == ("пуст.", "пуст."))
        # Правка карточки пункта поставки (updated_at был пуст).
        seed_name, seed_id = b.execute(
            "SELECT name, id FROM main.place WHERE origin <> 'user' AND trim(coalesce(np_type, '')) <> '' LIMIT 1").fetchone()
        row = b.execute("SELECT guberniya, uyezd, volost FROM main.place WHERE id = ?", (seed_id,)).fetchone()
        b.execute(sql["place_update"], dict(card(seed_name, "выселок", row[1], row[0], row[2]), id=seed_id))
        sync(b)
        check("правка пункта поставки — в общем файле",
              one_b("SELECT np_type FROM common.place WHERE name_norm = ?", norm(seed_name)) == "выселок", seed_name)

        print("\n6. Переименование")
        b.execute(sql["place_update"], dict(card("Пустошь Дальняя", "пуст.", "Макарьевский", "Костромская"), id=pid))
        b.execute(sql["place_renamed_remember"], dict(old_norm=norm("Пустошка Дальняя")))
        # Время — явно: тест идёт быстрее миллисекунды, а порядок здесь важен
        # (карточки заведены раньше переименования).
        b.execute("UPDATE common.place SET updated_at = '2026-10-01 11:00:00.000' WHERE name = 'Пустошка Дальняя'")
        b.execute("UPDATE main.place_renamed SET renamed_at = '2026-10-01 12:00:00.000'")
        sync(b)
        check("в общем файле — новое название, старого нет",
              [r[0] for r in b.execute("SELECT name FROM common.place WHERE name LIKE 'Пустош%' ORDER BY name")]
              == ["Пустошь Дальняя"])
        b.close()
        a = open_parish(path_a, common)
        a.execute("UPDATE main.place SET updated_at = '2026-10-01 11:00:00.000' WHERE name = 'Пустошка Дальняя'")
        sync(a)
        names = [r[0] for r in a.execute("SELECT name FROM main.place WHERE name LIKE 'Пустош%' ORDER BY name")]
        check("в А пришло новое название; своё старое осталось (на него ссылаются записи)",
              names == ["Пустошка Дальняя", "Пустошь Дальняя"], str(names))
        sync(a)
        check("старое название из А в общий файл не возвращается",
              a.execute("SELECT count(*) FROM common.place WHERE name = 'Пустошка Дальняя'").fetchone()[0] == 0)
        check("пункт поставки, исправленный в Б, исправлен и в А",
              a.execute("SELECT np_type FROM main.place WHERE name_norm = ? ORDER BY id LIMIT 1",
                        (norm(seed_name),)).fetchone()[0] == "выселок")

        print("\n6а. Переименовал и вернул название; правка в ту же секунду")
        # В А пункт «Пустошка Дальняя» ещё под старым названием. Человек в Б
        # возвращает прежнее название — пункт снова обычный и общий.
        a.close()
        b = open_parish(path_b, common)
        b.execute(sql["place_update"], dict(card("Пустошка Дальняя", "пуст.", "Макарьевский", "Костромская"), id=pid))
        b.execute(sql["place_renamed_remember"], dict(old_norm=norm("Пустошь Дальняя")))
        sync(b)
        check("возвращённое название — снова в общем файле, а не в памяти переименований",
              b.execute("SELECT count(*) FROM common.place WHERE name = 'Пустошка Дальняя'").fetchone()[0] == 1
              and b.execute("SELECT count(*) FROM common.place_renamed WHERE old_norm = ?",
                            (norm("Пустошка Дальняя"),)).fetchone()[0] == 0)
        sync(b)
        check("…и повторная сверка его оттуда не убирает",
              b.execute("SELECT count(*) FROM common.place WHERE name = 'Пустошка Дальняя'").fetchone()[0] == 1)
        # Карточка заведена и поправлена в одну секунду: правка не откатывается.
        b.execute(sql["place_save"], card("Выселок Новый", "д.", "Макарьевский", "Костромская"))
        sync(b)
        vid = b.execute("SELECT id, updated_at FROM main.place WHERE name = 'Выселок Новый'").fetchone()
        b.execute(sql["place_update"], dict(card("Выселок Новый", "с.", "Макарьевский", "Костромская", "Завражная"), id=vid[0]))
        later = b.execute("SELECT updated_at FROM main.place WHERE id = ?", (vid[0],)).fetchone()[0]
        check("время правки — с миллисекундами", len(later) == 23 and later[:19] == vid[1][:19] or later > vid[1], f"{vid[1]} → {later}")
        b.execute("UPDATE main.place SET updated_at = substr(?, 1, 20) || '999' WHERE id = ?", (vid[1], vid[0]))
        sync(b)
        check("правка в ту же секунду остаётся и в приходе, и в общем файле",
              b.execute("SELECT np_type, volost FROM main.place WHERE id = ?", (vid[0],)).fetchone() == ("с.", "Завражная")
              and b.execute("SELECT np_type FROM common.place WHERE name = 'Выселок Новый'").fetchone()[0] == "с.")
        b.close()
        a = open_parish(path_a, common)
        sync(a)
        check("в А возвращённое название перестало считаться переименованным",
              a.execute("SELECT count(*) FROM main.place_renamed WHERE old_norm = ?",
                        (norm("Пустошка Дальняя"),)).fetchone()[0] == 0)

        print("\n7. Соответствие имени: повторное решение заменяет прежнее")
        a.execute("UPDATE main.name_alias SET created_at = '2026-10-01 10:00:00' WHERE form = 'Пискарь'")
        a.execute("UPDATE common.name_alias SET created_at = '2026-10-01 10:00:00' WHERE form = 'Пискарь'")
        a.execute(sql["alias_save"], dict(kind="name", form="Пискарь", form_norm=norm("Пискарь"), target="Писарь", gender="М"))
        sync(a)
        a.close()
        b = open_parish(path_b, common)
        sync(b)
        check("новое решение из А — в Б",
              b.execute(sql["alias_find"], dict(kind="name", form_norm=norm("Пискарь"))).fetchone() == ("Писарь", "М"))
        check("дубликатов перечня нет",
              b.execute("SELECT count(*) FROM main.lookup WHERE kind = 'rank_m' AND value_norm = ?",
                        (norm("волостной писарь"),)).fetchone()[0] == 1)

        print("\n8. Настройки окна — общие")
        set_common = ("INSERT INTO common.setting (key, value) VALUES (?1, ?2) "
                      "ON CONFLICT(key) DO UPDATE SET value = excluded.value")
        b.execute(set_common, ("ui_font_scale", "125"))
        b.close()
        a = open_parish(path_a, common)
        check("масштаб, выставленный в Б, виден в А",
              a.execute("SELECT value FROM common.setting WHERE key = 'ui_font_scale'").fetchone()[0] == "125")
        check("отпечаток поставки — у прихода свой",
              a.execute("SELECT count(*) FROM common.setting WHERE key = 'seed_stamp'").fetchone()[0] == 0
              and a.execute("SELECT count(*) FROM main.setting WHERE key = 'seed_stamp'").fetchone()[0] == 1)

        print("\n9. Тестовые пункты шаблона не возвращаются из общего файла (Роман 06.10.2026)")
        a.execute("INSERT INTO common.place (name, name_norm, np_type, guberniya, uyezd, origin) "
                  "VALUES ('Лодзь', 'лодзь', 'г.', 'Петровская', 'Лодзинский', 'seed')")
        a.execute("INSERT INTO common.place (name, name_norm, np_type, guberniya, uyezd, volost, origin) "
                  "VALUES ('Котело', 'котело', 'с.', 'Костромская', 'Галический', 'Котельская', 'seed')")
        # «Котело» в этом приходе осталось (стоит в записи) — оно настоящее.
        a.execute("INSERT INTO main.place (name, name_norm, np_type, guberniya, uyezd, volost, origin) "
                  "VALUES ('Котело', 'котело', 'с.', 'Костромская', 'Галический', 'Котельская', 'seed')")
        a.commit()
        sync(a)
        check("«Лодзь» ушла из общего файла и в приход не вернулась",
              a.execute("SELECT (SELECT count(*) FROM common.place WHERE name_norm = 'лодзь') + "
                        "(SELECT count(*) FROM main.place WHERE name_norm = 'лодзь')").fetchone()[0] == 0)
        check("«Котело», оставшееся в приходе, — на месте и в общем файле",
              a.execute("SELECT (SELECT count(*) FROM common.place WHERE name_norm = 'котело') + "
                        "(SELECT count(*) FROM main.place WHERE name_norm = 'котело')").fetchone()[0] == 2)
        a.close()

    print(f"\nИтог: успешно {ok_count}, ошибок {fail_count}")
    return 1 if fail_count else 0


if __name__ == "__main__":
    raise SystemExit(main())
