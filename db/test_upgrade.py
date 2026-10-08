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

# С какой версии поднимаемся. Ровно та, что стоит сейчас у Романа: он ставит
# каждую сборку, поэтому проверять надо переход с предыдущей, а не с самой
# первой. Слепки схем лежат в db/fixtures.
FROM_VERSION = 6
TO_VERSION = 10

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
    path = DB_DIR / "fixtures" / f"schema-v{FROM_VERSION}.sql"
    if not path.exists():
        raise SystemExit(f"Не найден слепок старой схемы: {path}")
    return path.read_text(encoding="utf-8")


def build_old_database(path: Path) -> None:
    """База, какой она была у пользователя после первой сборки, плюс его правки."""
    db = sqlite3.connect(path)
    db.executescript(old_schema_sql())
    db.execute("INSERT INTO schema_version (version) VALUES (?)", (FROM_VERSION,))
    db.execute("INSERT INTO lookup_kind (kind, title, editable, autoextend) VALUES ('rank_m','Звания мужские',1,1)")
    db.executemany(
        "INSERT INTO lookup (kind, value, value_norm, sort_order, origin) VALUES (?,?,?,?,?)",
        [("rank_m", "крестьянин", "крестьянин", 10, "seed"),
         # значение, которого нет в поставке: человек завёл его сам
         ("rank_m", "мещанин города Юрьевца", "мещанин города юрьевца", 9999, "user"),
         # Уборка перечней 08.10.2026 (ответ Романа 6Б): значения, которых нет в
         # новой поставке. Набранное человеком и нигде не стоящее — уйдёт;
         # значение прежней поставки, не стоящее нигде, — уйдёт; оно же,
         # стоящее в записи, — останется.
         ("rank_m", "отставной канонир", "отставной канонир", 9998, "user"),
         ("rank_m", "уволенный в запас армии бомбардир наводчик",
          "уволенный в запас армии бомбардир наводчик", 9997, "seed"),
         ("rank_m", "отставной унтер-офицер", "отставной унтер-офицер", 9996, "seed"),
         ("death_cause", "древность", "древность", 9995, "seed"),
         ("death_cause", "апоплексия", "апоплексия", 9994, "seed"),
         # В записи это звание стоит в дореформенном написании («…ъ»), а в
         # перечень сохранение кладёт современное — оно занято (ревьюер 08.10.2026).
         ("rank_m", "безземельный крестьянин", "безземельный крестьянин", 9993, "user"),
         # Две записи, попавшие в чужой перечень прошлой поставкой. У Романа
         # они в базе есть, и просто убрать их из поставки недостаточно.
         ("rank_f", "крестьянский сын", "крестьянский сын", 470, "seed"),
         ("rank_m", "крестьянская вдова после 1-го брака",
          "крестьянская вдова после 1-го брака", 1070, "seed")])
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
    # Звания и причина смерти, стоящие в записи: в перечне они останутся. Одно
    # набрано с заглавной буквы — сравнение идёт по ключу, не по написанию.
    db.execute("INSERT INTO person_mention (entry_id, role_code, sort_order, first_name, rank) "
               "VALUES (1, 'child', 11, 'Занятый', 'Мещанин города Юрьевца')")
    db.execute("INSERT INTO person_mention (entry_id, role_code, sort_order, first_name, rank, death_cause) "
               "VALUES (1, 'child', 12, 'Занятая', 'отставной унтер-офицер', 'древность')")
    db.execute("INSERT INTO person_mention (entry_id, role_code, sort_order, first_name, rank) "
               "VALUES (1, 'child', 13, 'Старинный', 'безземельный крестьянинъ')")
    db.execute("INSERT INTO usage_stat (kind, scope, scope_key, value, value_norm, count) "
               "VALUES ('rank_m','global','','отставной канонир','отставной канонир',7)")
    db.execute("INSERT INTO usage_stat (kind, scope, scope_key, value, value_norm, count) "
               "VALUES ('rank_m','global','','отставной унтер-офицер','отставной унтер-офицер',5)")
    # Запись другого года в том же деле: до 02.10.2026 дело было одно на всю
    # базу, а реквизиты — у каждого года свои. Обновление заводит году своё дело.
    db.execute("INSERT INTO entry (id, case_id, section, page, event_day, event_month, event_year, "
               "rite_day, rite_month, rite_year) VALUES (90, 1, 3, '1001', 2, 3, 1897, 4, 3, 1897)")
    # Записи до 13.09.2026: счёт всегда в мужской колонке. Четыре случая —
    # девочка (чинить), мальчик (не трогать), девочка уже с женским номером
    # (не трогать), ребёнок без пола (не трогать: угадывать нельзя).
    db.executemany(
        "INSERT INTO entry (id, case_id, section, page, no_male, no_female) VALUES (?,1,1,'958',?,?)",
        [(2, 3, None), (3, 4, None), (4, None, 5), (5, 6, None)])
    db.executemany(
        "INSERT INTO person_mention (entry_id, role_code, sort_order, first_name, gender) VALUES (?,'child',10,?,?)",
        [(2, "Татьяна", "Ж"), (3, "Иван", "М"), (4, "Мария", "Ж"), (5, "Зурбаган", None)])
    # Причт без имени (сборки 21–22.09 после перезапуска) — в двух записях.
    db.execute("INSERT INTO role (code, title, section, sort_order) VALUES ('clergy1','церковнослужитель 1',0,100)")
    db.executemany(
        "INSERT INTO person_mention (entry_id, role_code, sort_order, first_name, surname, rank) VALUES (?,'clergy1',100,?,?,?)",
        [(2, None, None, "священник"), (3, None, None, "псаломщик"), (4, "Александр", "Рождественский", "священник")])
    # Пункт поставки, поправленный человеком в карточке (24.09.2026): другой
    # уезд — часть UNIQUE, и прежний INSERT OR IGNORE завёл бы второй.
    db.execute("INSERT INTO place (name, name_norm, np_type, guberniya, uyezd, volost, origin) "
               "VALUES ('Бухарино', 'бухарино', 'с.', 'Костромская', 'Кинешемский', 'Завражная', 'seed')")
    # Пункт поставки, переименованный человеком (правка названия, 25.09.2026):
    # узнаётся по ссылке Familio, «Кнышево» заново не заводится.
    db.execute("INSERT INTO place (name, name_norm, np_type, familio_url, origin) "
               "VALUES ('Кнышевка', 'кнышевка', 'д.', "
               "'https://familio.org/settlements/4a669185-0095-494a-8373-71cd6470c0d6', 'seed')")
    # Переименован и ссылку стёр — узнаётся по памяти переименований
    # (ревьюер 25.09.2026). В схеме 5 этой таблицы нет — заводим, как
    # сделала бы сборка #35 при правке до обновления.
    db.execute("CREATE TABLE IF NOT EXISTS place_renamed (old_norm TEXT PRIMARY KEY, "
               "renamed_at TEXT NOT NULL DEFAULT (datetime('now')))")
    db.execute("INSERT INTO place (name, name_norm, np_type, origin) VALUES ('Логинцево-2', 'логинцево-2', 'д.', 'seed')")
    db.execute("INSERT INTO place_renamed (old_norm) VALUES ('логинцево')")
    # Пункт из архива Excel — одно название, без подробностей (Роман 01.10.2026:
    # в выгрузке Familio пусты тип, уезд, ссылка). Подробности есть в поставке.
    db.execute("INSERT INTO place (name, name_norm, origin) VALUES ('Чертеж Малый', 'чертеж малый', 'archive')")
    # То же, но человек набрал полное место руками и поставил пробел в уезд —
    # первое не затирается, второе считается пустым.
    db.execute("INSERT INTO place (name, name_norm, full_location, origin) "
               "VALUES ('Поселихино', 'поселихино', 'мой текст', 'user')")
    db.execute("INSERT INTO place (name, name_norm, uyezd, origin) VALUES ('Логинцево Малое', 'логинцево малое', ' ', 'archive')")
    # Двойники (03.10.2026, база Романа): пустая строка рядом с пунктом
    # поставки, на неё ссылается упоминание; и тёзка из другого уезда — он
    # не двойник и должен остаться.
    for pid, nm in ((9003, "Борисоглебское"), (9004, "Малово")):
        db.execute("INSERT INTO place (id, name, name_norm, np_type, guberniya, uyezd, volost, full_location, origin) "
                   "VALUES (?, ?, ?, ?, 'Костромская', 'Макарьевский', 'Завражная', ?, 'seed')",
                   (pid, nm, nm.lower(), "с." if pid == 9003 else "д.", f"{nm}, Завражная волость, Макарьевский уезд"))
    db.execute("INSERT INTO place (id, name, name_norm, origin) VALUES (9001, 'Борисоглебское', 'борисоглебское', 'user')")
    db.execute("INSERT INTO place (id, name, name_norm, np_type, uyezd, origin) "
               "VALUES (9002, 'Малово', 'малово', 'д.', 'Нерехтский', 'user')")
    db.execute("INSERT INTO person_mention (entry_id, role_code, sort_order, first_name, place_id) "
               "VALUES (1, 'clergy1', 130, 'Двойник', 9001)")
    db.execute("INSERT INTO person_mention (entry_id, role_code, sort_order, first_name, place_id) "
               "VALUES (1, 'clergy1', 140, 'Тёзка', 9002)")
    # 06.10.2026: пустая строка с МЕНЬШИМ номером рядом с подробной — раньше
    # она оставалась «лучшей», и двойники не сливались. И тёзки: при двух
    # разных подробных строках пустая не сливается ни в одну.
    db.execute("INSERT INTO place (id, name, name_norm, origin) VALUES (9010, 'Пустошка Тестовая', 'пустошка тестовая', 'user')")
    db.execute("INSERT INTO place (id, name, name_norm, np_type, uyezd, origin) "
               "VALUES (9011, 'Пустошка Тестовая', 'пустошка тестовая', 'д.', 'Макарьевский', 'user')")
    db.execute("INSERT INTO place (id, name, name_norm, origin) VALUES (9020, 'Хмельничное Тестовое', 'хмельничное тестовое', 'user')")
    for pid, uyezd in ((9021, "Макарьевский"), (9022, "Кологривский")):
        db.execute("INSERT INTO place (id, name, name_norm, np_type, uyezd, origin) "
                   "VALUES (?, 'Хмельничное Тестовое', 'хмельничное тестовое', 'д.', ?, 'user')", (pid, uyezd))
    # Пустая строка с набранным руками полным местом — не сливается: набранное не стирается.
    db.execute("INSERT INTO place (id, name, name_norm, full_location, origin) "
               "VALUES (9030, 'Займище Тестовое', 'займище тестовое', 'за рекой, мой текст', 'user')")
    db.execute("INSERT INTO place (id, name, name_norm, np_type, uyezd, origin) "
               "VALUES (9031, 'Займище Тестовое', 'займище тестовое', 'д.', 'Макарьевский', 'user')")
    for order, (who, pid) in enumerate((("Пустой", 9010), ("Ничей", 9020), ("Заречный", 9030))):
        db.execute("INSERT INTO person_mention (entry_id, role_code, sort_order, first_name, place_id) "
                   "VALUES (1, 'clergy1', ?, ?, ?)", (150 + order, who, pid))
    # Тестовые строки шаблона Excel из прежней поставки (Роман 06.10.2026):
    # «Лодзь» никем не занята и уйдёт; «Котело» стоит в записи — останется, и
    # его уезд с волостью в перечнях тоже; губерния «Петровская» — уйдёт.
    db.execute("INSERT INTO place (id, name, name_norm, np_type, guberniya, uyezd, origin) "
               "VALUES (9040, 'Лодзь', 'лодзь', 'г.', 'Петровская', 'Лодзинский', 'seed')")
    db.execute("INSERT INTO place (id, name, name_norm, np_type, guberniya, uyezd, volost, origin) "
               "VALUES (9041, 'Котело', 'котело', 'с.', 'Костромская', 'Галический', 'Котельская', 'seed')")
    db.execute("INSERT INTO person_mention (entry_id, role_code, sort_order, first_name, place_id) "
               "VALUES (1, 'clergy1', 160, 'Котельский', 9041)")
    db.execute("INSERT OR IGNORE INTO lookup_kind (kind, title, editable, autoextend) VALUES ('guberniya','Губернии',1,1)")
    db.execute("INSERT OR IGNORE INTO lookup_kind (kind, title, editable, autoextend) VALUES ('uyezd','Уезды',1,1)")
    db.executemany("INSERT OR IGNORE INTO lookup (kind, value, value_norm, sort_order, origin) VALUES (?,?,?,?,?)",
                   [("guberniya", "Петровская", "петровская", 900, "seed"),
                    ("uyezd", "Лодзинский", "лодзинский", 900, "seed"),
                    ("uyezd", "Галический", "галический", 910, "seed"),
                    # То же слово, но заведённое человеком, — не трогается.
                    ("uyezd", "Петровский", "петровский", 920, "user")])
    # Память персон с дублями (техдолг В6, 27.09.2026): без места персона
    # задваивалась при каждом сохранении — NULL в UNIQUE не равен NULL.
    db.executemany(
        "INSERT INTO person_index (iof, iof_norm, place, rank, gender, uses) VALUES (?,?,?,?,?,?)",
        [("Иван Петров", "иван петров", None, "крестьянин", "М", 2),
         ("Иван Петров", "иван петров", None, "крестьянин", None, 3),
         ("Иван Петров", "иван петров", "", "крестьянин", None, 1),
         ("Пётр Сидоров", "петр сидоров", "Кнышевка", None, "М", 4)])
    # И память причта: без звания — дубль на каждой записи (ревьюер 27.09.2026).
    db.executemany("INSERT INTO clergy_index (iof, iof_norm, rank, uses) VALUES (?,?,?,?)",
                   [("Иоанн Скворцов", "иоанн скворцов", None, 1)] * 3)
    db.commit()
    db.close()


def upgrade(user_db: Path, seed_db: Path) -> None:
    """Та же последовательность, что выполняет приложение при запуске."""
    conn = sqlite3.connect(user_db, isolation_level=None)
    conn.execute("PRAGMA foreign_keys = OFF")
    conn.execute("ATTACH DATABASE ? AS seed", (str(seed_db),))

    # 0. Сначала недостающие колонки в существующих таблицах (схема 6: kinship, 7: age_text, age_weeks),
    #    потом (1) недостающие таблицы и индексы — как upgrade() в main.rs.
    for (table,) in conn.execute(
            "SELECT name FROM seed.sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'").fetchall():
        have = [r[1] for r in conn.execute(f'PRAGMA main.table_info("{table}")')]
        if not have:
            continue
        for r in conn.execute(f'PRAGMA seed.table_info("{table}")').fetchall():
            if r[1] not in have:
                conn.execute(f'ALTER TABLE main."{table}" ADD COLUMN "{r[1]}" {r[2]}')
    # 1. Недостающие таблицы и индексы по образцу из поставки.

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

    conn.execute("INSERT INTO schema_version (version) VALUES (?)", (TO_VERSION,))
    conn.execute("DETACH DATABASE seed")
    conn.execute("PRAGMA foreign_keys = ON")
    conn.close()


def snapshot(db) -> list:
    """Набранное: записи и упоминания с названием пункта (не номером — слияние
    двойников пунктов номер меняет законно). Обновление обязано оставить это
    как было; исключение — починка счёта девочек 21.09.2026, она в снимок не
    входит."""
    return db.execute(
        "SELECT e.id, e.section, e.page, e.event_day, e.event_month, e.event_year, e.note, "
        "       m.role_code, m.sort_order, m.first_name, m.patronymic, m.surname, m.gender, m.rank, "
        "       m.confession, m.note, (SELECT p.name FROM place p WHERE p.id = m.place_id) "
        "  FROM entry e LEFT JOIN person_mention m ON m.entry_id = e.id ORDER BY e.id, m.id").fetchall()


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
        check(f"схема версии {FROM_VERSION}",
              db.execute("SELECT max(version) FROM schema_version").fetchone()[0] == FROM_VERSION)
        # Со схемы 6 (сборка #35) до 7 (27.09.2026, смерти) новые колонки
        # person_mention.age_text и age_weeks — их создаёт шаг «недостающие колонки».
        cols = [r[1] for r in db.execute("PRAGMA table_info(person_mention)")]
        check("колонок возраста текстом и неделями ещё нет",
              "age_text" not in cols and "age_weeks" not in cols)
        check("звания в чужих перечнях у него есть",
              db.execute("SELECT count(*) FROM lookup WHERE "
                         "(kind='rank_f' AND value='крестьянский сын') OR "
                         "(kind='rank_m' AND value='крестьянская вдова после 1-го брака')"
                         ).fetchone()[0] == 2)
        check("у человека есть набранная запись",
              db.execute("SELECT count(*) FROM entry").fetchone()[0] == 6)
        before = snapshot(db)
        db.close()

        upgrade(user, seed)

        print("\n2. После обновления появилось новое из поставки")
        db = sqlite3.connect(user)
        one = lambda sql, *a: db.execute(sql, a).fetchone()[0]
        check(f"схема поднялась до версии {TO_VERSION}", one("SELECT max(version) FROM schema_version") == TO_VERSION)
        check("появилась память о персонах",
              one("SELECT count(*) FROM sqlite_master WHERE name='person_index'") == 1)
        check("появилась память о супругах",
              one("SELECT count(*) FROM sqlite_master WHERE name='spouse_index'") == 1)
        # Новое в версии 4: причт, который можно выбрать списком, а не набирать.
        check("появилась память о причте",
              one("SELECT count(*) FROM sqlite_master WHERE name='clergy_index'") == 1)
        # Новое в версии 5: соответствия имён из окна сверки (23.09.2026).
        # Своя таблица — обновление её не трогает, в отличие от name_form.
        cols = [r[1] for r in db.execute("PRAGMA table_info(person_mention)")]
        check("колонка «родство» (kinship) в person_mention", "kinship" in cols)
        # В6: дубли памяти персон слиты, частоты сложены, пустое — пустой строкой.
        check("дубли персоны без места слиты в одну строку",
              one("SELECT count(*) FROM person_index WHERE iof='Иван Петров'") == 1)
        check("частоты дублей сложены (2+3+1)",
              one("SELECT uses FROM person_index WHERE iof='Иван Петров'") == 6)
        check("пол взят у дубля, где он был",
              one("SELECT gender FROM person_index WHERE iof='Иван Петров'") == "М")
        check("в памяти персон не осталось NULL в месте и звании",
              one("SELECT count(*) FROM person_index WHERE place IS NULL OR rank IS NULL") == 0)
        check("дубли причта без звания слиты, частоты сложены",
              one("SELECT count(*) || '/' || max(uses) FROM clergy_index WHERE iof='Иоанн Скворцов'") == "1/3")
        check("роли поручителей 5 и 6 доехали",
              one("SELECT count(*) FROM role WHERE code IN ('witness5','witness6')") == 2)
        check("появились колонки возраста умершего (age_text, age_weeks)",
              "age_text" in cols and "age_weeks" in cols)
        check("появилась таблица соответствий имён",
              one("SELECT count(*) FROM sqlite_master WHERE name='name_alias'") == 1)
        db.execute("INSERT INTO name_alias (kind, form, form_norm, target) VALUES ('name','Пискарь','пискарь','Кесарь')")
        db.commit()
        n_forms = one("SELECT count(*) FROM name_form")
        check("таблица форм имён заполнена", n_forms > 12000, f"{n_forms} написаний")
        check("имена перенесены", one("SELECT count(*) FROM name_dict") > 3000)
        n_rank_m = one("SELECT count(*) FROM lookup WHERE kind='rank_m'")
        # 51 из поставки (файл «Справочник» Романа) плюс два занятых в записи:
        # своё «мещанин города Юрьевца» и «отставной унтер-офицер» прежней поставки.
        check("мужских званий: 51 из поставки и три занятых в записи", n_rank_m == 54, f"{n_rank_m}")
        check("женские звания — по новой поставке",
              one("SELECT count(*) FROM lookup WHERE kind='rank_f'") == 41)
        # Переложенные записи не должны остаться в чужом перечне у тех, кто уже
        # успел получить прежнюю поставку: migrate.sql только дополняет, поэтому
        # для них есть отдельное удаление.
        check("«крестьянский сын» только среди мужских",
              one("SELECT count(*) FROM lookup WHERE value='крестьянский сын' AND kind='rank_f'") == 0)
        check("«крестьянская вдова» только среди женских",
              one("SELECT count(*) FROM lookup WHERE value='крестьянская вдова после 1-го брака' "
                  "AND kind='rank_m'") == 0)
        check("«крестьянская жена» на месте",
              one("SELECT count(*) FROM lookup WHERE value='крестьянская жена'") == 1)
        # Справочник населённых пунктов доезжает до уже установленной программы.
        # Ровно этой проверки не хватало в августе: у Романа не появилась новая
        # таблица, и половина сборки молча не работала. НП — тот же случай:
        # без них поле подсказывать нечем, а поставку человек не пересоздаёт.
        # С 08.10.2026 пунктов в поставке нет: у установленного остаются свои,
        # чужие не добавляются.
        check("пункты поставки больше не добавляются: «Аксениха» не появилась",
              one("SELECT count(*) FROM place WHERE name_norm = 'аксениха'") == 0)
        check("Борисоглебское на месте",
              one("SELECT count(*) FROM place WHERE name='Борисоглебское'") == 1)
        check("двойник без подробностей слит с пунктом поставки, упоминание переведено",
              one("SELECT count(*) FROM place WHERE id = 9001") == 0
              and db.execute("SELECT p.np_type FROM person_mention m JOIN place p ON p.id = m.place_id "
                             "WHERE m.first_name = 'Двойник'").fetchone() == ("с.",))
        check("тёзка с другими подробностями остался, упоминание — при нём",
              one("SELECT count(*) FROM place WHERE name_norm = 'малово'") == 2
              and one("SELECT place_id FROM person_mention WHERE first_name = 'Тёзка'") == 9002)
        check("пустая строка с меньшим номером слита в подробную, упоминание переведено (06.10.2026)",
              one("SELECT count(*) FROM place WHERE id = 9010") == 0
              and db.execute("SELECT p.id, p.uyezd FROM person_mention m JOIN place p ON p.id = m.place_id "
                             "WHERE m.first_name = 'Пустой'").fetchone() == (9011, "Макарьевский"))
        check("при тёзках пустая строка не слита ни в одну — неясно, чья она",
              one("SELECT count(*) FROM place WHERE name_norm = 'хмельничное тестовое'") == 3
              and one("SELECT place_id FROM person_mention WHERE first_name = 'Ничей'") == 9020)
        check("пустая строка с набранным руками полным местом осталась",
              one("SELECT count(*) FROM place WHERE name_norm = 'займище тестовое'") == 2
              and one("SELECT place_id FROM person_mention WHERE first_name = 'Заречный'") == 9030)
        check("тестовый пункт шаблона «Лодзь» убран вместе с губернией и уездом (Роман 06.10.2026)",
              one("SELECT count(*) FROM place WHERE name_norm = 'лодзь'") == 0
              and one("SELECT count(*) FROM lookup WHERE value IN ('Петровская', 'Лодзинский')") == 0)
        check("«Котело» стоит в записи — остался, его уезд в перечне тоже",
              one("SELECT place_id FROM person_mention WHERE first_name = 'Котельский'") == 9041
              and one("SELECT count(*) FROM place WHERE id = 9041") == 1
              and one("SELECT count(*) FROM lookup WHERE kind = 'uyezd' AND value = 'Галический'") == 1)
        check("уезд «Петровский», заведённый человеком и нигде не стоящий, убран (уборка 08.10.2026)",
              one("SELECT count(*) FROM lookup WHERE kind = 'uyezd' AND value = 'Петровский'") == 0)
        check("в самой поставке тестовых строк больше нет",
              sqlite3.connect(seed).execute(
                  "SELECT (SELECT count(*) FROM place WHERE name IN ('Лодзь', 'Котело')) + "
                  "(SELECT count(*) FROM lookup WHERE value IN ('Петровская', 'Лодзинский', 'Галический', 'Котельская'))"
              ).fetchone()[0] == 0)
        check("упоминаний без пункта после слияния нет",
              one("SELECT count(*) FROM person_mention m WHERE m.place_id IS NOT NULL "
                  "AND NOT EXISTS (SELECT 1 FROM place p WHERE p.id = m.place_id)") == 0)
        check("список на сверку после импорта доехал: таблица и индекс (схема 9)",
              one("SELECT count(*) FROM sqlite_master WHERE name IN ('review_item', 'ix_review_open')") == 2
              and one("SELECT count(*) FROM review_item") == 0)
        check("перечень волостей есть — карточка пункта будет его пополнять",
              one("SELECT count(*) FROM lookup_kind WHERE kind = 'volost'") == 1)

        print("\n2а. Уборка перечней: только незанятое вне поставки (Роман 08.10.2026)")
        gone = lambda v: one("SELECT count(*) FROM lookup WHERE value = ?", v) == 0
        check("набранное человеком и нигде не стоящее — убрано вместе с частотами",
              gone("отставной канонир")
              and one("SELECT count(*) FROM usage_stat WHERE value = 'отставной канонир'") == 0)
        check("значение прежней поставки, не стоящее нигде, — убрано",
              gone("уволенный в запас армии бомбардир наводчик") and gone("апоплексия"))
        check("значение прежней поставки, стоящее в записи, — осталось, частоты целы",
              not gone("отставной унтер-офицер") and not gone("древность")
              and one("SELECT count FROM usage_stat WHERE value = 'отставной унтер-офицер'") == 5)
        check("значение, набранное в записи с заглавной буквы, — осталось (сравнение по ключу)",
              not gone("мещанин города Юрьевца"))
        check("звание, стоящее в записи в дореформенном написании («…ъ»), — осталось",
              not gone("безземельный крестьянин"))
        check("значения новой поставки на месте, даже если нигде не стоят",
              not gone("псаломщик") and not gone("чахотка") and not gone("Костромской"))
        check("убранное записано для сверки приходов; счётчик уборки — в настройках",
              one("SELECT count(*) FROM lookup_dropped WHERE value_norm = 'отставной канонир'") == 1
              and int(one("SELECT value FROM setting WHERE key = 'cleanup_lookups'")) >= 4)
        check("записи, упоминания и их тексты не изменились",
              snapshot(db) == before, "снимок записей до и после обновления различается")
        check("появилась колонка комментария пункта (схема 10)",
              "comment" in [r[1] for r in db.execute("PRAGMA table_info(place)")])
        check("разбор ИОФ заработает: «Никита» не подменяется",
              db.execute("SELECT d.name FROM name_form f JOIN name_dict d ON d.id=f.name_id "
                         "WHERE f.kind IN ('name','variant') AND f.form_norm='никита' "
                         "ORDER BY f.priority LIMIT 1").fetchone()[0] == "Никита")

        check("роли восприемников 3 и 4 доехали до пользователя (21.09.2026)",
              one("SELECT count(*) FROM role WHERE code IN ('godparent3','godparent4')") == 2)

        print("\n3. После обновления ничего пользовательского не потерялось")
        check("значение, заведённое человеком и стоящее в записи, на месте",
              one("SELECT count(*) FROM lookup WHERE value='мещанин города Юрьевца'") == 1)
        check("оно не задвоилось",
              one("SELECT count(*) FROM lookup WHERE value='крестьянин' AND kind='rank_m'") == 1)
        check("накопленная статистика подсказок цела",
              one("SELECT count FROM usage_stat WHERE value='крестьянин'") == 42)
        check("заведённое дело на месте (и дело второго года — рядом)", one("SELECT count(*) FROM mk_case") == 2
              and one("SELECT church FROM mk_case WHERE id = 1") == "Христорождественская")
        check("набранные записи на месте", one("SELECT count(*) FROM entry") == 6)
        check("персона записи на месте",
              one("SELECT first_name FROM person_mention WHERE entry_id=1") == "Евграф")
        check("настройка пользователя не перезаписана",
              one("SELECT value FROM setting WHERE key='modernize_names'") == "0")

        print("\n3а. Починка номера девочек — заказчик 21.09.2026: «нужно исправить»")
        check("девочке номер перенесён в женскую колонку",
              one("SELECT no_male IS NULL AND no_female = 3 FROM entry WHERE id=2") == 1)
        check("мальчик не тронут", one("SELECT no_male = 4 AND no_female IS NULL FROM entry WHERE id=3") == 1)
        check("девочка с женским номером не тронута",
              one("SELECT no_male IS NULL AND no_female = 5 FROM entry WHERE id=4") == 1)
        check("ребёнок без пола не тронут — угадывать нельзя",
              one("SELECT no_male = 6 AND no_female IS NULL FROM entry WHERE id=5") == 1)
        check("число исправленных записано для экрана «О программе»",
              one("SELECT value FROM setting WHERE key='repair_count_column'") == "1")
        check("ребёнок без пола с мужским номером посчитан отдельно: 1",
              one("SELECT value FROM setting WHERE key='repair_unknown_sex'") == "1")
        check("записей с причтом без имени посчитано: 2",
              one("SELECT value FROM setting WHERE key='repair_clergy_noname'") == "2")

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
        check("повторная уборка ничего больше не убрала и записей не тронула",
              db.execute("SELECT count(*) FROM lookup WHERE kind='rank_m'").fetchone()[0] == 54
              and snapshot(db) == before)
        check("и не задвоило имена",
              db.execute("SELECT count(*) FROM name_dict").fetchone()[0] == 3112)
        check("повторная починка ничего не нашла и счётчик не вырос",
              db.execute("SELECT value FROM setting WHERE key='repair_count_column'").fetchone()[0] == "1")
        # Соответствия имён — пользовательские, обновление их не трогает
        # (в отличие от name_form, которая перезаливается целиком).
        check("повторное слияние двойников ничего не изменило",
              db.execute("SELECT count(*) FROM place WHERE id IN (9011, 9020, 9021, 9022, 9030, 9031)").fetchone()[0] == 6
              and db.execute("SELECT group_concat(place_id, ',') FROM (SELECT place_id FROM person_mention "
                             "WHERE first_name IN ('Пустой', 'Ничей', 'Заречный') ORDER BY sort_order)").fetchone()[0]
              == "9011,9020,9030")
        check("поправленный в карточке пункт поставки не задвоен",
              db.execute("SELECT count(*) FROM place WHERE name_norm='бухарино'").fetchone()[0] == 1)
        check("переименованный пункт поставки не вернулся под старым названием",
              db.execute("SELECT count(*) FROM place WHERE name_norm='кнышево'").fetchone()[0] == 0)
        check("переименованный без ссылки пункт не вернулся (память переименований)",
              db.execute("SELECT count(*) FROM place WHERE name_norm='логинцево'").fetchone()[0] == 0)
        row = db.execute("SELECT np_type, uyezd, full_location, familio_url, origin FROM place "
                         "WHERE name_norm='чертеж малый'").fetchall()
        # До 08.10.2026 такой пункт получал подробности из поставки; теперь
        # пунктов в поставке нет — он остаётся как был, заполняется карточкой.
        check("пункт из архива без подробностей остался как был — чужих подробностей нет",
              row == [(None, None, None, None, "archive")], str(row))
        check("набранное руками полное место не затёрто поставкой",
              db.execute("SELECT full_location, np_type FROM place WHERE name_norm='поселихино'").fetchone()
              == ("мой текст", None))
        check("у поправленного человеком пункта подробности из поставки не подставлены",
              db.execute("SELECT familio_url FROM place WHERE name_norm='бухарино'").fetchone()[0] is None)
        check("и правка человека не откатилась",
              db.execute("SELECT uyezd FROM place WHERE name_norm='бухарино'").fetchone()[0] == "Кинешемский")
        cases = db.execute("SELECT id, year, church FROM mk_case ORDER BY year").fetchall()
        check("дело на год: у 1896 и 1897 годов — свои дела, приход тот же",
              [c[1] for c in cases] == [1896, 1897] and cases[0][2] == cases[1][2], str(cases))
        by_year = {c[1]: c[0] for c in cases}
        check("каждая запись с годом привязана к делу своего года",
              db.execute("SELECT case_id FROM entry WHERE id = 90").fetchone()[0] == by_year[1897]
              and db.execute("SELECT case_id FROM entry WHERE id = 1").fetchone()[0] == by_year[1896])
        check("записи без года остались при прежнем деле",
              db.execute("SELECT count(DISTINCT case_id) FROM entry WHERE event_year IS NULL").fetchone()[0] == 1)
        check("повторное обновление дел не плодит",
              db.execute("SELECT count(*) FROM mk_case").fetchone()[0] == 2)
        check("соответствие «Пискарь» → «Кесарь» пережило обновление",
              db.execute("SELECT target FROM name_alias WHERE form_norm='пискарь'").fetchone() == ("Кесарь",))
        db.close()

    print(f"\nИтог: успешно {ok_count}, ошибок {fail_count}\n")
    return 1 if fail_count else 0


if __name__ == "__main__":
    raise SystemExit(main())
