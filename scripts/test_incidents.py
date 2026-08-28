#!/usr/bin/env python3
"""
Регресс-тесты по инцидентам.

ПРАВИЛО ПРОЕКТА: инцидент не закрыт, пока на него нет механической проверки.

Сюда попадает каждая поломка, которая дошла до человека — до Романа или до Mike.
Список ниже не история, а действующая защита: он не даёт вернуть то, что уже
один раз стоило людям времени.

Проверки нарочно грубые и дешёвые. Они смотрят на форму кода и конфигурации,
а не на поведение — поведение проверяют db/test_*.py. Задача здесь другая:
поймать возврат конкретной ошибки, даже если её вернут в другом месте.

Как добавлять. Новая функция incident_ГГГГММДД_короткое_имя(), в docstring —
что случилось, кто пострадал и чем это стоило. Регистрация в списке ИНЦИДЕНТЫ.

Запуск:
    python3 scripts/test_incidents.py
"""

import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

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


def read(rel: str) -> str:
    p = REPO / rel
    return p.read_text(encoding="utf-8") if p.exists() else ""


def strip_comments(text: str) -> str:
    """Убирает комментарии: в них описаны прошлые ошибки, и проверки
    не должны ругаться на собственную летопись."""
    text = re.sub(r"/\*.*?\*/", "", text, flags=re.S)
    return re.sub(r"^\s*//.*$", "", text, flags=re.M)


def block_after(text: str, start: int, open_ch: str, close_ch: str) -> str:
    """Кусок кода от первой открывающей скобки до парной ей закрывающей.

    Окно фиксированной длины для этого не годится: 28.08.2026 проверяющий
    агент вставил молчаливый перехват ошибки рядом с настоящим, и проверка
    по окну в 400 символов увидела чужой report() и пропустила подделку.
    """
    i = text.find(open_ch, start)
    if i < 0:
        return ""
    depth = 0
    for j in range(i, len(text)):
        if text[j] == open_ch:
            depth += 1
        elif text[j] == close_ch:
            depth -= 1
            if depth == 0:
                return text[i:j + 1]
    return text[i:]


# ============================================================================


def incident_20260813_baza_ne_doehala():
    """13.08.2026. Роман поставил новую версию поверх старой, база осталась
    от прошлой сборки, и половина программы молча не работала. Стоило трёх дней.

    Защита: обновление по отпечатку поставки. Отпечаток обязан считаться по
    справочникам, схеме и файлу миграции — иначе правки до человека не доедут.
    """
    build = read("db/build_seed.py")
    check("отпечаток поставки считается",
          "seed_stamp" in build)
    for part in ("seed", "schema.sql", "migrate.sql"):
        check(f"в отпечаток входит {part}", part in build)
    check("миграция существует и не пуста", len(read("db/migrate.sql")) > 500)
    check("проверка обновления у пользователя на месте",
          (REPO / "db/test_upgrade.py").exists())


def incident_20260813_molchalivyj_perehvat():
    """13.08.2026. Пустой перехват ошибки в интерфейсе превращал поломку
    в «просто ничего не происходит». Роман потерял целый цикл проверки.

    Защита: у всех перехватов должно быть сообщение человеку через report().
    """
    bad = []
    for f in sorted((REPO / "src").rglob("*.ts")) + sorted((REPO / "src").rglob("*.tsx")):
        code = strip_comments(f.read_text(encoding="utf-8"))
        for m in re.finditer(r"\.catch\(", code):
            body = block_after(code, m.start(), "(", ")")
            if "report(" not in body:
                bad.append(f"{f.name}: .catch{body[:60].strip()}")
    check("нет перехватов ошибок без сообщения человеку", not bad, "; ".join(bad[:3]))
    check("есть общий приёмник ошибок", "export function report" in read("src/errors.ts"))


def incident_20260817_kirillica_i_lower():
    """17.08.2026 и повторно 28.08.2026. В SQLite lower() и COLLATE NOCASE
    понимают только латиницу: на кириллице поиск молча возвращает ноль строк.
    Правило было записано — и нарушено в тот же день в собственной проверке.

    Защита: этих конструкций не должно быть в запросах к данным.
    """
    for rel in ("db/statements.sql", "db/schema.sql", "db/migrate.sql"):
        text = read(rel)
        check(f"{rel} без COLLATE NOCASE", "COLLATE NOCASE" not in text.upper())
    rust = read("src-tauri/src/main.rs")
    check("в main.rs нет lower() в запросах", "lower(" not in rust)
    check("нормализация есть в main.rs", "fn normalize" in rust)
    check("нормализация есть в build_seed.py", "def norm" in read("db/build_seed.py"))


def incident_20260824_np_iskalos_ne_tam():
    """24.08.2026. Поле «НП» искало населённые пункты в плоских перечнях lookup,
    где их нет и быть не может — они в таблице place. Поле молчало три недели,
    по четыре раза на каждую запись. Роман принял поломку за недоделку.

    Защита: запросы подсказок обязаны жить в statements.sql, а не в коде,
    и населённые пункты обязаны браться из своей таблицы.
    """
    sql = read("db/statements.sql")
    # Имена блоков сверяем построчно и целиком: подстрока пропустила бы
    # переименование suggest_place → suggest_place_X.
    names = {ln.strip()[4:].strip() for ln in sql.splitlines() if ln.strip().startswith("-- @")}
    for block in ("suggest_ranked", "suggest_place", "suggest_first_name",
                  "suggest_patronymic", "suggest_lookup"):
        check(f"блок {block} в statements.sql", block in names)
    check("населённые пункты ищутся в таблице place",
          "FROM place" in sql.split("-- @suggest_place")[1].split("-- @")[0])
    rust = read("src-tauri/src/main.rs")
    body = rust.split("fn suggest(", 1)[-1].split("\n}\n", 1)[0]
    check("в suggest() не осталось своего SELECT", "SELECT" not in body.upper())


def incident_20260827_spisok_ne_pryatalsya():
    """24 и 27.08.2026, одна жалоба дважды. Подсказки уходят на каждое нажатие;
    при выборе строки предыдущий запрос ещё в пути и, вернувшись, открывает
    список заново. В первый раз починили не ту причину.

    Защита: закрытие списка обязано увеличивать счётчик запросов, иначе
    устаревший ответ считается актуальным.
    """
    for name in ("IofField.tsx", "Suggest.tsx"):
        code = strip_comments(read(f"src/{name}"))
        m = re.search(r"(closeSuggestions|closeList)\s*(\(\)\s*\{|=\s*\(\)\s*=>\s*\{)", code)
        check(f"{name}: есть отдельная функция закрытия списка", m is not None)
        if m:
            # Тело берём по балансу скобок: вложенный блок внутри функции
            # не должен обрывать проверку на первой закрывающей.
            body = block_after(code, m.start(), "{", "}")
            check(f"{name}: закрытие отменяет отправленные запросы",
                  "seq.current" in body)


def incident_20260827_vypusk_bez_ustanovshchikov():
    """27.08.2026. Выпуск latest-test ушёл к Роману без единого установщика,
    а конвейер отрапортовал успех: шаги проверяли, что команда вернула ноль,
    а не что человеку есть что скачать. Роман потерял день.

    Защита: сборка обязана падать, если установщиков нет, и обязана после
    выкладки спросить у GitHub, что реально лежит на странице выпуска.
    """
    wf = read(".github/workflows/build.yml")
    check("сборка падает без установщика Windows",
          "Нет установщика Windows" in wf)
    check("сборка падает без установщика macOS",
          "Нет установщика macOS" in wf)
    check("после выкладки проверяется состав выпуска",
          "gh release view" in wf and "--json assets" in wf)
    # Проверка обязана быть именно после выкладки: недостаточно посчитать файлы
    # в папке, надо спросить у GitHub, что видно на странице.
    after = wf.split("gh release create", 1)[-1]
    check("после создания выпуска проверяется, что видит человек",
          "gh release view" in after and "\\.exe$" in after and "\\.dmg$" in after)


def incident_20260828_pricht_dopushchenie():
    """28.08.2026. Причт свернули, решив, что он «меняется раз в дело».
    На первой же настоящей странице он сменился в 82% записей. Допущение
    о материале вывели, а не проверили.

    Защита формальная: правило записано в CLAUDE.md, и оттуда его читает
    каждая сессия и проверяющий агент. Проверить сам факт проверки допущений
    механически нельзя — можно только не дать правилу потеряться.
    """
    claude = read("CLAUDE.md")
    check("правило про допущения о материале записано",
          "на самом материале" in claude)
    check("правило про повторную жалобу записано",
          "жалоба дважды" in claude)
    check("правило про проверку результата, а не кода возврата",
          "команда вернула ноль" in claude)


def incident_20260828_pravilo_ispolneno_bukvalno():
    """28.08.2026. «Проверь, на какой коммит указывает тег» было исполнено
    дословно — а страницу, которую откроет Роман, никто не открыл. Установщиков
    на ней не было.

    Защита: в CLAUDE.md прямо сказано открывать страницу выпуска глазами,
    и то же требование стоит в задании проверяющего агента.
    """
    claude = read("CLAUDE.md")
    check("в CLAUDE.md требуется открыть страницу выпуска",
          "открой страницу выпуска" in claude.lower())
    agent = read(".claude/agents/проверяющий.md")
    check("проверяющий агент заведён", len(agent) > 500)
    check("агент обязан смотреть страницу выпуска глазами",
          "releases/tag/latest-test" in agent)
    check("агент не должен верить пересказу",
          "Не верь пересказу" in agent)
    # Проверка, которую можно молча выкинуть из конвейера, — не защита.
    wf = read(".github/workflows/build.yml")
    check("сами регресс-тесты стоят в конвейере",
          "scripts/test_incidents.py" in wf)


# ============================================================================

# Поломки, которые уже известны, но ещё не исправлены. Проверка приходит вместе
# с починкой — до этого момента инцидент живёт здесь и печатается при каждом
# прогоне, чтобы о нём нельзя было забыть. Пустой список — хорошая новость.
ОТКРЫТЫЕ = [
    ("28.08.2026", "Счёт всегда пишется в мужскую колонку независимо от пола "
                   "ребёнка (src/BirthForm.tsx). Половина записей ложится "
                   "с неверным номером, на экране не видно. "
                   "Решение: spec/2026-08-28-schyot-po-polu-i-pricht.md"),
    ("28.08.2026", "Причт свёрнут, хотя на настоящих сканах меняется в 82% "
                   "записей. Решение там же, ждём слова Романа."),
]

ИНЦИДЕНТЫ = [
    incident_20260813_baza_ne_doehala,
    incident_20260813_molchalivyj_perehvat,
    incident_20260817_kirillica_i_lower,
    incident_20260824_np_iskalos_ne_tam,
    incident_20260827_spisok_ne_pryatalsya,
    incident_20260827_vypusk_bez_ustanovshchikov,
    incident_20260828_pricht_dopushchenie,
    incident_20260828_pravilo_ispolneno_bukvalno,
]


def main() -> int:
    _utf8_stdout()
    print(f"\nРегресс-тесты по инцидентам: {len(ИНЦИДЕНТЫ)} инцидентов\n")
    for fn in ИНЦИДЕНТЫ:
        head = (fn.__doc__ or "").strip().split("\n")[0]
        print(f"{fn.__name__.replace('incident_', '')} — {head}")
        fn()
        print()
    if ОТКРЫТЫЕ:
        print("ОТКРЫТЫЕ ИНЦИДЕНТЫ — известны, но ещё не исправлены:")
        for дата, текст in ОТКРЫТЫЕ:
            print(f"  · {дата}. {текст}")
        print()
    print(f"Итог: успешно {ok_count}, ошибок {fail_count}, "
          f"открытых инцидентов {len(ОТКРЫТЫЕ)}")
    return 1 if fail_count else 0


if __name__ == "__main__":
    raise SystemExit(main())
