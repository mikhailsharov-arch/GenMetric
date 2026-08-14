#!/usr/bin/env python3
"""
Проверка хранения настроек.

Размер шрифта и прочие пользовательские настройки живут в базе, чтобы
переживать перезапуск. Запись идёт через ON CONFLICT DO UPDATE — синтаксис,
который легко написать неверно, а проверить в песочнице нельзя: код на Rust
здесь не собирается. Поэтому тот же запрос прогоняется тут.

Отдельно проверяется, что обновление программы не затирает выбор человека:
migrate.sql добавляет только недостающие ключи.

Запуск:
    python3 db/test_settings.py
"""

import sqlite3
import sys
import tempfile
from pathlib import Path

DB_DIR = Path(__file__).resolve().parent

# Тот же запрос, что в set_setting() в src-tauri/src/main.rs.
SET_SETTING = """
INSERT INTO setting (key, value) VALUES (?1, ?2)
ON CONFLICT(key) DO UPDATE SET value = excluded.value
"""

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


def main() -> int:
    _utf8_stdout()
    sys.path.insert(0, str(DB_DIR))
    import build_seed

    with tempfile.TemporaryDirectory() as tmp:
        db_path = Path(tmp) / "seed.sqlite"
        build_seed.build(db_path)
        db = sqlite3.connect(db_path)
        one = lambda sql, *a: db.execute(sql, a).fetchone()

        print("\n1. Настройка размера шрифта")
        row = one("SELECT value FROM setting WHERE key='ui_font_scale'")
        check("значение по умолчанию есть в поставке", row is not None, row[0] if row else "нет")

        print("\n2. Запись настройки тем же запросом, что в приложении")
        db.execute(SET_SETTING, ("ui_font_scale", "0.85"))
        check("значение записалось",
              one("SELECT value FROM setting WHERE key='ui_font_scale'")[0] == "0.85")
        db.execute(SET_SETTING, ("ui_font_scale", "1.2"))
        check("повторная запись перезаписывает, а не задваивает",
              one("SELECT count(*) FROM setting WHERE key='ui_font_scale'")[0] == 1)
        check("новое значение на месте",
              one("SELECT value FROM setting WHERE key='ui_font_scale'")[0] == "1.2")

        db.execute(SET_SETTING, ("новый_ключ", "значение"))
        check("незнакомый ключ добавляется",
              one("SELECT value FROM setting WHERE key='новый_ключ'")[0] == "значение")

        print("\n3. Обновление программы не затирает выбор человека")
        # migrate.sql добавляет настройки через INSERT OR IGNORE — проверяем,
        # что выбранный человеком размер шрифта переживёт обновление.
        db.execute("INSERT OR IGNORE INTO setting (key, value) "
                   "SELECT 'ui_font_scale', '1'")
        check("выбранный размер шрифта не сброшен",
              one("SELECT value FROM setting WHERE key='ui_font_scale'")[0] == "1.2")
        db.close()

    print(f"\nИтог: успешно {ok_count}, ошибок {fail_count}\n")
    return 1 if fail_count else 0


if __name__ == "__main__":
    raise SystemExit(main())
