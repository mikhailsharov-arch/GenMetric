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

Запуск:
    python3 db/test_upgrade.py
"""

import sqlite3
import subprocess
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
    """Схема предыдущей версии — берём из истории репозитория.

    Коммит 2c5e53d — последний, где схема была версии 1: без таблицы форм имён
    и со справочниками до пополнения.
    """
    result = subprocess.run(["git", "-C", str(REPO), "show", "2c5e53d:db/schema.sql"],
                            capture_output=True, text=True)
    if result.returncode != 0:
        raise SystemExit("Не удалось достать старую схему из git:\n" + result.stderr)
    return result.stdout


def build_old_database(path: Path) -> None:
    """База, какой она была у пользователя после первой сборки, плюс его правки."""
    db = sqlite3.connect(path)
    db.executescript(old_schema_sql())
    db.execute("INSERT INTO schema_version (version) VALUES (1)")
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
    db.execute("UPDATE setting SET value='0' WHERE key='modernize_names'") if db.execute(
        "SELECT count(*) FROM setting WHERE key='modernize_names'").fetchone()[0] else db.execute(
        "INSERT INTO setting (key, value) VALUES ('modernize_names','0')")
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

    conn.execute("INSERT INTO schema_version (version) VALUES (2)")
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
        check("схема версии 1", db.execute("SELECT max(version) FROM schema_version").fetchone()[0] == 1)
        has_forms = db.execute(
            "SELECT count(*) FROM sqlite_master WHERE name='name_form'").fetchone()[0]
        check("таблицы форм имён нет — из-за этого не работало поле ИОФ", has_forms == 0)
        check("званий всего 2", db.execute("SELECT count(*) FROM lookup WHERE kind='rank_m'").fetchone()[0] == 2)
        db.close()

        upgrade(user, seed)

        print("\n2. После обновления появилось новое из поставки")
        db = sqlite3.connect(user)
        one = lambda sql, *a: db.execute(sql, a).fetchone()[0]
        check("схема поднялась до версии 2", one("SELECT max(version) FROM schema_version") == 2)
        n_forms = one("SELECT count(*) FROM name_form")
        check("таблица форм имён заполнена", n_forms > 12000, f"{n_forms} написаний")
        check("имена перенесены", one("SELECT count(*) FROM name_dict") > 3000)
        n_rank_m = one("SELECT count(*) FROM lookup WHERE kind='rank_m'")
        check("мужских званий стало 123 плюс своё", n_rank_m == 124, f"{n_rank_m}")
        check("женские звания появились",
              one("SELECT count(*) FROM lookup WHERE kind='rank_f'") == 76)
        check("«крестьянская жена» на месте",
              one("SELECT count(*) FROM lookup WHERE value='крестьянская жена'") == 1)
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
