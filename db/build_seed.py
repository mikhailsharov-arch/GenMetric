#!/usr/bin/env python3
"""
Сборка стартовой базы данных из текстовых справочников.

Читает db/seed/*.csv и db/schema.sql, на выходе даёт готовый seed.sqlite.
Исходный Excel-Индексатор для этого не нужен: справочники хранятся в git
текстом, см. db/tools/extract_from_xlsm.py.

Запуск:
    python3 db/build_seed.py                 # соберёт db/seed.sqlite
    python3 db/build_seed.py путь/к/base.db  # или в указанный файл
"""

import csv
import hashlib
import re
import sqlite3
import sys
import unicodedata
from pathlib import Path

SCHEMA_VERSION = 2
DB_DIR = Path(__file__).resolve().parent
SEED_DIR = DB_DIR / "seed"


def norm(value) -> str:
    """Ключ для поиска по префиксу.

    Встроенный в SQLite COLLATE NOCASE приводит регистр только для латиницы,
    на кириллице он молча не работает. Поэтому нормализованные значения
    готовит приложение. Эта функция обязана совпадать с normalize() в коде
    интерфейса и с norm() в extract_from_xlsm.py.
    """
    if value is None:
        return ""
    s = unicodedata.normalize("NFC", str(value)).strip().lower().replace("ё", "е")
    return re.sub(r"\s+", " ", s)


def nz(value):
    """Пустая строка в CSV означает NULL в базе."""
    if value is None:
        return None
    v = str(value).strip()
    return v or None


def seed_stamp() -> str:
    """Отпечаток поставки: меняется при любой правке схемы или справочников.

    По нему программа понимает, что база пользователя отстала от установленной
    версии, и подмешивает свежие справочники. Без отпечатка обновления просто
    не доезжают: база копируется только при первой установке.
    """
    h = hashlib.sha256()
    for path in sorted(SEED_DIR.glob("*.csv")) + [DB_DIR / "schema.sql", DB_DIR / "migrate.sql"]:
        if path.exists():
            h.update(path.read_bytes())
    return h.hexdigest()[:12]


def read_csv(name: str):
    path = SEED_DIR / name
    if not path.exists():
        raise SystemExit(f"Не найден файл справочника: {path}")
    with path.open(encoding="utf-8", newline="") as f:
        return list(csv.DictReader(f))


def build(db_path: Path) -> dict:
    # Папку создаём сами: git не хранит пустые каталоги, поэтому в свежем
    # клоне src-tauri/resources/ может отсутствовать.
    db_path.parent.mkdir(parents=True, exist_ok=True)
    if db_path.exists():
        db_path.unlink()
    for suffix in ("-wal", "-shm"):
        stale = db_path.with_name(db_path.name + suffix)
        if stale.exists():
            stale.unlink()

    db = sqlite3.connect(db_path)
    db.executescript((DB_DIR / "schema.sql").read_text(encoding="utf-8"))
    db.execute("INSERT INTO schema_version (version) VALUES (?)", (SCHEMA_VERSION,))
    stats = {}

    rows = read_csv("name_dict.csv")
    db.executemany(
        """INSERT INTO name_dict
           (gender, name, name_norm, variant, base_name, usage_note, declension,
            genitive, patr_old_m, patr_old_f, patr_m, patr_f, source)
           VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)""",
        [(nz(r["gender"]), r["name"].strip(), norm(r["name"]), nz(r["variant"]),
          nz(r["base_name"]), nz(r["usage_note"]), nz(r["declension"]), nz(r["genitive"]),
          nz(r["patr_old_m"]), nz(r["patr_old_f"]), nz(r["patr_m"]), nz(r["patr_f"]),
          nz(r["source"])) for r in rows])
    stats["name_dict"] = len(rows)

    # Все написания одной таблицей: по ним идут разбор строки ИОФ и пословные
    # подсказки. Заголовочное написание имеет приоритет над вариантом — иначе
    # «Никита», который есть и сам по себе, и как вариант «Аникиты»,
    # превращался бы при разборе в «Аникиту».
    forms = []
    for i, r in enumerate(rows, start=1):
        forms.append((i, r["name"].strip(), norm(r["name"]), "name", nz(r["gender"]), 0))
        if nz(r["variant"]):
            forms.append((i, r["variant"].strip(), norm(r["variant"]), "variant", nz(r["gender"]), 1))
        for column, kind, gender in (("patr_old_m", "patr_old_m", "М"),
                                     ("patr_old_f", "patr_old_f", "Ж"),
                                     ("patr_m", "patr_m", "М"),
                                     ("patr_f", "patr_f", "Ж")):
            if nz(r[column]):
                forms.append((i, r[column].strip(), norm(r[column]), kind, gender, 0))
    db.executemany(
        "INSERT INTO name_form (name_id, form, form_norm, kind, gender, priority) VALUES (?,?,?,?,?,?)",
        forms)
    stats["name_form"] = len(forms)

    rows = read_csv("lookup_kind.csv")
    db.executemany(
        "INSERT INTO lookup_kind (kind, title, editable, autoextend) VALUES (?,?,?,?)",
        [(r["kind"], r["title"], int(r["editable"]), int(r["autoextend"])) for r in rows])
    stats["lookup_kind"] = len(rows)

    rows = read_csv("lookup.csv")
    db.executemany(
        "INSERT INTO lookup (kind, value, value_norm, sort_order, origin) VALUES (?,?,?,?,'seed')",
        [(r["kind"], r["value"].strip(), norm(r["value"]), int(r["sort_order"])) for r in rows])
    stats["lookup"] = len(rows)

    rows = read_csv("role.csv")
    db.executemany(
        "INSERT INTO role (code, title, section, sort_order, gender, age_min, age_max) VALUES (?,?,?,?,?,?,?)",
        [(r["code"], r["title"], int(r["section"]), int(r["sort_order"]), nz(r["gender"]),
          int(r["age_min"]) if nz(r["age_min"]) else None,
          int(r["age_max"]) if nz(r["age_max"]) else None) for r in rows])
    stats["role"] = len(rows)

    rows = read_csv("place.csv")
    db.executemany(
        """INSERT INTO place
           (name, name_norm, np_type, guberniya, uyezd, volost,
            short_location, full_location, familio_url, origin)
           VALUES (?,?,?,?,?,?,?,?,?,'seed')""",
        [(r["name"].strip(), norm(r["name"]), nz(r["np_type"]), nz(r["guberniya"]),
          nz(r["uyezd"]), nz(r["volost"]), nz(r["short_location"]),
          nz(r["full_location"]), nz(r["familio_url"])) for r in rows])
    stats["place"] = len(rows)

    rows = read_csv("setting.csv")
    db.executemany("INSERT INTO setting (key, value) VALUES (?,?)",
                   [(r["key"], r["value"]) for r in rows])
    db.execute("INSERT INTO setting (key, value) VALUES ('seed_stamp', ?)", (seed_stamp(),))
    stats["setting"] = len(rows) + 1

    db.commit()
    db.execute("VACUUM")
    db.close()
    return stats


def _utf8_stdout() -> None:
    """Windows запускает Python с кодировкой cp1252, и любой вывод кириллицей
    роняет скрипт с UnicodeEncodeError. Переключаем потоки на UTF-8."""
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8", errors="replace")
        except (AttributeError, ValueError):
            pass


def main() -> int:
    _utf8_stdout()
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else DB_DIR / "seed.sqlite"
    stats = build(out)
    print(f"Готово: {out}  ({out.stat().st_size / 1024:.0f} КБ)")
    for k, v in stats.items():
        print(f"  {k:<12} {v:>6}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
