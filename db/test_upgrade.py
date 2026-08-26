#!/usr/bin/env python3
"""
Проверка обновления базы при обновлении программы.

Моделируем ровно то, что случилось у тестировщика 13.08.2026: человек ставит
новую версию поверх старой, база остаётся от прошлой сборки, и половина
программы молча не работает.

Тест собирает «старую» базу по схеме предыдущей версии, добавляет в неё то,
что мог бы завести пользователь, прогоняет ту же процедуру обновления, что и
приложение, и проверяет два обязательства:

  1. всё новое из поставки в базе появилось;
  2. ничего пользовательского не потерялось.

Процедура обновления описана в db/migrate.sql — этот же файл выполняет
приложение. Порядок шагов (создание недостающих таблиц по образцу из поставки,
затем migrate.sql) повторяет upgrade() в src-tauri/src/main.rs.

Старая схема лежит слепком в db/fixtures/schema-v1.sql. Из истории git её
брать нельзя: сборка клонирует репозиторий без истории.

Запуск:
    python3 db/test_upgrade.py
"""

import sqlite3
import sys
import tempfile
from pathlib import Path

DB_DIR = Path(__file__).resolve().parent
REPO = DB_DIR.parent

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


def old_schema_sql() -> str:
    """Схема предыдущей версии — из слепка в db/fixtures.

    Именно из файла, а не из истории git: сборка клонирует репозиторий без
    истории, и обращение к старому коммиту там падает.
    """
    path = DB_DIR / "fixtures" / "schema-v2.sql"
    if not path.exists():
        raise SystemExit(f"Не найден слепок старой схемы: {path}")
    return path.read_text(encoding="utf-8")


def build_old_database(path: Path) -> None:
    """База, какой она была у пользователя после первой сборки, плюс его правки."""
    db = sqlite3.connect(path)
    db.executescript(old_schema_sql())
    db.execute("INSERT INTO schema_version (version) VALUES (2)")
    db.execute("INSERT INTO lookup_kind (kind, title, editable, autoextend) VALUES ('rank_m','Звания мужские',1,1)")
    db.executemany(
        "INSERT INTO lookup (kind, value, value_norm, sort_order, origin) VALUES (?,?,?,?,?)",
        [("rank_m", "крестьянин", "крестьянин", 10, "seed"),
         # значение, которого нет в поставке: человек завёл его сам
         ("rank_m", "мещанин города Юрьевца", "мещанин города юрьевца", 9999, "user")])
    # накопленная статистика подсказок и заведённое дело — их терять нельзя
    db.execute("INSERT INTO mk_case (id, church, village, parish_key, year) "
               "VALUES (1,'Христорождественская','Борисоглебское','приход-1',1893)")
    db.execute("INSERT INTO usage_stat (kind, scope, scope_key, value, value_norm, count) "
               "VALUES ('rank_m','case','1','крестьянин','крестьянин',42)")
    db.execute("INSERT INTO setting (key, value) VALUES ('modernize_names','0')")
    # Набранные записи — их потерять нельзя ни при каких обстоятельствах.
    # Роль нужна из-за внешнего ключа: у человека справочник ролей заполнен.
    db.execute("INSERT INTO role (code, title, section, sort_order) VALUES ('child','ребенок',1,10)")
    db.execute("INSERT INTO entry (id, case_id, section, page, event_day, event_month, event_year) "
               "VALUES (1, 1, 1, '957', 6, 12, 1896)")
    db.execute("INSERT INTO person_mention (entry_id, role_code, sort_order, first_name) "
               "VALUES (1, 'child', 10, 'Евграф')")
    db.commit()
    db.close()


def upgrade(user_db: Path, seed_db: Path) -> None:
    """Та же последовательность, что выполняет приложение при запуске."""
    conn = sqlite3.connect(user_db, isolation_level=None)
    conn.execute("PRAGMA foreign_keys = OFF")
    conn.execute("ATTACH DATABASE ? AS seed", (str(seed_db),))

    # 1. Создаём недостающие таблицы и индексы по образцу из поставки.
    #    Так закрываются миграции вида «добавилась таблица». Изменение состава
    #    колонок в существующей таблице этим способом НЕ покрывается — такие
    #    правки придётся писать отдельным шагом и отдельным тестом.
    items = conn.execute(
        "SELECT name, sql FROM seed.sqlite_master "
        "WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%'").fetchall()
    for name, sql in items:
        exists = conn.execute(
            "SELECT count(*) FROM main.sqlite_master WHERE name = ?", (name,)).fetchone()[0]
        if not exists:
            conn.executescript(sql)

    # 2. Обновляем справочники.
    conn.executescript((DB_DIR / "migrate.sql").read_text(encoding="utf-8"))

    conn.execute("INSERT INTO schema_version (version) VALUES (3)")
    conn.execute("DETACH DATABASE seed")
    conn.execute("PRAGMA foreign_keys = ON")
    conn.close()


def main() -> int:
    _utf8_stdout()
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        seed = tmp / "seed.sqlite"
        user = tmp / "user.sqlite"

        sys.path.insert(0, str(DB_DIR))
        import build_seed
        build_seed.build(seed)
        build_old_database(user)

        print("\n1. База до обновления — такая же, как была у тестировщика")
        db = sqlite3.connect(user)
        check("схема версии 2", db.execute("SELECT max(version) FROM schema_version").fetchone()[0] == 2)
        has_persons = db.execute(
            "SELECT count(*) FROM sqlite_master WHERE name='person_index'").fetchone()[0]
        check("памяти о персонах ещё нет", has_persons == 0)
        check("у человека есть набранная запись",
              db.execute("SELECT count(*) FROM entry").fetchone()[0] == 1)
        db.close()

        upgrade(user, seed)

        print("\n2. После обновления появилось новое из поставки")
        db = sqlite3.connect(user)
        one = lambda sql, *a: db.execute(sql, a).fetchone()[0]
        check("схема поднялась до версии 3", one("SELECT max(version) FROM schema_version") == 3)
        check("появилась память о персонах",
              one("SELECT count(*) FROM sqlite_master WHERE name='person_index'") == 1)
        check("появилась память о супругах",
              one("SELECT count(*) FROM sqlite_master WHERE name='spouse_index'") == 1)
        n_forms = one("SELECT count(*) FROM name_form")
        check("таблица форм имён заполнена", n_forms > 12000, f"{n_forms} написаний")
        check("имена перенесены", one("SELECT count(*) FROM name_dict") > 3000)
        n_rank_m = one("SELECT count(*) FROM lookup WHERE kind='rank_m'")
        check("мужских званий стало 123 плюс своё", n_rank_m == 124, f"{n_rank_m}")
        check("женские звания появились",
              one("SELECT count(*) FROM lookup WHERE kind='rank_f'") == 76)
        check("«крестьянская жена» на месте",
              one("SELECT count(*) FROM lookup WHERE value='крестьянская жена'") == 1)
        # Справочник населённых пунктов доезжает до уже установленной программы.
        # Ровно этой проверки не хватало в августе: у Романа не появилась новая
        # таблица, и половина сборки молча не работала. НП — тот же случай:
        # без них поле подсказывать нечем, а поставку человек не пересоздаёт.
        n_place = one("SELECT count(*) FROM place")
        check("справочник НП доехал до пользователя", n_place > 100, f"{n_place} пунктов")
        check("Борисоглебское на месте",
              one("SELECT count(*) FROM place WHERE name='Борисоглебское'") == 1)
        check("разбор ИОФ заработает: «Никита» не подменяется",
              db.execute("SELECT d.name FROM name_form f JOIN name_dict d ON d.id=f.name_id "
                         "WHERE f.kind IN ('name','variant') AND f.form_norm='никита' "
                         "ORDER BY f.priority LIMIT 1").fetchone()[0] == "Никита")

        print("\n3. После обновления ничего пользовательского не потерялось")
        check("значение, заведённое человеком, на месте",
              one("SELECT count(*) FROM lookup WHERE value='мещанин города Юрьевца'") == 1)
        check("оно не задвоилось",
              one("SELECT count(*) FROM lookup WHERE value='крестьянин' AND kind='rank_m'") == 1)
        check("накопленная статистика подсказок цела",
              one("SELECT count FROM usage_stat WHERE value='крестьянин'") == 42)
        check("заведённое дело на месте", one("SELECT count(*) FROM mk_case") == 1)
        check("набранная запись на месте", one("SELECT count(*) FROM entry") == 1)
        check("персона записи на месте",
              one("SELECT first_name FROM person_mention WHERE entry_id=1") == "Евграф")
        check("настройка пользователя не перезаписана",
              one("SELECT value FROM setting WHERE key='modernize_names'") == "0")

        print("\n4. Целостность и повторный запуск")
        check("integrity_check", one("PRAGMA integrity_check") == "ok")
        check("foreign_key_check", len(db.execute("PRAGMA foreign_key_check").fetchall()) == 0)
        stamp_user = one("SELECT value FROM setting WHERE key='seed_stamp'")
        seed_db = sqlite3.connect(seed)
        stamp_seed = seed_db.execute("SELECT value FROM setting WHERE key='seed_stamp'").fetchone()[0]
        seed_db.close()
        check("отпечаток поставки записан — повторно обновляться не будет",
              stamp_user == stamp_seed, stamp_user)
        db.close()

        # обновление должно быть безопасно применять дважды
        upgrade(user, seed)
        db = sqlite3.connect(user)
        check("повторное обновление ничего не сломало",
              db.execute("SELECT count(*) FROM lookup WHERE value='мещанин города Юрьевца'").fetchone()[0] == 1)
        check("и не задвоило имена",
              db.execute("SELECT count(*) FROM name_dict").fetchone()[0] == 3112)
        db.close()

    print(f"\nИтог: успешно {ok_count}, ошибок {fail_count}\n")
    return 1 if fail_count else 0


if __name__ == "__main__":
    raise SystemExit(main())
