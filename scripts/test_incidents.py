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


def rust_all() -> str:
    """Весь код на Rust: программа (src-tauri/src) и крейт без окна
    (src-tauri/core/src). Проверки не должны зависеть от того, в каком из
    файлов живёт функция, — 01.10.2026 перенос кода в крейт уронил шесть
    проверок, хотя поведение не менялось (техдолг Д4)."""
    files = sorted((REPO / "src-tauri" / "src").glob("*.rs")) + sorted((REPO / "src-tauri" / "core" / "src").glob("*.rs"))
    return "\n".join(f.read_text(encoding="utf-8") for f in files)


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
    rust = rust_all()
    check("в коде на Rust нет lower() в запросах", "lower(" not in rust)
    check("нормализация своя, не средствами SQLite", "pub fn normalize(" in rust)
    check("её поведение проверяет тест крейта (NFC, «ё», пробелы)",
          "fn nfc()" in read("src-tauri/core/src/text.rs"))
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
    rust = rust_all()
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

def incident_20260913_schyot_v_muzhskuyu_kolonku():
    """Найдено 28.08.2026 при наборе настоящих сканов, исправлено 13.09.2026.
    Форма всегда писала счёт в мужскую колонку независимо от пола ребёнка.
    Половина записей ложилась с неверным номером, на экране не видно,
    выгрузка в Familio ушла бы с ошибкой.

    Защита: раскладка вынесена в чистую функцию src/count.ts, форма обязана
    пользоваться ею, а не писать в колонки напрямую; функция проверяется
    отдельным тестом, который стоит в конвейере.
    """
    form = strip_comments(read("src/BirthForm.tsx"))
    check("форма раскладывает счёт через splitCount", "splitCount(" in form)
    check("в форме нет прямой записи счёта в мужскую колонку",
          not re.search(r"no_male:\s*count\b", form))
    check("в форме нет буквального null в женской колонке",
          not re.search(r"no_female:\s*null", form))
    count_ts = read("src/count.ts")
    check("при неизвестном поле функция отказывается", "return null" in count_ts)
    check("тест функции есть", (REPO / "scripts/test_count.mjs").exists())
    check("тест функции стоит в конвейере",
          "scripts/test_count.mjs" in read(".github/workflows/build.yml"))


# ============================================================================

def incident_20260913_ctrl_enter_pri_otkrytom_spiske():
    """Найдено проверяющим агентом 13.09.2026 на стенде, до выкладки.
    Ctrl+Enter при открытом списке подсказок: поле подставляло выбранное
    («Бухарино»), но событие всплывало до сохранения, и запись уходила
    с недобранным «Бух» — в place заводился НП «Бух», в person_index
    запоминалась персона с ним. Порча данных, на экране незаметная.

    Защита: поля с подсказкой при Ctrl+Enter и открытом списке только
    подставляют и гасят всплытие; сохранение — следующим Ctrl+Enter, когда
    списка уже нет. Живой сценарий прогоняется стендом (не в репозитории).
    """
    for f in ("src/Suggest.tsx", "src/IofField.tsx"):
        src = strip_comments(read(f))
        i = src.find('e.key === "Enter"')
        block = src[i:i + 400] if i >= 0 else ""
        check(f"{f}: Ctrl+Enter при открытом списке гасит всплытие",
              "stopPropagation()" in block and "ctrlKey" in block)
    form = strip_comments(read("src/BirthForm.tsx"))
    check("сохранение по Ctrl+Enter висит на обёртке формы, а не на полях",
          "function hotkeys" in form and "onKeyDown={hotkeys}" in form)


# ============================================================================

def incident_20260914_arhiv_ne_gruzitsya_na_windows():
    """Роман, 14.09.2026: «Архив не загрузился — ожидался файл архива,
    а пришло что-то другое». Команда import_archive ждала сырое тело запроса
    (tauri::ipc::Request → InvokeBody::Raw). На Windows fetch на ipc.localhost
    у WebView2 не прошёл, Tauri молча упал на postMessage, где всё уходит
    JSON-ом, — и тело пришло не сырым. В песочнице это не воспроизвести:
    стенд не Tauri, cargo build невозможен.

    Защита: байты передаются обычным аргументом Vec<u8>; сырое тело в
    командах не используется. Живой прогон — сквозной проверкой на
    Windows-раннере (scripts/e2e/, msedgedriver «attach»), она стоит в конвейере перед выкладкой.
    """
    rs = strip_comments(rust_all())
    check("команды не принимают сырое тело запроса",
          "tauri::ipc::Request" not in rs and "InvokeBody::Raw" not in rs)
    i = rs.find("fn import_archive(")
    check("import_archive принимает bytes: Vec<u8>",
          i >= 0 and "bytes: Vec<u8>" in rs[i:i + 200])
    # С 05.10.2026 блок архива живёт в своём файле (экран «О программе»).
    ts = strip_comments(read("src/ArchiveBlock.tsx"))
    check("интерфейс передаёт байты аргументом { bytes }",
          'invoke<ImportReport>("import_archive", { bytes })' in ts)
    wf = read(".github/workflows/build.yml")
    check("сквозная проверка на Windows стоит в конвейере",
          "msedgedriver" in wf and "scripts/e2e/windows.py" in wf)
    check("сквозная проверка перезапускает приложение (форма продолжает с места)",
          "def resumed(" in read("scripts/e2e/windows.py"))


# ============================================================================

def incident_20260915_neskolko_spiskov_razom():
    """Роман, 15.09.2026: «при выборе отца, матери или восприемника открывается
    сразу несколько списков, которые приходится протыкивать мышкой». Три снимка.
    Списки открывались на любое изменение значения поля, включая программную
    подстановку (НП, звание, жена по мужу, причт из списка).

    Защита: в обоих полях с подсказкой список открывается только после
    onChange с клавиатуры (флаг typed). Живой сценарий — _стенд/lists.mjs.
    """
    for f in ("src/Suggest.tsx", "src/IofField.tsx"):
        src = strip_comments(read(f))
        check(f"{f}: есть флаг набора с клавиатуры", "typed = useRef(false)" in src)
        check(f"{f}: onChange поля ставит флаг", "typed.current = true;" in src)
        check(f"{f}: без флага эффект список не открывает", "if (!byKeyboard) return;" in src)
    sql = read("db/statements.sql")
    check("частоты имён фильтруются по полу (usage_gender_filter)",
          "-- @usage_gender_filter" in sql and "{usage_gender}" in sql)
    check("тест 4б на имена из частот есть", "4б" in read("db/test_suggest.py"))


# ============================================================================

def incident_20260918_razbor_stiraet_poslednyuyu_bukvu():
    """Сквозная проверка на Windows, сборка #27, 18.09.2026: робот набрал
    «Мария», в поле осталось «Мари». Асинхронный разбор имени (parse_iof)
    отдавал родителю значение, с которым был вызван, — ответ на «Мари»
    приходил после последней буквы и перезаписывал поле. При быстром наборе
    то же случилось бы у Романа.

    Защита: у разбора счётчик запросов, как у подсказок; устаревший ответ
    отбрасывается. Живой сценарий — e2e шаг 5 на Windows.
    """
    src = strip_comments(read("src/IofField.tsx"))
    i = src.find('invoke<Parsed>("parse_iof"')
    block = src[max(0, i - 200):i + 400] if i >= 0 else ""
    check("разбор имени нумерует запросы", "parseSeq.current" in block)
    check("устаревший ответ разбора отбрасывается",
          "if (mine !== parseSeq.current) return;" in block)


# ============================================================================

def incident_20260922_prichjt_bez_imeni():
    """Проверяющий 22.09.2026: после перезапуска (с 21.09) и при правке записи
    персоны поднимались в форму без разбора ИОФ, а причт при этом свёрнут —
    поле не смонтировано, разбора нет, payload() писал имя и фамилию NULL.
    У Романа записи после перезапуска ушли с причтом «только звание».

    Защита: перед сохранением персона без разбора разбирается той же командой
    parse_iof (withParsed); число пострадавших записей считается при
    обновлении и показывается на «О программе».
    """
    form = strip_comments(read("src/BirthForm.tsx"))
    check("перед сохранением персоны без разбора разбираются", "async function withParsed(" in form)
    i = form.find("async function save()")
    check("save() ждёт разбор всех персон", i >= 0 and "map(withParsed)" in form[i:i + 1200])
    check("записи с причтом без имени считаются при обновлении",
          "repair_clergy_noname" in read("db/migrate.sql"))
    check("… и показываются на «О программе»", "clergy_noname_entries" in read("src/App.tsx"))


def incident_20260924_sverka_ne_vezde():
    """Роман 24.09.2026: сверка имени не срабатывала у матери и восприемников —
    поле заполнялось не с клавиатуры (жена по мужу, персона из подсказки),
    фокус уходил к следующему пустому полю, «выхода из поля» не было.
    И Ctrl+Enter в окне сверки подставлял первое похожее имя, а следующий
    Ctrl+Enter сохранял запись с ним.

    Защита: разбор, пришедший не с клавиатуры, сразу идёт в ту же проверку
    (decide), что и уход из поля; в окне Enter с Ctrl/Cmd не выбирает.
    """
    iof = strip_comments(read("src/IofField.tsx"))
    check("разбор не с клавиатуры сверяется сразу",
          "if (!byKeyboard && value.trim() && inputEl.current?.offsetParent != null)" in iof)
    # 27.09.2026: только видимое поле — причт общий для трёх форм, скрытые
    # не должны открывать окна сверки на чужой набор.
    check("уход из поля и автоподстановка — одна логика decide()",
          "function decide(" in iof and "decide(p, text)" in iof)
    res = strip_comments(read("src/NameResolve.tsx"))
    check("Ctrl+Enter в окне сверки не выбирает", "if (!e.ctrlKey && !e.metaKey) pick()" in res)
    check("кнопки «Новое имя» нет (имена только из справочника)", "Новое имя" not in res)


def incident_20260924_pravka_sterla_pometku():
    """Проверяющий 24.09.2026, до выкладки: первая версия чистки пометок сверки
    помнила слово из прошлой записи и при открытии другой записи на правку
    стирала её «Имя в документе: …» — после сохранения пометка пропадала из
    базы. И при стирании отца уходил любой НП матери, совпадающий с отцовским,
    а при перенаборе отца не возвращался.

    Защита: трогается только та пометка, что дописана в этом блоке, и только
    пока она есть в примечании; НП матери убирается, только если он был
    скопирован от отца, при перенаборе отца копируется снова.
    """
    pb = strip_comments(read("src/PersonBlock.tsx"))
    check("чистка пометки сверяет её точный текст", "parts.includes(want.note)" in pb)
    form = strip_comments(read("src/BirthForm.tsx"))
    check("НП матери помнит, что скопирован от отца", "copiedPlace.current" in form)
    check("при новой или открытой записи память сбрасывается",
          form.count("copiedPlace.current = null") >= 2 and form.count("childDocFor.current = {}") >= 2)


def incident_20260925_otchestvo_bez_familii():
    """Роман 25.09.2026: «Иван Пискарев» без фамилии не сверялся — второе
    слово считалось фамилией; с фамилией сверялось. Курсор после выбора отца
    и подстановки матери вставал в ИОФ матери, а не восприемника. «Стр.» на
    100% показывала «93» вместо «938об-939».

    Защита: отчество спрашивается и без фамилии; после подстановки жены
    курсор переносится к восприемнику; первая строка — узкие год и счёт,
    страница не уже 10em, при нехватке места перенос строки.
    """
    rust = strip_comments(rust_all())
    check("отчество без фамилии сверяется", "rest.len() >= 2 && looks_like_patronymic" not in rust
          and "!not_patr && looks_like_patronymic(first_rest)" in rust)
    form = strip_comments(read("src/BirthForm.tsx"))
    check("после подстановки жены — к первому восприемнику", "god1Iof.current?.focus()" in form)
    css = read("src/styles.css")
    check("«Стр.» не уже 10em, строка переносится",
          ".row.tight .field.wide { flex: 1 1 10em; min-width: 10em; }" in css and ".row.tight { flex-wrap: wrap;" in css)


def incident_20260925_lishnij_parametr():
    """Ревьюер 25.09.2026, до выкладки: переименование НП в собранной программе
    падало всегда — во все три блока place_rename_* передавался :name_norm, а
    в двух его нет; rusqlite на лишний именованный параметр отвечает ошибкой
    (InvalidParameterName). Python-тест те же блоки гонял через sqlite3, а он
    лишние ключи словаря прощает — «тест, который не ходит через Rust».

    Защита — механическая, на весь класс: у каждого вызова
    statement("X") … named_params! { … } в коде на Rust набор параметров
    обязан входить в набор параметров блока X в statements.sql. С 30.09.2026 —
    во всех файлах src-tauri/src (выгрузка живёт в export.rs).
    """
    rust = rust_all()
    text = read("db/statements.sql")
    blocks, name, buf = {}, None, []
    for line in text.splitlines():
        m = line.strip()
        if m.startswith("-- @"):
            if name:
                blocks[name] = "\n".join(buf)
            name, buf = m[4:].strip(), []
        elif name is not None and not m.startswith("--"):
            buf.append(re.sub(r"'[^']*'", "''", line))
    if name:
        blocks[name] = "\n".join(buf)
    calls = re.findall(r'statement\("(\w+)"\)\?[^;]{0,200}?named_params!\s*\{(.*?)\}', rust, re.S)
    check("вызовы с именованными параметрами найдены", len(calls) >= 20, f"{len(calls)}")
    bad = []
    for block, params in calls:
        passed = set(re.findall(r'":(\w+)"', params))
        declared = set(re.findall(r":(\w+)", blocks.get(block, "")))
        extra = passed - declared
        if block not in blocks or extra:
            bad.append(f"{block}: {sorted(extra) or 'нет блока'}")
    check("ни одному блоку не передаётся лишний параметр", not bad, "; ".join(bad))
    # Имя блока переменной (как было в цикле по place_rename_*) проверка выше
    # не видит — такие вызовы только без параметров или с выбором из литералов.
    loose = [m for m in re.findall(r"statement\((\w+)\)", rust) if m not in ("name",)]
    check("имя блока в statement() — литерал, не переменная", not loose, ", ".join(loose))


def incident_20260927_otchyot_27_09():
    """Роман и Mike 27.09.2026: у причта ИОФ не сверялось как у всех (исключение
    #35); причт в каждом разделе свой; «Запись пустая» пугала «это не ваша
    ошибка, пришлите текст»; память персон задваивала людей без места;
    e2e находил поле в скрытой форме; сборка предупреждала «STATIC_VCRUNTIME
    is deprecated» (CLI 2.11 ставил устаревшую переменную).

    Защита: исключения для причта нет; причт восстанавливает один
    ClergyProvider (раздел 0 — любой); проверки заполнения идут через warn();
    person_remember пишет пустую строку вместо NULL; e2e ищет видимый
    элемент; @tauri-apps/cli в lock-файле не 2.11.*.
    """
    iof = strip_comments(read("src/IofField.tsx")) + strip_comments(read("src/PersonBlock.tsx"))
    check("у причта сверка ИОФ как у всех (surnameSecond нет)", "surnameSecond" not in iof)
    forms = {f: strip_comments(read(f"src/{f}")) for f in ("BirthForm.tsx", "MarriageForm.tsx", "DeathForm.tsx")}
    check("формы не восстанавливают причт сами — общий ClergyProvider",
          all('"last_clergy"' not in t and "useFormClergy" in t for t in forms.values()))
    clergy = strip_comments(read("src/clergy.tsx"))
    check("общий причт — последняя запись любого раздела", '"last_clergy", { section: 0 }' in clergy)
    check("«Запись пустая» и «Не указан год» — предупреждение, не поломка",
          all('report("Запись пустая"' not in t and 'report("Не указан год"' not in t for t in forms.values())
          and all('warn("Запись пустая"' in t for t in forms.values()))
    bar = read("src/ErrorBar.tsx")
    check("у предупреждения нет «пришлите текст»", "error.warn" in bar)
    stm = read("db/statements.sql")
    check("память персон без NULL в UNIQUE", "coalesce(:place, ''), coalesce(:rank, '')" in stm)
    check("обновление сливает старые дубли памяти персон", "DELETE FROM person_index" in read("db/migrate.sql"))
    check("память причта — тоже без NULL в UNIQUE (ревьюер)",
          "VALUES (:iof, :iof_norm, coalesce(:rank, ''), 1," in stm and "DELETE FROM clergy_index" in read("db/migrate.sql"))
    pb = strip_comments(read("src/PersonBlock.tsx"))
    check("«+ примечание» не внутри заголовка — e2e ищет h2 по тексту «Отец»",
          "{titleExtra}{noteLink" not in pb)
    e2e = read("scripts/e2e/windows.py")
    check("e2e ищет видимое поле (формы скрыты hidden)", "def shown(" in e2e and "return shown(driver" in e2e)
    app = strip_comments(read("src/App.tsx"))
    check("автопрокрутка только для полей — щелчок по кнопке не теряется (e2e #36)",
          'el.matches("input, textarea, select")' in app and ".savebar" in app)
    lock = read("package-lock.json")
    m = re.search(r'"node_modules/@tauri-apps/cli":\s*\{\s*"version":\s*"([\d.]+)"', lock)
    ver = m.group(1) if m else "?"
    check("@tauri-apps/cli не 2.11.* (устаревший STATIC_VCRUNTIME)", m is not None and not ver.startswith("2.11."), ver)


def incident_20260927_molchalivoe_sohranenie():
    """e2e сборки #36 (27.09.2026): «Сохранить изменения» после перезапуска
    ничего не сделала и ничего не сказала. Причина на стенде не
    воспроизведена; вероятная — автопрокрутка по фокусу на кнопке.

    Защита: автопрокрутка только для полей; непойманные ошибки — в полосу;
    e2e правит и сохраняет записи во всех трёх разделах и печатает
    подробности при провале. Окна — в body (portal), фокус из формы в окно
    не уводится, первые мгновения окно не принимает набор.
    """
    main = strip_comments(read("src/main.tsx"))
    check("непойманные ошибки — в полосу", "unhandledrejection" in main and "report(" in main)
    e2e = read("scripts/e2e/windows.py")
    check("e2e правит записи в браках и смертях",
          'edit_and_save(driver, wait, m, ".marriage"' in e2e and 'edit_and_save(driver, wait, d, ".death"' in e2e)
    check("e2e проверяет, что окно на экране", "окно целиком на экране по высоте" in e2e)
    modal = strip_comments(read("src/Modal.tsx"))
    check("окно — через createPortal в body", "createPortal(" in modal and "document.body" in modal)
    check("окно первые мгновения не принимает набор",
          "onKeyDownCapture" in modal and '"beforeinput"' in modal and "GUARD_MS" in modal)
    app = strip_comments(read("src/App.tsx"))
    check("Shift+Enter на «Сохранить» — назад, а не сохранение", 'e.key === "Enter" && e.shiftKey' in app)
    iof = strip_comments(read("src/IofField.tsx"))
    check("окно сверки снимается сразу (flushSync) — фокус не теряется", "flushSync(() => setResolve(null))" in iof)
    check("скрытая форма не открывает окно после проверки", "inputEl.current?.offsetParent == null" in iof)
    rust = strip_comments(rust_all())
    check("звания причта — в перечень rank_clergy", 'role_code.starts_with("clergy") { "rank_clergy" }' in rust)
    focus = strip_comments(read("src/focus.ts"))
    check("переход из формы при открытом окне не уводит фокус из окна",
          'document.querySelector(".modal")) return []' in focus)


def incident_20260928_spisok_vslepuyu():
    """Роман 28.09.2026: «в выпадающем списке ИОФ при использовании стрелок
    фокус перемещается на нижние строки, но сам список не прокручивается …
    пользователь выбирает элементы вслепую». У НП и званий (Suggest) и в окне
    сверки прокрутка была, у ИОФ — нет.

    Защита: активная строка списка ИОФ прокручивается в видимую область —
    и у персон, и у слов.
    """
    # С 01.10.2026 прокручивается сам список (scrollInList), а не страница.
    iof = strip_comments(read("src/IofField.tsx"))
    check("строки списка ИОФ прокручиваются за стрелками — и персоны, и слова",
          iof.count("scrollInList") >= 3)
    for f in ("src/Suggest.tsx", "src/NameResolve.tsx"):
        check(f"{f}: прокрутка к активной строке на месте", "scrollInList" in read(f))
    focus = strip_comments(read("src/focus.ts"))
    check("прокручивается список, а страница — только если строка за краем окна",
          "list.scrollTop" in focus and "window.innerHeight" in focus)


# Поломки, которые уже известны, но ещё не исправлены. Проверка приходит вместе
# с починкой — до этого момента инцидент живёт здесь и печатается при каждом
# прогоне, чтобы о нём нельзя было забыть. Пустой список — хорошая новость.
ОТКРЫТЫЕ = [
    # 13.09.2026: Роман ответил, что сворачивание причта удобно. Инцидентом это
    # больше не считается — кнопка выбора становится доступной всегда,
    # см. spec/2026-09-13-sem-pravok.md, п. 6.
]

def incident_20260930_python_314():
    """Mike 29.09.2026: db/test_parse.py упал на Mac с Python 3.14 —
    именованный параметр (:name_norm) получал значения списком. В 3.12 это
    было только предупреждением, и конвейер на 3.12 оставался зелёным.

    Защита: быстрая проверка в конвейере идёт на 3.14; в тестах базы
    запросы с именованными параметрами получают словарь.
    """
    wf = read(".github/workflows/build.yml")
    check_job = wf[wf.index("  check:"):wf.index("  build:")]
    check("быстрая проверка в конвейере — на Python 3.14",
          'python-version: "3.14"' in check_job)
    tp = read("db/test_parse.py")
    check("test_parse передаёт именованные параметры словарём",
          "db.execute(q, a).fetchone()" not in tp and "isinstance(a[0], dict)" in tp)


def incident_20260930_umershij_bez_imeni():
    """Роман 30.09.2026: запись о смерти неизвестного («тело неизвестного
    человека мужеского пола») нельзя было сохранить — сверка ИОФ требовала
    имя. И стенд #39 поймал первую версию флажка: локальная переменная
    unknown в save() закрывала флажок, и форма снова говорила «Запись пустая».

    Защита: флажок «личность не установлена» (nameless) пропускает пустой
    ИОФ; имени, совпадающего с локальными переменными save(), у него нет;
    выгрузка не теряет умершего без имени.
    """
    form = strip_comments(read("src/DeathForm.tsx"))
    check("флажок «личность не установлена» есть", "личность не установлена" in form
          and "const [nameless, setNameless]" in form)
    check("пустой ИОФ сохраняется при флажке", "if (!d.iof.trim() && !nameless)" in form)
    check("при «Открыть» флажок восстанавливается по пустому ИОФ", "setNameless(!!dm && !iof(dm))" in form)
    sql = read("db/statements.sql")
    check("лист «МК» не отбрасывает умершего без имени",
          "(x.iof_b <> '' OR x.role_code = 'deceased')" in sql)


def incident_20261001_excel_vosstanovlenie():
    """Роман 01.10.2026: выгрузка в Familio открывалась в Excel с «Ошибка в
    части содержимого… Выполнить попытку восстановления?». Причина:
    drop_children() в xlsx.rs, убирая запись о calcChain из
    [Content_Types].xml, собирала обёртку заново из одних <Override> и теряла
    все <Default> (типы .rels, .xml, .bin, .vml). openpyxl, Numbers и разбор
    XML в e2e это прощали — «проверено» было не тем, чем откроет человек.

    Защита: drop_children вырезает только нужные элементы; тест xlsx.rs
    сверяет число <Default> с образцом; e2e на Windows гоняет оба файла
    выгрузки через валидатор Open XML (scripts/xlsx-validate).
    """
    # Поведение проверяет тест крейта (fills_familio: число <Default> как в
    # образце) — и теперь он идёт в быстрой проверке конвейера, как и
    # валидатор Open XML на файлах, выгруженных из тестовой базы.
    rs = read("src-tauri/core/src/xlsx.rs")
    check("тест крейта сверяет <Default> с образцом",
          'elements(&ct, "Default").len(), elements(&template_ct, "Default").len()' in rs)
    wf = read(".github/workflows/build.yml")
    check_job = wf[wf.index("  check:"):wf.index("  build:")]
    check("тесты крейта идут в быстрой проверке конвейера", "cargo test -p genmetric-core" in check_job)
    check("быстрая проверка выгружает тестовую базу и гонит файлы через валидатор",
          "export_real" in check_job and check_job.count("scripts/xlsx-validate") >= 3)
    e2e = read("scripts/e2e/windows.py")
    check("e2e проверяет оба файла выгрузки валидатором Open XML",
          'validate_xlsx(path, "Familio"' in e2e and 'validate_xlsx(path, "Excel")' in e2e)
    check("валидатор в репозитории", "OpenXmlValidator" in read("scripts/xlsx-validate/Program.cs"))
    check("провал валидатора не превращается в пропуск",
          "валидатор не запустился" in e2e and "r.returncode == 0" in e2e)


def incident_20261001_pustye_mesta():
    """Роман 01.10.2026: в выгрузке Familio на листе location пусты тип,
    губерния, уезд, волость и ссылка, в листах данных пуст full_location, а
    person_location заполнен «лишь частично». Записи ссылались на строку
    пункта без подробностей: пункт пришёл одним названием (архив, набор
    руками), а подробности поставки до него не доезжали — либо строка поставки
    лежала рядом «двойником».

    Защита: обновление дозаполняет пункт без единой подробности; выгрузка
    берёт подробности у самой полной строки с тем же названием (x_place), а
    без них пишет название; place_find выбирает полную строку. Поведение —
    в db/test_upgrade.py и db/test_export.py (§10).
    """
    mig = read("db/migrate.sql")
    check("обновление дозаполняет подробности пунктов из поставки",
          "UPDATE OR IGNORE main.place" in mig and "FROM seed.place s" in mig)
    check("…и не трогает набранное руками полное место",
          "trim(coalesce(main.place.full_location, '')) = ''" in mig)
    sql = strip_comments(read("db/statements.sql"))
    check("выгрузка берёт лучшую строку пункта по названию", "CREATE TEMP TABLE x_place" in sql
          and "LEFT JOIN place p ON p.id = xp.best_id" in sql)
    check("полное место не бывает пустым при известном пункте",
          "coalesce(nullif(trim(p.full_location), ''), nullif(trim(p.short_location), ''), p.name) AS place_full" in sql)
    te = read("db/test_export.py")
    check("поведение проверяется в test_export (двойник и пункт без подробностей)",
          "двойник: person_location" in te and "пункт без подробностей: person_location" in te)
    check("…и в test_upgrade", "пункт из архива без подробностей получил их из поставки" in read("db/test_upgrade.py"))


def incident_20261002_odno_delo_na_vse_gody():
    """Разбор Excel Романа 02.10.2026: реквизиты дела (фонд, опись, дело) были
    одни на всю базу, а у каждого года книги своё архивное дело — у него 1889
    год «Ф.56 Оп.31 Д.11», 1890 — «Д.12». Выгрузка в Familio поставила бы всем
    годам одно дело; после импорта тринадцати лет — тем более.

    Защита: запись привязывает к делу своего года книги программа, а не форма
    (records.rs); новому году дело заводится копией с напоминанием; запросы
    «в своё дело» смотрят в приход. Поведение — db/test_entry.py (§9г),
    db/test_upgrade.py, db/test_export.py и тест импорта в крейте.
    """
    rec = strip_comments(read("src-tauri/core/src/records.rs"))
    check("дело записи выбирает программа по году книги",
          "case_for_year(conn, year)" in rec and "entry.rite_year.or(entry.event_year)" in rec)
    sql = strip_comments(read("db/statements.sql"))
    blocks = dict(re.findall(r"-- @(\w+)\n(.*?)(?=\n-- @|\Z)", read("db/statements.sql"), re.S))
    check("запросы дела на год есть", all(b in blocks for b in
          ("case_current", "case_years", "case_for_year", "case_adopt_year", "case_copy_for_year", "case_spread_parish")))
    check("правка записи переносит её в дело своего года", "case_id = :case_id" in blocks.get("entry_update", ""))
    for name in ("entry_list", "last_clergy", "birth_father", "infant_suggest"):
        check(f"«{name}» смотрит в приход, а не в дело года", ":case_id" not in strip_comments(blocks.get(name, ":case_id")))
    check("у установленной программы записи разных лет получают свои дела",
          "INSERT INTO mk_case" in read("db/migrate.sql") and "UPDATE entry SET case_id" in sql + read("db/migrate.sql"))
    forms = [strip_comments(read(f"src/{f}")) for f in ("BirthForm.tsx", "MarriageForm.tsx", "DeathForm.tsx")]
    check("формы не решают, к какому делу запись, и напоминают о новом годе",
          all("new_case_year" in t and "caseId: mkCase.id" not in t for t in forms))
    check("экран «Дело» показывает дело года и даёт выбрать другой",
          "case_years" in read("src/CaseHeader.tsx") and "Дело за ${loadedYear} год" in read("src/CaseHeader.tsx"))
    check("поведение проверено: test_entry, test_upgrade, test_export",
          "9г. Дело — на год книги" in read("db/test_entry.py")
          and "дело на год: у 1896 и 1897 годов" in read("db/test_upgrade.py")
          and "дело на год: у 1886 года" in read("db/test_export.py"))

    # Тот же класс, что 25.09 («тест, который не ходит через Rust»): окно зовёт
    # команду, которой в программе нет, — видно только в собранной программе.
    front = "".join(f.read_text(encoding="utf-8") for f in sorted((REPO / "src").glob("*.ts*")))
    called = set(re.findall(r'invoke(?:<[^(]*>)?\(\s*"(\w+)"', front))
    main_rs = read("src-tauri/src/main.rs")
    handler = main_rs[main_rs.index("generate_handler!["):]
    registered = set(re.findall(r"(?:\w+::)?(\w+)\s*[,\]]", handler[:handler.index("]")] + "]"))
    missing = sorted(called - registered)
    check("каждая команда, которую зовёт окно, зарегистрирована в программе", len(called) >= 30 and not missing,
          ", ".join(missing) or f"{len(called)} команд")


def incident_20261003_otvet_na_prihody():
    """Ответ Романа 03.10.2026 на сборку с приходами. Три вещи дошли до него:
    (1) порядок «сначала запись нового года, потом правка дела» — «запутался
    уже на этапе чтения описания», а привычный ему порядок молча затирал бы
    реквизиты прошлого года; (2) подсказка умершего-младенца выдала 19
    «Евдокий» — после импорта в приходе рождения за 13 лет; (3) при выборе
    младенца звание умершего оставалось пустым.

    Защита: «Сохранить дело» с новым годом заводит дело этого года, а чужой год
    переписывает только с согласия (records::save_case, тест крейта
    case_free_order); подсказка — за 2 года, по деревне и по имени родителя
    (db/test_entry.py); звание подставляется по полу ребёнка.
    """
    rec = strip_comments(read("src-tauri/core/src/records.rs"))
    check("новый год на экране «Дело» — новое дело, а не правка прежнего",
          "pub fn save_case" in rec and '"created"' in rec and '"exists"' in rec and "if !overwrite" in rec)
    check("три ветки сохранения дела проверены тестом крейта", "fn case_free_order" in read("src-tauri/core/src/records.rs"))
    ch = read("src/CaseHeader.tsx")
    check("год книги — поле на экране «Дело», чужой год — с вопросом",
          'label="Год книги"' in ch and 'r.status === "exists"' in ch and "save(true)" in ch)
    blocks = dict(re.findall(r"-- @(\w+)\n(.*?)(?=\n-- @|\Z)", read("db/statements.sql"), re.S))
    inf = strip_comments(blocks.get("infant_suggest", ""))
    check("подсказка младенца: окно 2 года, деревня, имя родителя",
          ":year - 2 AND :year" in inf and ":place" in inf and ":parent" in inf and ":year - 7" not in inf)
    check("отец по записи о рождении — в том же окне и деревне",
          ":year - 2 AND :year" in strip_comments(blocks.get("birth_father", "")) and ":place" in blocks.get("birth_father", ""))
    te = read("db/test_entry.py")
    check("поведение проверено в test_entry", "НП набран — только дети этой деревни" in te
          and "второе слово отбирает по началу имени родителя" in te)
    df = strip_comments(read("src/DeathForm.tsx"))
    check("звание младенца подставляется по полу", '"сын младенец"' in df and '"дочь младенец"' in df and "autoRank" in df)
    check("поля вне обхода Tab: флажок и месяцы обряда",
          "tabIndex={-1}" in read("src/DeathForm.tsx") and "noTab" in read("src/BirthForm.tsx") and "noTab" in df)


def incident_20261005_enter_i_uezd():
    """Ответ Романа 05.10.2026, две ошибки. (1) «Набрал „Ник“, выбрал „Николай“,
    Enter — фокус перепрыгивает в следующее поле»: после импорта в памяти
    персон лежат дети — «персоны» из одного имени, и строка «Николай»
    оказывалась персоной, выбор которой уводит фокус. (2) Уезд в карточке
    пункта не подсказывался: в перечне уездов было 4 значения, в справочнике
    пунктов — 11 («Юрьевецкий» отсутствовал).

    Защита: персона из одного слова без места и звания в подсказке не
    показывается и выбирается как слово; уезды и губернии пунктов попадают в
    перечни при сборке поставки (проверка — в verify_seed.py).
    """
    iof = strip_comments(read("src/IofField.tsx"))
    check("персона из одного имени не уводит фокус и не показывается строкой-двойником",
          "!hint.infant && !/\\s/.test(hint.iof.trim()) && !hint.place && !hint.rank" in iof
          and "knownPersons.filter((p) => /\\s/.test(p.iof.trim()) || p.place || p.rank)" in iof)
    check("уезды и губернии пунктов — в перечнях поставки",
          '("uyezd", "uyezd"), ("guberniya", "guberniya")' in read("db/build_seed.py"))
    check("…и это проверяет verify_seed", "«Юрьевецкий» подсказывается в карточке" in read("db/verify_seed.py"))
    check("поля, которые подставляются сами, пропускают все клавиши перехода",
          'hasAttribute("data-skip")' in read("src/focus.ts") and "data-skip" in read("src/NumberField.tsx"))


def incident_20261005_imya_ne_ukazano():
    """Роман 05.10.2026: имя, которого в книге нет, он ставил звёздочками
    («***»), а программа считала это именем вне словаря — в списке на сверку,
    а при наборе не дала бы сохранить запись («нельзя пропускать несуществующие
    имена»). И задача 8: мы поняли её как смену правила «в записи — как в
    книге» и подготовили опросник, а он просил лишь подсказывать уже связанные
    со словарём написания.

    Защита: «***» — системное слово (records.rs, тест no_name_and_surnames):
    разбор считает имя известным, в память персон и частоты оно не идёт, в
    Familio имя пустое; связанные написания — в запросах подсказки имён.
    """
    rec = read("src-tauri/core/src/records.rs")
    check("«***» — системное слово, а не имя вне словаря",
          'pub const NO_NAME: &str = "***"' in rec and "name_missing = true" in rec and "fn no_name_and_surnames" in rec)
    sql = read("db/statements.sql")
    check("в основных полях Familio у «***» имени нет", "'***') AS first_m" in sql)
    blocks = dict(re.findall(r"-- @(\w+)\n(.*?)(?=\n-- @|\Z)", sql, re.S))
    check("связанные человеком написания — в подсказке имени и отчества",
          "FROM name_alias" in blocks.get("suggest_first_name", "") and "FROM name_alias" in blocks.get("suggest_patronymic", ""))
    check("…и это проверено в test_suggest", "связанное написание имени предлагается" in read("db/test_suggest.py"))
    check("подсказки — по частоте в приходе, яруса «дело» нет",
          "WHEN 'case' THEN 1" not in strip_comments(blocks.get("suggest_ranked", "WHEN 'case' THEN 1")))


def incident_20261006_molcha_i_padenie():
    """Спринт «Надёжность и хвосты», 06.10.2026. До человека эти поломки не
    дошли — их нашли проверяющий и ревьюер на сборках 02.10–06.10, — но каждая
    была из уже знакомых классов: «программа сделала молча» и «упала, оставив
    мусор». С `panic = "abort"` в выпуске паника при разборе битого файла
    Excel закрывала программу без объяснения, а посреди импорта оставляла
    файл прихода без строки в перечне. Набор и импорт по-разному решали, что
    такое «имени нет». «—» превращалось в «***» только в базе, лишняя цифра
    дня пропадала, запись без года уходила в чужое дело — всё без единого
    слова человеку.

    Защита: перехват паники в крейте (тесты panic_becomes_error и
    open_create_and_share), одно правило имени и его копия в окне с общими
    примерами, пометка `fallback_case`, оговорка о годе-опечатке, имя копии
    по файлу прихода, запросы к делу — в statements.sql.
    """
    cargo = strip_comments(re.sub(r"(?m)#.*$", "", read("src-tauri/Cargo.toml")))
    check('в выпуске нет panic = "abort" — паника перехватывается', "panic" not in cargo, cargo[-120:].replace("\n", " | "))
    lib = read("src-tauri/core/src/lib.rs")
    check("перехват паники — в крейте и с тестом", "pub fn guarded<" in lib and "catch_unwind" in lib and "fn panic_becomes_error" in lib)
    parish = read("src-tauri/core/src/parish.rs")
    check("наполнение нового прихода идёт под перехватом, и это проверено",
          'crate::guarded("наполнение нового прихода"' in parish and 'panic!("разбор сломался")' in parish)
    main = read("src-tauri/src/main.rs")
    inspect = main.split("async fn import_inspect(", 1)[-1].split("\n}\n", 1)[0]
    check("разбор файла перед импортом — под перехватом", "genmetric_core::guarded(" in inspect)

    rec = read("src-tauri/core/src/records.rs")
    imp = read("src-tauri/core/src/import.rs")
    check("правило «имени нет» одно: импорт зовёт records::is_no_name",
          "all(is_no_name)" in imp and "is_alphabetic" not in strip_comments(imp))
    names = read("src/names.ts")
    check("копия правила в окне — с теми же примерами в обоих тестах",
          "export function noNameWord" in names and "records::is_no_name" in names
          and all(w in read("scripts/test_names.mjs") and w in rec for w in ('"Имярек"', '"ИМЯ"', '"—"')))
    check("заглушка заменяется на «***» в самом поле при уходе из него",
          "showNoName(titleCase(" in read("src/IofField.tsx"))

    body = rec.split("#[cfg(test)]", 1)[0]
    check("запросов к mk_case в records.rs нет — они в statements.sql",
          "mk_case" not in strip_comments(body), "литеральный запрос к делу вернулся в код")
    sql = read("db/statements.sql")
    check("блоки case_parish_key и case_or_first на месте", "-- @case_parish_key" in sql and "-- @case_or_first" in sql)
    check("запись без года в чужом деле помечена, и формы об этом говорят",
          "fallback_case: fallback.is_some()" in rec
          and all("done.fallback_case" in read(f"src/{f}.tsx") for f in ("BirthForm", "MarriageForm", "DeathForm")))
    check("год-опечатка при импорте: оговорка в отчёте и тест", "fn odd_years(" in imp and "fn odd_year_is_reported" in imp
          and "odd_years" in read("src/ParishDialog.tsx"))

    db = read("src-tauri/core/src/db.rs")
    check("копия перед обновлением называется по файлу прихода",
          'format!("{stem}{BACKUP_MARK}{seconds}.sqlite")' in db and "genmetric-до-обновления-{seconds}" not in db)
    check("копии и заменённые файлы приходами не становятся", "contains(crate::db::BACKUP_MARK)" in parish)
    migrate = read("db/migrate.sql")
    check("слияние двойников: лучшая строка — с подробностями; при тёзках пустая не сливается",
          "ORDER BY (b.sig = '||||')" in migrate and "s.kinds <= 1" in migrate
          and "при тёзках пустая строка не слита" in read("db/test_upgrade.py"))

    num = read("src/NumberField.tsx")
    check("поле числа не отбрасывает набранное молча", "reject()" in num and "onBlur" in num)
    check("…и не начинает год заново с одной цифры", "Number(tail) >= Math.max(1, min)" in num)
    check("выгрузки идут под перехватом паники", read("src-tauri/src/export.rs").count("genmetric_core::guarded(") == 2)
    check("окно списка на сверку не закрывается при занятой форме",
          "dirtyForms().includes(form)" in read("src/CaseHeader.tsx") and "data-review-busy" in read("src/ReviewDialog.tsx"))
    forms = [read(f"src/{f}.tsx") for f in ("BirthForm", "MarriageForm", "DeathForm")]
    check("год дела, сохранённый во время правки, форма подхватывает после неё",
          all("setYear(yearWhileEditing.current ?? b.year)" in f for f in forms))
    check("пустой список младенцев из-за НП объясняется", "data-infant-elsewhere" in read("src/IofField.tsx"))
    blocks = dict(re.findall(r"-- @(\w+)\n(.*?)(?=\n-- @|\Z)", sql, re.S))
    check("фамилии в подсказке — по полу, в обеих ветках запроса",
          ":gender = 'Ж'" in blocks.get("surname_gender_filter", "") and "FROM lookup pair" in blocks.get("surname_gender_filter", "")
          and lib.count("surname_gender_filter") >= 1
          and "fn suggest_sql_runs" in lib)

    export = read("src-tauri/src/export.rs")
    check("«Найти на Familio» открывает только адрес, собранный крейтом",
          "genmetric_core::familio_search_url(" in export and "pub fn open_familio(" in export
          and "url: String" not in export.split("pub fn open_familio(", 1)[-1].split(")", 1)[0])
    check("адрес поиска — title и georequisites (параметр search страница не читает)",
          "places?title=" in lib and "&georequisites=" in lib and "places?search=" not in strip_comments(lib))
    e2e = read("scripts/e2e/windows.py")
    check("e2e грузит файл импорта несколькими частями и сверяет число частей",
          "window.__genmetricChunk = 1024" in e2e and "data-parts" in e2e and "data-familio-find" in e2e)
    css = read("src/styles.css")
    check("шрифт Inter вшит: файлы, лицензия, строка на «О программе»",
          all((REPO / "src" / "fonts" / f).exists() for f in
              ("inter-cyrillic-wght-normal.woff2", "inter-cyrillic-ext-wght-normal.woff2",
               "inter-latin-wght-normal.woff2", "OFL.txt"))
          and 'font-family: "Inter", "Segoe UI"' in css and "SIL Open Font" in read("src/App.tsx"))
    check("«всегда в столбик» — общая настройка, плотная раскладка отключается одним правилом",
          '"ui_one_column"' in parish and ":root.onecol .formroot" in css)


def incident_20261006_otvet_na_sborku_05_10():
    """Роман 06.10.2026, ответ на сборку 05.10 — он уже ведёт в программе
    чистовую индексацию. У «Поручителя 4» «Никанор Орнатский» программа «никак
    не отреагировала на неизвестное второе слово»; невесте предлагались мужские
    отчества, поручителям — женские имена; у родственников в браках не было
    подсказки персон; у нижней персоны список подсказок «обрезается нижней
    границей окна»; у новой матери Enter останавливался на подставленном НП.
    И в справочниках подсказывались тестовые строки шаблона Excel
    («Петровская», «Галический», «Лодзинский»).

    Защита: под полем ИОФ всегда видно, как понято второе слово; пол задан у
    поручителей и учтён в связанных отчествах; список открывается вверх;
    тестовые строки убраны из поставки и уходят при обновлении, только если
    никем не заняты. Заодно — правила автоподстановок: они работают только в
    новой записи, запись на правке не трогают.
    """
    iof = read("src/IofField.tsx")
    check("под полем видно, что второе слово понято как фамилия", "data-surname-only" in iof and "без отчества, фамилия" in iof)
    marriage = read("src/MarriageForm.tsx")
    check("поручителям задан мужской пол — женских имён и отчеств не предлагают",
          re.search(r'title=\{`Поручитель \$\{i \+ 1\}`\}.*?gender="М"', marriage, re.S) is not None)
    check("у родственников в браках есть подсказка персон",
          marriage.count("onPickPerson={pickRelative(") == 2)
    sql = read("db/statements.sql")
    blocks = dict(re.findall(r"-- @(\w+)\n(.*?)(?=\n-- @|\Z)", sql, re.S))
    check("связанные человеком отчества предлагаются по полу",
          ":gender = 'Ж'" in blocks.get("suggest_patronymic", "") and "связанное отчество — по полу" in read("db/test_suggest.py"))
    focus = read("src/focus.ts")
    check("список подсказок у нижнего края открывается вверх, пол пересчитывается после прокрутки",
          'classList.add("up")' in focus and "floorNow()" in focus and ".suggest.up" in read("src/styles.css"))
    birth = read("src/BirthForm.tsx")
    check("Enter в ИОФ матери ведёт к первому пустому полю", "enterToEmpty" in birth and "focusNextEmptyField(el)" in iof)
    place_csv = read("db/seed/place.csv")
    check("тестовых строк шаблона в поставке нет",
          not any(w in place_csv for w in ("Лодзь", "Петровская", "Галический", "Котело")))
    migrate = read("db/migrate.sql")
    check("у установленных тестовые строки уходят, только если пункт не занят и не правлен",
          "name_norm = 'лодзь'" in migrate and "NOT EXISTS (SELECT 1 FROM main.person_mention m WHERE m.place_id = main.place.id)" in migrate
          and "updated_at IS NULL" in migrate and "«Котело» стоит в записи — остался" in read("db/test_upgrade.py"))
    check("…и не возвращаются из общего файла приходов", "name_norm = 'лодзь'" in read("db/parish_sync.sql"))

    # Автоподстановки — только в новой записи: «открыл, посмотрел, сохранил»
    # не должно менять старую запись.
    death = read("src/DeathForm.tsx")
    check("звание по умолчанию выключено для записи на правке",
          'defaultRank={editingId === null ? "father" : undefined}' in birth
          and birth.count("defaultRank={godRank}") == 4
          and 'const godRank = editingId === null && autoRankGod ? "godparent" as const : undefined;' in birth
          and 'defaultRank={fresh ? "groom" : undefined}' in marriage
          and 'defaultRank={fresh && autoRankWitness ? "witness" : undefined}' in marriage
          and "const fresh = editingId === null;" in marriage)
    check("НП поручителя, НП родственника младенца и фамилия матери — не в записи на правке",
          "if (editingId !== null) return;" in marriage.split("witnessPlaceManual = useRef", 1)[-1][:400]
          and "editingId !== null || relPlaceManual.current" in death
          and "if (editingId !== null) return;" in birth.split("async function appendFathersSurname()", 1)[-1][:120])
    check("сохранение дожидается дописывания фамилии матери (Ctrl+Enter из её поля)",
          "await motherLeaving.current;" in birth and "mother = latestMother.current;" in birth)
    check("поручитель из подсказки со своим НП — «набран руками»",
          "if (hint.place) witnessPlaceManual.current[i] = true;" in marriage)
    check("строки шаблона не идут в частоты; чистка частот в миграции — только этих значений",
          "NOT (kind = 'guberniya' AND value = 'Петровская')" in blocks.get("place_field_counts", "")
          and "WHERE kind IN ('guberniya', 'uyezd', 'volost')" not in migrate)
    block = read("src/PersonBlock.tsx")
    check("звание по роли и полу, а не по общим частотам перечня",
          "role_code LIKE :role" in blocks.get("rank_default", "") and 'invoke<string | null>("rank_default"' in block)
    check("правки блока персоны идут от последнего состояния — подстановки не затирают друг друга",
          "latest.current = next;" in block and "push({ ...latest.current, ...patch })" in block)
    rec = read("src-tauri/core/src/records.rs")
    names = read("src/names.ts")
    check("слова-заглушки одни и те же в крейте и в окне",
          all(f'"{w}"' in rec.split("NO_NAME_WORDS: &[&str]", 1)[-1][:160] and f'"{w}"' in names.split("NO_NAME_WORDS = [", 1)[-1][:160]
              for w in ("имя", "нрзб", "н/д", "неизвестно", "неизв", "нет", "б/и")))
    check("вероисповедание и поля карточки пункта идут в частоты",
          'remember(conn, "confession"' in rec and "pub fn remember_card" in rec and "pub fn seed_usage" in rec
          and "seed_usage(&conn)" in read("src-tauri/core/src/parish.rs"))
    conf = read("src-tauri/tauri.conf.json")
    check("текст лицензии шрифта едет в установщике и совпадает с исходным",
          '"resources/Inter-OFL.txt"' in conf and read("src-tauri/resources/Inter-OFL.txt") == read("src/fonts/OFL.txt")
          and read("src/fonts/OFL.txt") != "")


def incident_20261007_otvet_na_sborku_06_10():
    """
    07.10.2026, ответ Романа на сборку 06.10 (intent/2026-10-07…): звание по
    умолчанию у восприемников мешает — «постоянно приходится стирать и
    исправлять данные за программой» (на его данных самое частое звание
    подходит восприемнику в 31 % случаев); словарные имена «отодвигают»
    персон в подсказке; НП умершего не переходил родственнику, если умерший
    набран, а не выбран; причт — первый только «священник».
    Спека 2026-10-08-nabor-bez-stiraniya.
    """
    birth, death, marriage = read("src/BirthForm.tsx"), read("src/DeathForm.tsx"), read("src/MarriageForm.tsx")
    flags = read("src/flags.ts")
    parish = read("src-tauri/core/src/parish.rs")
    common = parish.split("COMMON_SETTINGS: &[&str] = &[", 1)[-1].split("]", 1)[0]
    check("выключатели звания помнятся в приходе, а не в общих настройках окна",
          '"auto_rank_godparent"' in flags and '"auto_rank_witness"' in flags and "auto_rank" not in common)
    check("выключатель изначально включён: выключено — только явное «0»", 'setOn(v !== "0")' in flags)
    check("флажки звания — вне обхода клавишами",
          birth.count("tabIndex={-1} data-auto-rank") == 1 and marriage.count("tabIndex={-1} data-auto-rank") == 1)
    iof = read("src/IofField.tsx")
    check("подсказка ИОФ: есть персоны — одна словарная строка",
          "early && foundPersons.length > 0 ? foundWords.slice(0, 1) : foundWords" in iof)
    check("НП умершего идёт родственнику у любого умершего, не только у младенца",
          "!/младен/i.test(dead.rank)" not in death and "editingId !== null || relPlaceManual.current" in death)
    check("правило младенца — не у записи на правке и не поверх звания, тронутого руками",
          "if (editingId !== null || nameless || rankManual.current) return null;" in death)
    check("…и срабатывает при Ctrl+Enter прямо из ИОФ (итог не зависит от клавиши сохранения)",
          "const babyRank = babyRankFor(d, sex);" in death.split("async function save()", 1)[-1][:2600])
    check("подставленное формой звание и НП умершего блок персоны не затирает своей копией",
          "rank: p.rank === prev.rank ? s.rank : p.rank," in death and "place: p.place === prev.place ? s.place : p.place," in death)
    check("НП родственника, подставленный от умершего, не мешает подставить отца выбранного ребёнка",
          death.count("relPlaceFree(") == 2 and "!relPlaceManual.current && r.place === latestDead.current.place" in death)
    check("правило младенца смотрит на возраст: взрослому с одним именем «младенец» не пишется",
          ">= BABY_MAX_YEARS" in death.split("function babyRankFor", 1)[-1][:700])
    sug = read("src/Suggest.tsx")
    check("регистр при уходе из поля правится, только если в поле набирали (старое написание не меняется)",
          "if (fix && wasTyped && pickedValue.current === null && real) {" in sug and "typedHere.current = true;" in sug)
    check("счёт — от наибольшего номера пола, а не от последней набранной записи",
          "top = Math.max(top, (of === \"М\" ? e.no_male : e.no_female) ?? 0);" in read("src/count.ts"))
    for name, form in (("рождения", birth), ("смерти", death)):
        check(f"{name}: сохранённая запись сразу попадает в список, по которому считается счёт",
              "if (editingId === null)\n        setSaved((l) => [{ id: done.id, page, no_male: columns.no_male" in form)
        check(f"{name}: счёт ставится сам по полу и не трогается у записи на правке и после набора руками",
              "if (editingId !== null || countManual.current) return;" in form and "nextCount(saved," in form
              and "countManual.current = false;" in form.split("function next()", 1)[-1][:1400])
        check(f"{name}: при сохранении счёт считается тем же правилом (Ctrl+Enter сразу после имени)",
              "editingId === null && !countManual.current ? nextCount(saved, " in form.split("async function save()", 1)[-1])
        check(f"{name}: поле «Счёт» вне обхода клавишами",
              "onChange={typeCount}" in form and "noTab" in form.split("onChange={typeCount}", 1)[-1][:120])
    clergy = read("src/ClergyBlock.tsx")
    check("причт: первое место — только «священник», остальные — все прочие",
          'const PRIEST = "священник";' in clergy
          and "known.filter((h) => (slot === 0 ? isPriest(h.rank) : !isPriest(h.rank)))" in clergy)
    check("причт: тот, кто стоит в записи, из выпадающего списка не пропадает",
          "{nowKey && !listed && (" in clergy)
    check("причт: списки не исчезают, когда выбрано «никого», а выбирать есть из кого",
          "if (!open && filled.length === 0 && known.length === 0) {" in clergy)
    check("причт: примечание прежнего человека не переезжает к выбранному",
          'rank: hint.rank ?? "", note: "" });' in clergy)
    focus = read("src/focus.ts")
    check("обход клавишами знает выпадающие списки и не зовёт у них select()",
          'FIELDS = "input[data-field], select[data-field]"' in focus and 'typeof el.select === "function"' in focus)
    check("«всегда в столбик» изначально включено и стоит на «Деле»",
          'setOn(value !== "0");' in read("src/OneColumn.tsx") and "{viewBlock}" in read("src/CaseHeader.tsx")
          and "<OneColumn" not in read("src/App.tsx").split('{screen === "about" && info && (', 1)[-1])
    block = read("src/PersonBlock.tsx")
    check("заглавная буква в НП — с исключением для названий с типа пункта",
          "fix={(text, final) => placeCase(text, placeTypes(), final)}" in block)
    check("предохранитель страницы — не у записи на правке, во всех трёх формах",
          all("pageOverflow(saved.map((e) => e.page), page)" in f for f in (birth, death, marriage))
          and "const pageGuard = editingId === null ?" in birth and "const pageGuard = fresh ?" in marriage)


ИНЦИДЕНТЫ = [
    incident_20261007_otvet_na_sborku_06_10,
    incident_20261006_otvet_na_sborku_05_10,
    incident_20261006_molcha_i_padenie,
    incident_20261005_imya_ne_ukazano,
    incident_20261005_enter_i_uezd,
    incident_20261003_otvet_na_prihody,
    incident_20261002_odno_delo_na_vse_gody,
    incident_20261001_pustye_mesta,
    incident_20261001_excel_vosstanovlenie,
    incident_20260930_umershij_bez_imeni,
    incident_20260930_python_314,
    incident_20260928_spisok_vslepuyu,
    incident_20260927_molchalivoe_sohranenie,
    incident_20260927_otchyot_27_09,
    incident_20260925_lishnij_parametr,
    incident_20260925_otchestvo_bez_familii,
    incident_20260924_sverka_ne_vezde,
    incident_20260924_pravka_sterla_pometku,
    incident_20260813_baza_ne_doehala,
    incident_20260813_molchalivyj_perehvat,
    incident_20260817_kirillica_i_lower,
    incident_20260824_np_iskalos_ne_tam,
    incident_20260827_spisok_ne_pryatalsya,
    incident_20260827_vypusk_bez_ustanovshchikov,
    incident_20260828_pricht_dopushchenie,
    incident_20260828_pravilo_ispolneno_bukvalno,
    incident_20260913_schyot_v_muzhskuyu_kolonku,
    incident_20260913_ctrl_enter_pri_otkrytom_spiske,
    incident_20260914_arhiv_ne_gruzitsya_na_windows,
    incident_20260915_neskolko_spiskov_razom,
    incident_20260918_razbor_stiraet_poslednyuyu_bukvu,
    incident_20260922_prichjt_bez_imeni,
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
