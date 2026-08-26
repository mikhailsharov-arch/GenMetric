#!/usr/bin/env python3
"""
Проверка подсказок.

Зачем отдельный файл. В сборке от 17.08 поле «НП» искало населённые пункты
в плоских перечнях lookup, где их нет и быть не может — они лежат в своей
таблице place. Поле молчало на каждой записи, четыре раза, и это оказалось
самой дорогой потерей времени во всём замере. Ошибка прожила три недели
только потому, что запрос был вшит в код на Rust, который в песочнице
не собирается, и проверить его было нечем.

Теперь запросы лежат в db/statements.sql, и приложение и этот файл читают
оттуда один и тот же текст. Собирается запрос одинаково: в блоке
suggest_ranked есть место {dict}, куда подставляется один из источников.

Что проверяется:
  1. блоки на месте и подстановка {dict} даёт исполнимый SQL;
  2. населённый пункт из поставки находится по префиксу;
  3. отчества фильтруются по полу — мужчине не предлагается женское;
  4. без указания пола показываются обе формы;
  5. имена и плоские перечни ищутся по-прежнему;
  6. набранное руками обгоняет словарь, а частое обгоняет редкое;
  7. кириллица ищется без учёта регистра и «ё».

Запуск:
    python3 db/test_suggest.py
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


def like_prefix(value) -> str:
    """Экранирование под LIKE — как like_prefix() в main.rs."""
    s = norm(value)
    for ch in ("\\", "%", "_"):
        s = s.replace(ch, "\\" + ch)
    return s + "%"


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


# Та же сборка запроса, что в suggest() в main.rs.
DICT_FOR_KIND = {
    "first_name": "suggest_first_name",
    "patronymic": "suggest_patronymic",
    "place": "suggest_place",
}


def build(sql: dict, kind: str) -> str:
    dict_block = sql[DICT_FOR_KIND.get(kind, "suggest_lookup")]
    return sql["suggest_ranked"].replace("{dict}", dict_block.strip().rstrip(";"))


def suggest(db, sql, kind, prefix, gender=None, limit=8):
    params = {"kind": kind, "prefix": like_prefix(prefix), "limit": limit}
    if kind == "patronymic":
        params["gender"] = gender
    rows = db.execute(build(sql, kind), params).fetchall()
    return [r[0] for r in rows]


def main() -> int:
    _utf8_stdout()
    sys.path.insert(0, str(DB_DIR))
    import build_seed

    sql = load_statements()

    print("\n1. Блоки подсказок на месте")
    for required in ("suggest_ranked", "suggest_first_name", "suggest_patronymic",
                     "suggest_place", "suggest_lookup"):
        check(f"блок {required}", required in sql)
    check("в suggest_ranked есть место для источника",
          "{dict}" in sql.get("suggest_ranked", ""))

    # Проверка ниже держит договорённость: подсказки собираются из этого файла,
    # а не из строк внутри кода. Если запрос вернётся в main.rs, проверка упадёт,
    # и ошибка вроде «НП ищется в lookup» больше не проживёт три недели.
    print("\n1а. Приложение берёт запросы отсюда, а не из своего кода")
    rust = (DB_DIR.parent / "src-tauri" / "src" / "main.rs").read_text(encoding="utf-8")
    body = rust.split("fn suggest(", 1)[-1].split("\n}\n", 1)[0]
    for block in ("suggest_ranked", "suggest_first_name", "suggest_patronymic",
                  "suggest_place", "suggest_lookup"):
        check(f"main.rs берёт {block} из statements.sql", f'"{block}"' in body)
    check("в suggest() не осталось своего SELECT",
          "SELECT" not in body.upper(), "запрос должен жить в statements.sql")

    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "db.sqlite"
        build_seed.build(path)
        db = sqlite3.connect(path)

        print("\n2. Запросы исполняются")
        for kind in ("first_name", "patronymic", "place", "rank_m", "confession"):
            try:
                suggest(db, sql, kind, "а")
                check(f"источник для «{kind}»", True)
            except sqlite3.Error as e:
                check(f"источник для «{kind}»", False, str(e))

        print("\n3. Населённые пункты из поставки находятся")
        places = db.execute("SELECT count(*) FROM place").fetchone()[0]
        check("справочник НП не пуст", places > 100, f"{places} пунктов")
        got = suggest(db, sql, "place", "Черте")
        check("«Черте» находит Чертеж", any("Чертеж" in v for v in got), ", ".join(got))
        check("оба Чертежа в выдаче", len([v for v in got if "Чертеж" in v]) >= 2)
        check("Борисоглебское находится по «борис»",
              "Борисоглебское" in suggest(db, sql, "place", "борис"))
        check("несуществующий пункт не выдумывается",
              suggest(db, sql, "place", "Зззз") == [])

        print("\n4. Отчества по полу")
        # «Алексеев» — мужское отчество, «Алексеева» — женское.
        m = suggest(db, sql, "patronymic", "Алексее", gender="М", limit=20)
        f = suggest(db, sql, "patronymic", "Алексее", gender="Ж", limit=20)
        both = suggest(db, sql, "patronymic", "Алексее", gender=None, limit=20)
        check("мужчине предлагается мужское", any(v.endswith("ев") or v.endswith("евич") for v in m),
              ", ".join(m[:4]))
        check("мужчине НЕ предлагается женское",
              not any(v.endswith("евна") or v.endswith("ева") for v in m), ", ".join(m[:6]))
        check("женщине предлагается женское", any(v.endswith("евна") or v.endswith("ева") for v in f),
              ", ".join(f[:4]))
        check("женщине НЕ предлагается мужское",
              not any(v.endswith("евич") for v in f), ", ".join(f[:6]))
        check("без пола показываются обе формы", len(both) >= max(len(m), len(f)),
              f"м {len(m)}, ж {len(f)}, обе {len(both)}")

        print("\n5. Имена и плоские перечни не сломались")
        check("имя «Никит» находится",
              any(v.startswith("Никит") for v in suggest(db, sql, "first_name", "Никит")))
        check("звание находится", len(suggest(db, sql, "rank_m", "крест")) > 0)
        check("вероисповедание по умолчанию есть в перечне",
              "православного" in suggest(db, sql, "confession", "правосл"))

        print("\n6. Набранное руками впереди словаря")
        db.execute(
            "INSERT INTO usage_stat (kind, scope, scope_key, value, value_norm, count, last_used_at)"
            " VALUES ('place','case','1','Чертеж Малый',?,5,datetime('now'))",
            (norm("Чертеж Малый"),))
        db.execute(
            "INSERT INTO usage_stat (kind, scope, scope_key, value, value_norm, count, last_used_at)"
            " VALUES ('place','global','','Чертеж Большой',?,99,datetime('now'))",
            (norm("Чертеж Большой"),))
        got = suggest(db, sql, "place", "Черте")
        check("значение из текущего дела первое", got[0] == "Чертеж Малый", ", ".join(got))
        check("значение из базы обгоняет словарь",
              got.index("Чертеж Большой") < len(got) - 1 or len(got) == 2, ", ".join(got))
        check("дубликатов нет", len(got) == len(set(got)), ", ".join(got))

        print("\n7. Кириллица: регистр и «ё»")
        db.execute(
            "INSERT INTO usage_stat (kind, scope, scope_key, value, value_norm, count, last_used_at)"
            " VALUES ('rank_m','global','','бобыль дер. Кнышёво',?,1,datetime('now'))",
            (norm("бобыль дер. Кнышёво"),))
        check("верхний регистр находит",
              "Борисоглебское" in suggest(db, sql, "place", "БОРИС"))
        check("«е» находит «ё»",
              any("Кнышёво" in v for v in suggest(db, sql, "rank_m", "бобыль дер. Кнышев")))

        db.close()

    print(f"\nИтого: {ok_count} ок, {fail_count} ошибок")
    return 1 if fail_count else 0


if __name__ == "__main__":
    raise SystemExit(main())
