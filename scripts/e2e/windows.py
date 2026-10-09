#!/usr/bin/env python3
"""
Сквозная проверка собранного приложения на настоящем Windows.

Зачем. 13.09.2026 Роман получил сборку, в которой кнопка «Загрузить архив»
не работала: сырое тело IPC-запроса на WebView2 не доходило. В песочнице
это не воспроизвести — стенд не Tauri, cargo build невозможен. Единственное
место, где есть настоящий Windows с настоящим WebView2, — раннер конвейера.
Поэтому здесь: запускаем собранный genmetric.exe сами, с портом отладки
WebView2, подключаем к нему msedgedriver (подход «attach» из документации
Microsoft) и проходим путь Романа руками робота.

Почему не tauri-driver (подход «launch»): 14.09.2026 первый прогон упал на
создании сессии — «DevToolsActivePort file doesn't exist», без единой
строки от драйвера, и понять, запустилось ли вообще приложение, было нельзя.
Здесь приложение запускается явно: видно, живо ли оно, и его журнал.

Что проверяется:
  1. окно открылось, база справочников подключилась (нет экрана «База не открылась»);
  2. дело заполняется и сохраняется;
  3. архив загружается через <input type=file> и настоящий IPC: под кнопкой
     появляется «Добавлено: персон …»;
  4. подсказка отца видит персону из архива;
  5. запись девочки сохраняется, и в списке «Набрано» у неё «№ ж.»;
  6. после перезапуска приложения форма продолжает с места; счёт ставится
     сам по полу ребёнка, причт — выпадающими списками;
  7. сохранённая запись открывается в форму, правится и сохраняется без дублей;
  …
  13. умерший без имени («личность не установлена») сохраняется;
  14. выгрузка в Familio и в Excel пишет файлы, в них набранные записи;
  15. новый приход — отдельный файл: записи не смешиваются;
  16. импорт Excel-индексатора (db/fixtures/indexer.xlsx) через окно, список
      на сверку; свободный порядок на экране «Дело».

Запуск (в конвейере, см. .github/workflows/build.yml):
    python scripts/e2e/windows.py путь\\к\\genmetric.exe путь\\к\\архив.sqlite путь\\к\\msedgedriver.exe

При провале рядом остаётся снимок e2e-failure.png — его конвейер
выкладывает артефактом.
"""

import os
import subprocess
import sys
import time
from pathlib import Path
from urllib.parse import quote

from selenium import webdriver
from selenium.common.exceptions import (NoSuchElementException, StaleElementReferenceException,
                                        TimeoutException, WebDriverException)
from selenium.webdriver.common.by import By
from selenium.webdriver.common.action_chains import ActionChains
from selenium.webdriver.common.keys import Keys
from selenium.webdriver.edge.options import Options as EdgeOptions
from selenium.webdriver.edge.service import Service as EdgeService
from selenium.webdriver.support import expected_conditions as EC
from selenium.webdriver.support.ui import WebDriverWait

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


def shown(driver, xpath):
    """Первый ВИДИМЫЙ элемент по xpath (техдолг В9, 27.09.2026).

    Формы рождений, браков и смертей живут в окне все сразу, скрытые — через
    hidden. «Год» или «Счёт» по одной подписи находились в первой форме по
    DOM, даже если на экране другая; сценарий держался на точных scope.
    """
    els = driver.find_elements(By.XPATH, xpath)
    for el in els:
        if el.is_displayed():
            return el
    raise NoSuchElementException(f"видимого элемента нет ({len(els)} скрытых): {xpath}")


def field(driver, label, scope="//"):
    """Поле формы по подписи: <div class="field"><label>НП</label>…<input>."""
    return shown(driver, f"{scope}div[contains(@class,'field')][./label[normalize-space()='{label}']]//input")


def click(driver, xpath):
    """Клик по кнопке в середине окна. WebDriver сам прокручивает элемент
    к нижнему краю — а там прилипшая панель «Сохранить и следующая», и клик
    уходит в неё (сборка #30: «element click intercepted»)."""
    el = shown(driver, xpath)
    driver.execute_script("arguments[0].scrollIntoView({block: 'center'});", el)
    el.click()


def error_details(driver):
    """Текст полосы ошибок вместе с «Подробностями» — чтобы по журналу e2e
    было видно причину, а не только заголовок (сборка #36, шаг 7)."""
    out = []
    for bar in driver.find_elements(By.CSS_SELECTOR, ".errorbar"):
        for b in bar.find_elements(By.XPATH, ".//button[normalize-space()='Подробности']"):
            b.click()
        out.append(bar.text.replace("\n", " | "))
    return " || ".join(out) or "полосы нет"


def clergy_shown(driver, css):
    """Кто стоит в причте формы `css` — и в развёрнутом (поля), и в свёрнутом
    (выпадающие списки, 08.10.2026) виде. Текст выбранной строки списка в
    `.text` элемента может не попасть — читаем значения напрямую."""
    return driver.execute_script(
        "const f = document.querySelector(arguments[0]); if (!f) return '';"
        "const sel = [...f.querySelectorAll('select[data-clergy]')]"
        "  .map((s) => (s.selectedOptions[0] || {}).textContent || '');"
        "const inp = [...f.querySelectorAll('.clergyslot input')].map((i) => i.value);"
        "return sel.concat(inp).join(' | ');", css) or ""


def edit_and_save(driver, wait, scope, css, count, expect):
    """Открыть первую запись списка формы, сменить счёт, «Сохранить изменения».

    Ловушка на сбой #36: e2e нажал «Сохранить изменения», и ничего не
    произошло — ни сохранения, ни сообщения. Теперь то же — в браках и
    смертях, сразу после открытия записи, без пауз.
    """
    click(driver, f"({scope}table[contains(@class,'saved')]//button[starts-with(normalize-space(),'Открыть')])[1]")
    wait.until(EC.presence_of_element_located((By.CSS_SELECTOR, f"{css} .editbar")))
    fill(driver, "Счёт", str(count), scope)
    click(driver, f"{scope}button[starts-with(normalize-space(),'Сохранить изменения')]")
    try:
        wait.until(EC.text_to_be_present_in_element((By.CSS_SELECTOR, css), expect))
    except TimeoutException:
        pass
    block = driver.find_element(By.CSS_SELECTOR, css).text
    check(f"правка сохранена: «{expect}»", expect in block, error_details(driver))
    check("режим правки снят", not driver.find_elements(By.CSS_SELECTOR, f"{css} .editbar"))


def fill(driver, label, value, scope="//"):
    el = field(driver, label, scope)
    el.clear()
    el.send_keys(value)
    # clear() у WebDriver React не будит: если поле перерисовалось между
    # очисткой и набором (счёт ставится сам, НП подставляется), прежнее
    # значение вернулось бы и набранное дописалось к нему. Сверяем и, если не
    # сошлось, набираем заново поверх выделенного — как человек (техдолг
    # после сборки 08.10.2026). Регистр программа правит сама — его не сверяем.
    got = el.get_attribute("value") or ""
    if got.strip().lower() != str(value).strip().lower():
        print(f"  [инфо]   поле «{label}»: вместо «{value}» оказалось «{got}» — набираю заново")
        el.send_keys(Keys.CONTROL, "a")
        el.send_keys(value)
    el.send_keys(Keys.ESCAPE)  # закрыть подсказку, если открылась


def village_card(driver, wait, name):
    """Карточка села прихода: открывается сама после первого сохранения дела
    нового прихода (Роман 06.10.2026). Сохраняем её — это и проверка того,
    что карточка с новым полем комментария проходит через настоящий Rust."""
    try:
        wait.until(EC.presence_of_element_located((By.CSS_SELECTOR, ".modal[data-modal='place']")))
        time.sleep(0.5)  # окно первые 350 мс не принимает набор (Modal.GUARD_MS)
        opened = True
    except TimeoutException:
        opened = False
    check(f"после первого сохранения дела открылась карточка села «{name}»", opened, error_details(driver))
    if not opened:
        return
    modal = "//div[contains(@class,'modal')]//"
    kind = field(driver, "Тип", modal).get_attribute("value")
    gub = field(driver, "Губерния", modal).get_attribute("value")
    check("в карточке села тип «с.», губерния из дела", kind == "с." and gub == "Костромская", f"«{kind}», «{gub}»")
    click(driver, modal + "button[normalize-space()='Сохранить населённый пункт']")
    try:
        wait.until(EC.invisibility_of_element_located((By.CSS_SELECTOR, ".modal")))
    except TimeoutException:
        pass
    driver.set_script_timeout(20)
    got = driver.execute_async_script(
        "const done = arguments[arguments.length - 1];"
        "window.__TAURI_INTERNALS__.invoke('place_get', {name: arguments[0]})"
        ".then((r) => done(r), (e) => done({error: String(e)}));", name)
    check("село заведено пунктом: тип и чистое название на месте",
          bool(got) and "error" not in got and got.get("np_type") == "с." and got.get("clean") == name
          and got.get("comment") == "" and not driver.find_elements(By.CSS_SELECTOR, ".modal"),
          str(got)[:300] + " | " + error_details(driver))


def run(driver, wait, archive):
    """Путь Романа: дело → архив → подсказка → запись."""
    print("\n1. Окно и база")
    wait.until(EC.presence_of_element_located((By.CSS_SELECTOR, ".app")))
    body = driver.find_element(By.TAG_NAME, "body").text
    check("база справочников открылась", "База не открылась" not in body)
    check("экран «Дело» на месте", "Сохранить дело" in body)
    # Окно по высоте экрана (заказчик 24.09.2026): целиком в рабочей области
    # и занимает её почти всю. Раньше только печаталось (техдолг после #36).
    # screen.availTop — верх рабочей области (панель задач сверху бывает).
    sizes = driver.execute_script(
        "return [window.outerHeight, screen.availHeight, window.screenY, screen.availTop || 0]")
    height, avail, top, avail_top = sizes
    print(f"  [инфо]   окно: высота {height}, рабочая область экрана {avail}, верх {top}")
    check("окно целиком на экране по высоте",
          top >= avail_top - 12 and top + height <= avail_top + avail + 12,  # развёрнутое окно: рамка −8, масштаб 125% — ±1 px
          f"верх {top}, низ {top + height}, рабочая область {avail_top}…{avail_top + avail}")
    check("окно занимает почти всю высоту рабочей области", height >= avail * 0.85,
          f"{height} из {avail}")
    # Шрифт Inter вшит в программу (06.10.2026): файлы лежат среди ресурсов
    # окна, и грузит их WebView2 под правилами безопасности из tauri.conf.json
    # — стенд этого не видит.
    driver.set_script_timeout(20)
    fonts = driver.execute_async_script(
        "const done = arguments[arguments.length - 1];"
        "document.fonts.ready.then(() => done([getComputedStyle(document.body).fontFamily,"
        " [...document.fonts].filter(f => f.family.replace(/[\"']/g, '') === 'Inter' && f.status === 'loaded').length,"
        " [...document.fonts].filter(f => f.status === 'error').length]));")
    check("шрифт Inter загружен в настоящем окне", fonts[0].replace('"', "").startswith("Inter") and fonts[1] >= 1 and fonts[2] == 0,
          f"семейство «{fonts[0][:40]}», загружено начертаний {fonts[1]}, с ошибкой {fonts[2]}")

    print("\n2. Дело")
    for label, value in [("Архив", "ГА Костромской области"), ("Церковь", "Христорождественская"),
                         ("Село", "Борисоглебское"), ("Уезд", "Макарьевский"),
                         ("Губерния", "Костромская")]:
        fill(driver, label, value)
    # Чистая поставка (08.10.2026): пунктов в установщике нет — чужая деревня
    # прежней поставки программе неизвестна.
    driver.set_script_timeout(20)
    clean = driver.execute_async_script(
        "const done = arguments[arguments.length - 1];"
        "const call = window.__TAURI_INTERNALS__.invoke;"
        "Promise.all([call('place_check', {name: 'Аксениха'}), call('suggest', {kind: 'place', prefix: '', limit: 200}),"
        "             call('suggest', {kind: 'rank_m', prefix: '', limit: 200})])"
        ".then((r) => done({check: r[0], places: r[1].length, ranks: r[2].length}), (e) => done({error: String(e)}));")
    check("поставка чистая: пунктов нет, мужских званий 51",
          "error" not in clean and clean["check"].get("known") is False and clean["places"] == 0 and clean["ranks"] == 51,
          str(clean)[:300])
    driver.find_element(By.XPATH, "//button[normalize-space()='Сохранить дело']").click()
    wait.until(EC.text_to_be_present_in_element((By.TAG_NAME, "body"), "Сохранено"))
    check("дело сохранено", True)
    village_card(driver, wait, "Борисоглебское")

    print("\n3. Архив через настоящий IPC")
    # С 05.10.2026 блок архива — на экране «ⓘ О программе», не на «Деле».
    click(driver, "//button[@aria-label='О программе']")
    wait.until(EC.presence_of_element_located((By.CSS_SELECTOR, ".archive input[type=file]")))
    file_input = driver.find_element(By.CSS_SELECTOR, ".archive input[type=file]")
    # Поле спрятано (кнопка вместо него); WebDriver кормит файл только видимому.
    driver.execute_script("arguments[0].hidden = false;", file_input)
    file_input.send_keys(str(archive))
    try:
        wait.until(EC.text_to_be_present_in_element((By.CSS_SELECTOR, ".archive"), "Добавлено:"))
    except TimeoutException:
        pass
    block = driver.find_element(By.CSS_SELECTOR, ".archive").text
    errorbar = [e.text for e in driver.find_elements(By.CSS_SELECTOR, ".errorbar")]
    check("под кнопкой появилось «Добавлено: персон 4»", "Добавлено: персон 4" in block,
          block.replace("\n", " | ") + (" || ОШИБКА: " + " | ".join(errorbar) if errorbar else ""))
    check("надпись сменилась на «Загружен:»", "Загружен:" in block)
    check("полосы ошибок нет", not errorbar, " | ".join(errorbar))

    print("\n4. Подсказка видит архив")
    driver.find_element(By.XPATH, "//nav//button[normalize-space()='Рождения']").click()
    wait.until(EC.presence_of_element_located((By.XPATH, "//section[.//h2[normalize-space()='Отец']]")))
    father = "//section[.//h2[normalize-space()='Отец']]//"
    iof = field(driver, "ИОФ", father)
    iof.send_keys("Ник")
    try:
        wait.until(EC.text_to_be_present_in_element((By.CSS_SELECTOR, ".suggest"), "Никита Алексеев"))
        found = True
    except TimeoutException:
        found = False
    check("отцу предлагается «Никита Алексеев» из архива", found,
          "" if found else " | ".join(e.text for e in driver.find_elements(By.CSS_SELECTOR, ".suggest")))
    iof.send_keys(Keys.ESCAPE)
    # clear() у WebDriver не будит React; стираем клавишами, чтобы отец
    # не остался «Ник» в состоянии формы.
    iof.send_keys(Keys.CONTROL, "a")
    iof.send_keys(Keys.BACKSPACE)

    print("\n5. Запись девочки — номер в женскую колонку")
    # Страница-разворот и шаг по ней (заказчик 21.09.2026).
    fill(driver, "Стр.", "938об-939")
    click(driver, "//div[contains(@class,'field')][./label[normalize-space()='Стр.']]//button[@aria-label='Больше']")
    page = field(driver, "Стр.").get_attribute("value")
    check("«+» на странице 938об-939 даёт 939об-940", page == "939об-940", f"«{page}»")
    # Год — только на форме (заказчик 22.09.2026: с «Дела» убран).
    fill(driver, "Год", "1897")
    fill(driver, "Счёт", "7")
    fill(driver, "Ребёнок", "Мария")
    # Четвёртый восприемник — кнопкой, дважды.
    for _ in range(2):
        click(driver, "//button[normalize-space()='Добавить восприемника']")
    god4 = "//section[.//h2[normalize-space()='Восприемник 4']]//"
    field(driver, "ИОФ", god4).send_keys("Пётр Сидоров")
    field(driver, "ИОФ", god4).send_keys(Keys.ESCAPE)
    # Причт — чтобы было что восстанавливать после перезапуска.
    clergy1 = "(//div[contains(@class,'clergyslot')])[1]//div[contains(@class,'field')][./label[normalize-space()='ИОФ']]//"
    driver.find_element(By.XPATH, clergy1 + "input").send_keys("Александр Рождественский")
    driver.find_element(By.XPATH, clergy1 + "input").send_keys(Keys.ESCAPE)
    # С 27.09.2026 у причта та же сверка ИОФ, что у всех: если второе слово
    # сочтут отчеством, откроется окно — отвечаем «Это не отчество».
    driver.find_element(By.XPATH, clergy1 + "input").send_keys(Keys.TAB)
    time.sleep(0.8)
    not_patr = driver.find_elements(By.XPATH, "//div[contains(@class,'modal')]//button[normalize-space()='Это не отчество']")
    if not_patr:
        not_patr[0].click()
        time.sleep(0.5)
    check("после причта окон сверки нет", not driver.find_elements(By.CSS_SELECTOR, ".modal"))
    # Пол приходит асинхронно (parse_iof). Нажать «Сохранить» раньше —
    # форма попросит выбрать пол и не сохранит. Ждём метку «Ж» под полем.
    child = "//div[contains(@class,'field')][./label[normalize-space()='Ребёнок']]"
    try:
        wait.until(EC.text_to_be_present_in_element((By.XPATH, child + "//*[contains(@class,'parsedline')]"), "Ж"))
    except TimeoutException:
        # Не гадать по «элемент не найден» (сборка #27): показать, что в поле.
        value = field(driver, "Ребёнок").get_attribute("value")
        under = driver.find_element(By.XPATH, child).text.replace("\n", " | ")
        errorbar = " | ".join(e.text for e in driver.find_elements(By.CSS_SELECTOR, ".errorbar"))
        check("под полем «Ребёнок» появилась метка пола «Ж»", False,
              f"в поле «{value}», под ним «{under}», полоса ошибок: {errorbar or 'нет'}")
        return
    # В кнопке ещё <span class="kbd">Ctrl+Enter</span>, поэтому starts-with.
    click(driver, "//button[starts-with(normalize-space(),'Сохранить и следующая')]")
    try:
        wait.until(EC.text_to_be_present_in_element((By.TAG_NAME, "body"), "Набрано: 1"))
    except TimeoutException:
        pass
    body = driver.find_element(By.TAG_NAME, "body").text
    errorbar = [e.text for e in driver.find_elements(By.CSS_SELECTOR, ".errorbar")]
    check("запись сохранена", "Набрано: 1" in body, " | ".join(errorbar))
    check("у девочки «№ ж. 7»", "№ ж. 7" in body)
    check("в мужской колонке ничего", "№ м." not in body)

    print("\n5б. Новые команды спринта 06.10 — через настоящий Rust")
    # Привязку параметров запросов в Rust Python-тесты не видят (грабли
    # 26.09.2026). Команды зовём напрямую, как их зовёт окно: звание по
    # умолчанию, отметка новой фамилии, волость последнего пункта.
    driver.set_script_timeout(20)
    called = driver.execute_async_script(
        "const done = arguments[arguments.length - 1];"
        "const call = window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke;"
        "if (!call) { done({error: 'нет window.__TAURI_INTERNALS__.invoke'}); return; }"
        "Promise.all(["
        "  call('rank_default', {role: 'father', gender: 'М'}),"
        "  call('rank_default', {role: 'godparent', gender: 'Ж'}),"
        "  call('parse_iof', {text: 'Иван Петров Небывалов'}),"
        "  call('place_check', {name: 'Несуществующее Тестовое'}),"
        "]).then((r) => done({rank: r[0], rank_f: r[1], parsed: r[2], place: r[3]}),"
        "        (e) => done({error: String(e)}));")
    check("звание по умолчанию, разбор ИОФ и проверка пункта отвечают без ошибки",
          "error" not in called, str(called)[:300])
    if "error" not in called:
        check("отметка новой фамилии приходит из Rust",
              called["parsed"].get("surname") == "Небывалов" and called["parsed"].get("surname_new") is True,
              str(called["parsed"])[:200])
        check("у проверки пункта есть волость последнего заведённого",
              called["place"].get("known") is False and isinstance(called["place"].get("last_volost"), str),
              str(called["place"])[:200])
        check("звание по умолчанию — строка или пусто", called["rank"] is None or isinstance(called["rank"], str),
              str(called["rank"]))

    print("\n5в. Новое в сборках 08.10 — через настоящий Rust")
    # Флажок «звание само» — настройка ПРИХОДА (не общая): пишется и читается
    # через те же команды, что зовёт окно. Пункт с комментарием деревни-тёзки:
    # название в программе с комментарием, чистое — отдельно; набранное без
    # комментария находит тёзку первой среди похожих. Подсказка персон
    # принимает новый параметр «звание жены как есть».
    called = driver.execute_async_script(
        "const done = arguments[arguments.length - 1];"
        "const call = window.__TAURI_INTERNALS__.invoke;"
        "(async () => {"
        "  await call('set_setting', {key: 'auto_rank_godparent', value: '0'});"
        "  const off = await call('get_setting', {key: 'auto_rank_godparent'});"
        "  await call('set_setting', {key: 'auto_rank_godparent', value: '1'});"
        "  const on = await call('get_setting', {key: 'auto_rank_godparent'});"
        "  await call('place_save', {card: {name: 'Заборье', comment: 'Столпино', np_type: 'д.',"
        "    guberniya: 'Костромская', uyezd: 'Макарьевский', volost: '', familio_url: ''}});"
        "  const got = await call('place_get', {name: 'Заборье (Столпино)'});"
        "  const twin = await call('place_check', {name: 'Заборье'});"
        "  await call('place_update', {id: got.id, card: {name: 'Заборье', comment: 'Нежитино', np_type: 'д.',"
        "    guberniya: 'Костромская', uyezd: 'Макарьевский', volost: '', familio_url: ''}});"
        "  const renamed = await call('place_get', {name: 'Заборье (Нежитино)'});"
        "  const persons = await call('suggest_person', {prefix: 'Евлампия', limit: 6, gender: 'Ж',"
        "    preferInfant: false, keepWifeRank: false});"
        "  const asIs = await call('suggest_person', {prefix: 'Евлампия', limit: 6, gender: 'Ж',"
        "    preferInfant: false, keepWifeRank: true});"
        "  const clergy = await call('list_clergy', {limit: 100});"
        "  return {off, on, got, twin, renamed, persons, asIs, clergy: clergy.length};"
        "})().then(done, (e) => done({error: String(e)}));")
    check("новые команды и параметры отвечают без ошибки", "error" not in called, str(called)[:400])
    if "error" not in called:
        check("флажок «звание само» пишется и читается в настройках прихода",
              called["off"] == "0" and called["on"] == "1", f"{called['off']} / {called['on']}")
        got = called["got"] or {}
        check("пункт с комментарием: название с комментарием, чистое — отдельно",
              got.get("name") == "Заборье (Столпино)" and got.get("clean") == "Заборье" and got.get("comment") == "Столпино",
              str(got)[:200])
        similar = [x.get("value") for x in (called["twin"] or {}).get("similar", [])]
        check("набрали без комментария — тёзка первой среди похожих",
              called["twin"].get("known") is False and similar[:1] == ["Заборье (Столпино)"], str(similar)[:200])
        renamed = called["renamed"] or {}
        check("смена комментария переименовывает пункт", renamed.get("comment") == "Нежитино"
              and renamed.get("clean") == "Заборье", str(renamed)[:200])
        ranks = [p.get("rank") for p in called["persons"]]
        as_is = [p.get("rank") for p in called["asIs"]]
        # В архиве сквозной проверки Евлампия Васильева — жена крестьянина.
        check("«законная жена его» в подсказке — «крестьянская жена», в поле матери — как есть",
              ranks == ["крестьянская жена"] and as_is == ["законная жена его"], f"{ranks} / {as_is}")

    print("\n5г. Сборка 09.10: жена по НП мужа, замки и свёрнутые блоки — настоящий Rust и WebView2")
    # Жена ищется по ИОФ мужа и его НП (Роман 09.10.2026: «жена его полного
    # тёзки из другой деревни»): новый параметр команды и новый запрос.
    # В архиве сквозной проверки у Никиты Алексеева жена — Евлампия Васильева.
    called = driver.execute_async_script(
        "const done = arguments[arguments.length - 1];"
        "const call = window.__TAURI_INTERNALS__.invoke;"
        "(async () => {"
        "  const any = await call('suggest_spouse', {husband: 'Никита Алексеев', place: null});"
        "  const same = any && any.place ? await call('suggest_spouse', {husband: 'Никита Алексеев', place: any.place}) : null;"
        "  const other = await call('suggest_spouse', {husband: 'Никита Алексеев', place: 'Заборье (Нежитино)'});"
        "  const known = await call('place_check', {name: 'заборье (нежитино)'});"
        "  const relative = await call('rank_default', {role: 'relative', gender: 'М'});"
        "  return {any, same, other, known, relative};"
        "})().then(done, (e) => done({error: String(e)}));")
    check("поиск жены с НП мужа и проверка пункта отвечают без ошибки", "error" not in called, str(called)[:400])
    if "error" not in called:
        wife = (called["any"] or {}).get("iof")
        check("жена находится по мужу", wife == "Евлампия Васильева", str(called["any"])[:200])
        check("с НП мужа — та же жена; с чужим НП — никого",
              (called["same"] or {}).get("iof") == wife and called["other"] is None,
              f"{called['same']} / {called['other']}")
        check("звание по умолчанию для родственника в браке — строка или пусто",
              called["relative"] is None or isinstance(called["relative"], str), str(called["relative"]))
        check("проверка пункта отдаёт написание справочника",
              called["known"].get("known") is True and called["known"].get("canonical") == "Заборье (Нежитино)",
              str(called["known"])[:200])

    # Замки и сворачивание — щелчками в окне, настройка читается из прихода.
    def setting(key):
        return driver.execute_async_script(
            "const done = arguments[arguments.length - 1];"
            "window.__TAURI_INTERNALS__.invoke('get_setting', {key: arguments[0]}).then(done, (e) => done('ошибка: ' + e));", key)

    def toggled(js_click, js_state, key, want_state, want_value):
        """Щёлкнуть и дождаться и перерисовки, и записи настройки — до 8 с, без
        пауз на глаз (урок 08.10.2026: чтение сразу после действия — гонка).
        Возвращает (состояние на экране, настройка) — какими они стали."""
        driver.execute_script(js_click)
        state = value = None
        for _ in range(40):
            state = driver.execute_script(js_state)
            value = setting(key)
            if state == want_state and value == want_value:
                break
            time.sleep(0.2)
        return state, value

    lock_click = "document.querySelector('.birth [data-clergy-lock]').click();"
    lock_state = "return document.querySelectorAll('.birth select.clergyselect:disabled').length;"
    third_js = "const s = document.querySelector(\".birth select[data-clergy='2']\"); return s ? [s.value, s.options.length] : null;"
    if driver.execute_script("return !!document.querySelector('.birth [data-clergy-lock]');"):
        # Список причта с клавиатуры в WebView2: стрелка меняет человека, не
        # раскрывая список (до 09.10.2026 это проверял только стенд в Chromium).
        # Выбирать есть из кого, только если в памяти причта есть не священник
        # (шаг 5 сохранил причт без звания) — иначе проверка стрелок пропускается
        # с пометкой, а не роняет сборку по посторонней причине.
        was, options = driver.execute_script(third_js)
        if options >= 2 and was == "":
            def third_value(want_empty):
                for _ in range(25):
                    got = driver.execute_script(third_js)[0]
                    if (got == "") == want_empty:
                        return got
                    time.sleep(0.2)
                return driver.execute_script(third_js)[0]
            driver.execute_script(
                "const s = document.querySelector(\".birth select[data-clergy='2']\");"
                "s.scrollIntoView({block: 'center'}); s.focus();")
            driver.switch_to.active_element.send_keys(Keys.ARROW_DOWN)
            picked = third_value(False)
            driver.switch_to.active_element.send_keys(Keys.ARROW_UP)
            back = third_value(True)
            check("стрелки в списке причта меняют церковнослужителя и возвращают «никого»",
                  picked != "" and back == "", f"«{was}» → «{picked}» → «{back}»")
            if back != "":
                # Не оставлять следующим шагам лишнего третьего причта.
                driver.execute_script(
                    "const s = document.querySelector(\".birth select[data-clergy='2']\");"
                    "const set = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set;"
                    "set.call(s, ''); s.dispatchEvent(new Event('change', {bubbles: true}));")
                third_value(True)
            driver.switch_to.active_element.send_keys(Keys.ENTER)
            on_save = False
            for _ in range(15):
                on_save = driver.execute_script("return !!(document.activeElement && document.activeElement.closest('.savebar'));")
                if on_save:
                    break
                time.sleep(0.2)
            check("Enter с последнего списка ведёт на «Сохранить»", bool(on_save))
        else:
            print(f"  [инфо]   стрелки в списке причта не проверены: в третьем списке строк {options}, выбрано «{was}»")
        state, value = toggled(lock_click, lock_state, "clergy_lock", 3, "1")
        check("причт закреплён: три списка недоступны, настройка в приходе", state == 3 and value == "1", f"{state} / {value}")
        state, value = toggled(lock_click, lock_state, "clergy_lock", 0, "0")
        check("замок снят: списки доступны", state == 0 and value == "0", f"{state} / {value}")
    else:
        check("в свёрнутом причте есть флажок «закрепить»", False, clergy_shown(driver, ".birth"))

    conf_click = ("document.querySelector('.birth button.lock')"
                  ".dispatchEvent(new MouseEvent('mousedown', {bubbles: true, cancelable: true}));")
    conf_state = "return [...document.querySelectorAll('.birth input.locked')].filter((i) => i.readOnly).length;"
    state, value = toggled(conf_click, conf_state, "confession_lock", 2, "1")
    check("замок вероисповедания: закрыто у отца и у матери, настройка в приходе", state == 2 and value == "1", f"{state} / {value}")
    state, value = toggled(conf_click, conf_state, "confession_lock", 0, "0")
    check("замок вероисповедания открыт", state == 0 and value == "0", f"{state} / {value}")

    fold_state = "return document.querySelectorAll('.birth [data-fold=\"birth_godparents\"]').length;"
    state, value = toggled("document.querySelector('.birth [data-fold-close=\"birth_godparents\"]').click();",
                           fold_state, "fold_birth_godparents", 1, "1")
    check("восприемники свёрнуты в строку, настройка в приходе", state == 1 and value == "1", f"{state} / {value}")
    state, value = toggled("document.querySelector('.birth [data-fold-open=\"birth_godparents\"]').click();",
                           fold_state, "fold_birth_godparents", 0, "0")
    check("восприемники развёрнуты", state == 0 and value == "0", f"{state} / {value}")

    print("\n5д. Поиск персоны и досье — настоящий Rust и клавиши в WebView2")
    # Запись шага 5: ребёнок Мария, четвёртый восприемник «Пётр Сидоров», причт
    # «Александр Рождественский». Команды зовём как окно: новый запрос
    # search_mentions и отбор в крейте через настоящий rusqlite.
    called = driver.execute_async_script(
        "const done = arguments[arguments.length - 1];"
        "const call = window.__TAURI_INTERNALS__.invoke;"
        "const f = (query, extra) => Object.assign({query, place: null, own: true, part: true, clergy: false,"
        "                                            year_from: null, year_to: null}, extra || {});"
        "(async () => {"
        "  const found = await call('search_persons', {filter: f('сид пет')});"
        "  const none = await call('search_persons', {filter: f('рождеств')});"
        "  const clergy = await call('search_persons', {filter: f('рождеств', {clergy: true})});"
        "  const years = await call('search_persons', {filter: f('сид пет', {year_from: 1700, year_to: 1701})});"
        "  const hit = found.persons[0];"
        "  const dossier = hit ? await call('person_dossier', {key: hit.key, place: hit.place, filter: f('')}) : null;"
        "  return {found, none: none.total, clergy: clergy.persons.map((p) => p.iof), years: years.total, dossier};"
        "})().then(done, (e) => done({error: String(e)}));")
    check("поиск и досье отвечают без ошибки", "error" not in called, str(called)[:400])
    if "error" not in called:
        persons = [(p.get("iof"), p.get("mentions")) for p in called["found"]["persons"]]
        # В списке он стоит в современном написании, а его даёт словарь имён:
        # «Сидоров» там — «Исидорович». Сверяем не написание, а то, что нашёлся
        # один человек с одним упоминанием и что в записи он «Пётр Сидоров»
        # (сборка 09.10.2026 упала на ожидании «Петр Сидор…» в списке).
        in_entry = (((called["dossier"] or {}).get("events") or [{}])[0].get("me") or {}).get("iof")
        check("«сид пет» находит восприемника «Пётр Сидоров» — слова в любом порядке и не целиком",
              len(persons) == 1 and persons[0][1] == 1 and in_entry == "Пётр Сидоров", f"{persons} / в записи: {in_entry}")
        check("причт без переключателя не ищется, с переключателем — находится; отбор по годам действует",
              called["none"] == 0 and len(called["clergy"]) == 1 and called["years"] == 0,
              f"{called['none']} / {called['clergy']} / {called['years']}")
        d = called["dossier"] or {}
        ev = (d.get("events") or [{}])[0]
        near = [(m.get("role_code"), m.get("iof")) for m in ev.get("others", [])]
        check("досье: одно событие — восприемник у Марии, год 1897, страница разворотом",
              d.get("mentions") == 1 and ev.get("me", {}).get("role_code") == "godparent4"
              and ("child", "Мария") in near and ev.get("year") == 1897 and ev.get("page") == "939об-940",
              f"{str(d)[:300]}")
        check("причта среди «рядом в записи» нет", not any(r.startswith("clergy") for r, _ in near), str(near))

    # Ctrl+F настоящими клавишами: свой экран вместо поиска по странице,
    # который есть у окна WebView2. Курсор — в поле формы.
    def search_shown():
        return bool(driver.execute_script(
            "const s = document.querySelector('[data-search]'); return !!s && s.offsetParent !== null;"))

    def wait_for(cond, seconds=8):
        for _ in range(int(seconds / 0.2)):
            if cond():
                return True
            time.sleep(0.2)
        return cond()

    start = field(driver, "Ребёнок")
    driver.execute_script("arguments[0].scrollIntoView({block: 'center'}); arguments[0].focus();", start)
    # Что именно дошло до страницы — в журнал: без этого по «экран не открылся»
    # не понять, клавиша не дошла или программа её не узнала.
    driver.execute_script(
        "window.__lastKey = null;"
        "document.addEventListener('keydown', (e) => { window.__lastKey ="
        " {key: e.key, code: e.code, keyCode: e.keyCode, ctrl: e.ctrlKey}; }, true);")
    # Через действия драйвера, а не send_keys: только так у события есть код
    # клавиши (code), а буква зависит от раскладки (ревьюер 09.10.2026).
    # Событие идёт сразу в страницу — перехватит ли окно WebView2 настоящий
    # Ctrl+F своим поиском по странице, этот шаг не покажет: это вопрос Роману.
    ActionChains(driver).key_down(Keys.CONTROL).send_keys("f").key_up(Keys.CONTROL).perform()
    opened = wait_for(search_shown)
    if not opened:
        # Запасной путь: то же сочетание прежним способом (событие без code).
        print(f"  [инфо]   после действий драйвера экран не открылся, клавиша: {driver.execute_script('return window.__lastKey')}")
        start.send_keys(Keys.CONTROL, "f")
        opened = wait_for(search_shown, 4)
    check("Ctrl+F открывает экран «Поиск»", opened,
          f"последняя клавиша: {driver.execute_script('return window.__lastKey')} | {error_details(driver)}")
    if search_shown():
        box = driver.find_element(By.CSS_SELECTOR, "[data-search-query]")
        box.send_keys("петр сидоров")
        got = wait_for(lambda: bool(driver.find_elements(By.CSS_SELECTOR, "[data-dossier] table.events tr")))
        text = driver.execute_script(
            "const d = document.querySelector('[data-dossier]'); return d ? d.innerText.replace(/\\s+/g, ' ') : '';") or ""
        check("набрали «петр сидоров» — досье с событием «восприемник»",
              got and "восприемник" in text and "Мария" in text, text[:300] or error_details(driver))
        driver.find_element(By.CSS_SELECTOR, "[data-search-query]").send_keys(Keys.ESCAPE)
        check("Esc возвращает в форму, в то же поле",
              wait_for(lambda: not search_shown()) and driver.execute_script(
                  "const a = document.activeElement; const f = a && a.closest('.field');"
                  "return !!f && f.querySelector('label').textContent.trim() === 'Ребёнок';"),
              error_details(driver))
    if search_shown():
        # Не оставлять следующим шагам чужой экран.
        driver.find_element(By.XPATH, "//nav//button[normalize-space()='Рождения']").click()
        time.sleep(0.5)

    # Кнопка на «Деле» — главный вход (ответ заказчика 4Б): открывает экран,
    # «Закрыть» возвращает на «Дело».
    driver.find_element(By.XPATH, "//nav//button[normalize-space()='Дело']").click()
    wait_for(lambda: bool(driver.execute_script(
        "const b = document.querySelector('[data-find-person]'); return !!b && b.offsetParent !== null;")))
    driver.execute_script("const b = document.querySelector('[data-find-person]'); b.scrollIntoView({block: 'center'}); b.click();")
    check("кнопка «Найти персону» на «Деле» открывает экран «Поиск»", wait_for(search_shown), error_details(driver))
    if search_shown():
        driver.execute_script("document.querySelector('[data-search-close]').click();")
        check("«Закрыть» возвращает на «Дело»", wait_for(lambda: not search_shown()), error_details(driver))
    driver.find_element(By.XPATH, "//nav//button[normalize-space()='Рождения']").click()
    time.sleep(0.5)

    print("\n5а. «Всегда в столбик» — на «Деле», изначально включено; общая настройка")
    # С 08.10.2026 переключатель стоит на экране «Дело» и включён, пока его
    # явно не выключили (Роман 07.10.2026).
    driver.find_element(By.XPATH, "//nav//button[normalize-space()='Дело']").click()
    time.sleep(0.5)
    on = driver.execute_script("return document.documentElement.classList.contains('onecol')")
    pressed = shown(driver, "//button[@data-onecol]").get_attribute("aria-pressed")
    check("изначально включено: форма в один столбец", bool(on) and pressed == "true",
          f"класс {on}, кнопка «{pressed}» | {error_details(driver)}")
    click(driver, "//button[@data-onecol]")
    time.sleep(0.8)
    on = driver.execute_script("return document.documentElement.classList.contains('onecol')")
    pressed = shown(driver, "//button[@data-onecol]").get_attribute("aria-pressed")
    check("выключили: плотная раскладка", not on and pressed == "false",
          f"класс {on}, кнопка «{pressed}» | {error_details(driver)}")


def print_app_log():
    """Журнал ошибок приложения — там, куда его пишет main.rs."""
    appdata = os.environ.get("APPDATA", "")
    log = Path(appdata) / "org.genmetric.app" / "genmetric-журнал.txt"
    if log.exists():
        print(f"--- {log} ---")
        print(log.read_text(encoding="utf-8", errors="replace")[-4000:])
    else:
        print(f"журнала приложения нет: {log}")
def launch(exe, msedgedriver, port=9222):
    """Запускает приложение с портом отладки и подключает к нему msedgedriver.
    Возвращает (процесс, driver, wait) или (процесс, None, None) при отказе."""
    # Порт отладки WebView2 приложение открывает само, увидев
    # GENMETRIC_E2E_DEBUG_PORT (main.rs). Переменная
    # WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS, которую обещает документация
    # Microsoft, на раннере 14.09.2026 до WebView2 не дошла — порт не открылся.
    env = dict(os.environ)
    env["GENMETRIC_E2E_DEBUG_PORT"] = str(port)
    env["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"] = f"--remote-debugging-port={port}"
    # «Найти на Familio» на раннере браузер не открывает (export.rs): окно Edge
    # поверх программы забрало бы фокус у остальных шагов.
    env["GENMETRIC_NO_BROWSER"] = "1"
    app = subprocess.Popen([str(exe)], env=env, cwd=str(exe.parent))
    print(f"приложение запущено, pid {app.pid}")
    time.sleep(5)
    if app.poll() is not None:
        print(f"приложение завершилось само с кодом {app.returncode} — WebView2 не создан?")
        print_app_log()
        return app, None, None
    print("приложение живо через 5 с")
    # Диагностика до драйвера: слушает ли кто-то порт, и есть ли процесс WebView2.
    import socket
    with socket.socket() as sock:
        sock.settimeout(2)
        listening = sock.connect_ex(("127.0.0.1", port)) == 0
    print(f"порт {port} {'открыт' if listening else 'ЗАКРЫТ'}")
    tasks = subprocess.run(["tasklist", "/FI", "IMAGENAME eq msedgewebview2.exe"],
                           capture_output=True, text=True, errors="replace").stdout
    print("процессов msedgewebview2.exe:", tasks.count("msedgewebview2.exe"))

    opts = EdgeOptions()
    opts.use_webview = True  # browserName: webview2
    opts.debugger_address = f"127.0.0.1:{port}"
    service = EdgeService(executable_path=str(msedgedriver), log_output="e2e-msedgedriver.log")
    try:
        driver = webdriver.Edge(service=service, options=opts)
    except Exception as e:  # noqa: BLE001 — сессия не создалась: показать всё, что есть
        print(f"сессия WebDriver не создалась: {type(e).__name__}: {e}")
        print_app_log()
        app.kill()
        return app, None, None
    return app, driver, WebDriverWait(driver, 30)


def stop(app, driver):
    """Закрыть сессию и приложение, дождаться выхода — иначе база занята."""
    if driver:
        try:
            driver.quit()
        except Exception:  # noqa: BLE001 — на пути выхода всё равно убиваем
            pass
    app.kill()
    try:
        app.wait(timeout=10)
    except Exception:  # noqa: BLE001
        pass


def resumed(driver, wait):
    """После перезапуска форма продолжает с места остановки (заказчик 15.09.2026)."""
    print("\n6. Перезапуск: форма помнит, где остановились")
    wait.until(EC.presence_of_element_located((By.CSS_SELECTOR, ".app")))
    # «Всегда в столбик», выключенное в конце первого запуска (шаг 5а), так и
    # осталось выключенным: настройка лежит в общем файле приходов и читается
    # через Rust. Без неё форма встала бы в столбик — он включён изначально.
    time.sleep(1)
    check("выключенное «всегда в столбик» пережило перезапуск",
          not driver.execute_script("return document.documentElement.classList.contains('onecol')"),
          error_details(driver))
    driver.find_element(By.XPATH, "//nav//button[normalize-space()='Рождения']").click()
    wait.until(EC.presence_of_element_located((By.XPATH, "//section[.//h2[normalize-space()='Отец']]")))
    time.sleep(1)
    # Счёт ставится сам, по полу (Роман 08.10.2026): последняя запись —
    # девочка № 7, пол следующего ребёнка ещё не известен — в поле 8.
    count = field(driver, "Счёт").get_attribute("value")
    check("счёт продолжен: после девочки № 7 — 8", count == "8", f"«{count}»")
    check("поле «Счёт» вне обхода клавишами", field(driver, "Счёт").get_attribute("tabindex") == "-1")
    page = field(driver, "Стр.").get_attribute("value")
    check("страница восстановлена: 939об-940", page == "939об-940", f"«{page}»")
    year = field(driver, "Год").get_attribute("value")
    check("год восстановлен: 1897", year == "1897", f"«{year}»")
    body = driver.find_element(By.TAG_NAME, "body").text
    check("список набранного на месте", "Набрано: 1" in body)
    check("причт восстановлен (21.09.2026)", "Александр Рождественский" in clergy_shown(driver, ".birth"),
          clergy_shown(driver, ".birth"))
    check("свёрнутый причт — три выпадающих списка",
          len(driver.find_elements(By.CSS_SELECTOR, ".birth select[data-clergy]")) == 3)
    # Записать ещё одну — с восстановленным причтом (ревьюер 21.09.2026: причт
    # приходит без разбора, и сохранение должно пройти без ошибок).
    # Счёт руками не набираем: мальчиков в этом году ещё не было — программа
    # сама поставит 1, как только поймёт пол по имени.
    fill(driver, "Ребёнок", "Иван")
    child = "//div[contains(@class,'field')][./label[normalize-space()='Ребёнок']]"
    wait.until(EC.text_to_be_present_in_element((By.XPATH, child + "//*[contains(@class,'parsedline')]"), "М"))
    time.sleep(0.5)
    count = field(driver, "Счёт").get_attribute("value")
    check("набрали мальчика — счёт сам стал 1", count == "1", f"«{count}»")
    click(driver, "//button[starts-with(normalize-space(),'Сохранить и следующая')]")
    try:
        wait.until(EC.text_to_be_present_in_element((By.TAG_NAME, "body"), "Набрано: 2"))
    except TimeoutException:
        pass
    body = driver.find_element(By.TAG_NAME, "body").text
    errorbar = [e.text for e in driver.find_elements(By.CSS_SELECTOR, ".errorbar")]
    check("вторая запись после перезапуска сохранена", "Набрано: 2" in body, " | ".join(errorbar))
    check("у мальчика «№ м. 1»", "№ м. 1" in body)
    time.sleep(0.5)  # счёт пересчитывается вслед за списком «Набрано»
    count = field(driver, "Счёт").get_attribute("value")
    check("после сохранения в поле следующий номер мальчиков — 2", count == "2", f"«{count}»")

    print("\n7. Правка сохранённой записи (заказчик 22.09.2026)")
    # Открываем последнюю (первую в списке — мальчик, счёт 1), меняем счёт на 9.
    click(driver, "(//table[contains(@class,'saved')]//button[starts-with(normalize-space(),'Открыть')])[1]")
    wait.until(EC.presence_of_element_located((By.CSS_SELECTOR, ".editbar")))
    lifted = field(driver, "Ребёнок").get_attribute("value")
    check("запись поднялась в форму: ребёнок «Иван»", lifted == "Иван", f"«{lifted}»")
    # Запись «Иван» сохранена после перезапуска с восстановленным причтом —
    # имя причта обязано быть в базе (инцидент 22.09.2026: терялось).
    check("у записи после перезапуска причт с именем", "Александр Рождественский" in clergy_shown(driver, ".birth"),
          clergy_shown(driver, ".birth"))
    fill(driver, "Счёт", "9")
    click(driver, "//button[starts-with(normalize-space(),'Сохранить изменения')]")
    try:
        wait.until(EC.text_to_be_present_in_element((By.TAG_NAME, "body"), "№ м. 9"))
    except TimeoutException:
        pass
    body = driver.find_element(By.TAG_NAME, "body").text
    errorbar = [e.text for e in driver.find_elements(By.CSS_SELECTOR, ".errorbar")]
    check("изменения сохранены: «№ м. 9» в списке", "№ м. 9" in body, error_details(driver))
    if "№ м. 9" not in body:
        state = driver.execute_script(
            "const f=document.querySelector('.birth');"
            "return {focus: document.activeElement && (document.activeElement.outerHTML||'').slice(0,120),"
            " modal: !!document.querySelector('.modal'), busy: f && f.querySelector('.savebar button').disabled,"
            " count: f && f.querySelector('.row.tight').innerText.replace(/\\n/g,' '), list: f && (f.querySelector('.saved')||{}).innerText};")
        print(f"  [инфо]   состояние формы: {state}")
    check("записей по-прежнему две — правка не плодит", "Набрано: 2" in body)
    check("режим правки снят", not driver.find_elements(By.CSS_SELECTOR, ".editbar"))

    print("\n8. Сверка имени со справочником (заказчик 23.09.2026)")
    father = "//section[.//h2[normalize-space()='Отец']]//"
    iof = field(driver, "ИОФ", father)
    iof.send_keys("Пискарь Иванов Сидоров")
    iof.send_keys(Keys.ESCAPE)
    iof.send_keys(Keys.TAB)  # уход из поля — момент сверки
    try:
        wait.until(EC.presence_of_element_located((By.CSS_SELECTOR, ".modal[data-modal='resolve-name']")))
        time.sleep(0.5)  # окно первые 350 мс не принимает набор (Modal.GUARD_MS)
        opened = True
    except TimeoutException:
        opened = False
    check("окно сверки открылось на уходе из поля", opened,
          "" if opened else "полоса: " + " | ".join(e.text for e in driver.find_elements(By.CSS_SELECTOR, ".errorbar")))
    if opened:
        modal = driver.find_element(By.CSS_SELECTOR, ".modal[data-modal='resolve-name']").text
        check("в заголовке — «Пискарь»", "«Пискарь»" in modal, modal.split("\n")[0])
        check("среди похожих — «Кесарь»", "Кесарь" in modal, modal.replace("\n", " | ")[:200])
        active = driver.switch_to.active_element
        check("фокус в поле поиска окна", active.tag_name == "input" and active.find_elements(By.XPATH, "ancestor::div[contains(@class,'modal')]"))
        active.send_keys(Keys.ENTER)  # первое похожее = Кесарь
        wait.until(EC.invisibility_of_element_located((By.CSS_SELECTOR, ".modal")))
        value = field(driver, "ИОФ", father).get_attribute("value")
        check("в поле — «Кесарь Иванов Сидоров»", value == "Кесарь Иванов Сидоров", f"«{value}»")
        note = field(driver, "Прим.", father).get_attribute("value")
        check("в примечании — «Имя в документе: Пискарь»", "Имя в документе: Пискарь" in note, f"«{note}»")

    print("\n9. Карточка населённого пункта (заказчик 23.09.2026)")
    np = field(driver, "НП", father)
    np.send_keys("Букарина")
    np.send_keys(Keys.ESCAPE)
    np.send_keys(Keys.TAB)
    try:
        wait.until(EC.presence_of_element_located((By.CSS_SELECTOR, ".modal[data-modal='place']")))
        time.sleep(0.5)  # окно первые 350 мс не принимает набор (Modal.GUARD_MS)
        opened = True
    except TimeoutException:
        opened = False
    check("карточка открылась для «Букарина»", opened)
    if opened:
        modal = driver.find_element(By.CSS_SELECTOR, ".modal[data-modal='place']").text
        check("похожее — «Бухарино»", "Бухарино" in modal, modal.replace("\n", " | ")[:200])
        driver.switch_to.active_element.send_keys(Keys.ENTER)  # выбрать похожее
        wait.until(EC.invisibility_of_element_located((By.CSS_SELECTOR, ".modal")))
        value = field(driver, "НП", father).get_attribute("value")
        check("в поле — «Бухарино»", value == "Бухарино", f"«{value}»")
    god1 = "//section[.//h2[normalize-space()='Восприемник 1']]//"
    field(driver, "ИОФ", god1).send_keys("Анна Иванова")
    field(driver, "ИОФ", god1).send_keys(Keys.ESCAPE)
    np = field(driver, "НП", god1)
    np.send_keys("Новодеревенька")
    np.send_keys(Keys.ESCAPE)
    np.send_keys(Keys.TAB)
    try:
        wait.until(EC.presence_of_element_located((By.CSS_SELECTOR, ".modal[data-modal='place']")))
        time.sleep(0.5)  # окно первые 350 мс не принимает набор (Modal.GUARD_MS)
        opened = True
    except TimeoutException:
        opened = False
    check("карточка нового НП открылась", opened)
    if opened:
        gub = field(driver, "Губерния", "//div[contains(@class,'modal')]//").get_attribute("value")
        check("губерния подставлена из дела", gub == "Костромская", f"«{gub}»")
        # «Найти на Familio» (Роман 03.10.2026): команда идёт через настоящий
        # Rust и открывает браузер системы. Что именно открыто — в журнале.
        click(driver, "//div[contains(@class,'modal')]//button[@data-familio-find]")
        time.sleep(1.5)
        appdata = os.environ.get("APPDATA", "")
        log = Path(appdata) / "org.genmetric.app" / "genmetric-журнал.txt"
        journal = log.read_text(encoding="utf-8", errors="replace") if log.exists() else ""
        want = "https://familio.org/places?title=" + quote("Новодеревенька", safe="")
        check("«Найти на Familio»: открыт поиск по названию и губернии, ошибки нет",
              want in journal and "georequisites=" + quote("Костромская", safe="") in journal
              and "Familio" not in error_details(driver),
              (journal[-300:].replace("\n", " | ") or "журнала нет") + " | " + error_details(driver))
        focused = driver.switch_to.active_element.get_attribute("placeholder") or ""
        check("после кнопки фокус — в поле ссылки", "ссылка" in focused, f"«{focused}»")
        # Дальше — как раньше, с первого поля карточки: Enter ведёт по полям.
        field(driver, "Тип", "//div[contains(@class,'modal')]//").click()
        for _ in range(5):  # тип, губерния, уезд, волость, Familio → сохранить
            driver.switch_to.active_element.send_keys(Keys.ENTER)
            time.sleep(0.2)
        wait.until(EC.invisibility_of_element_located((By.CSS_SELECTOR, ".modal")))
        value = field(driver, "НП", god1).get_attribute("value")
        check("НП остался в поле", value == "Новодеревенька", f"«{value}»")
        # Правка названия в карточке (заказчик 25.09.2026). В собранной
        # программе переименование падало на лишнем параметре SQL — ревьюер
        # нашёл до выкладки; этот шаг ходит через настоящий Rust.
        click(driver, god1 + "div[contains(@class,'field')][./label[normalize-space()='НП']]"
                             "//button[contains(@class,'action')]")
        wait.until(EC.presence_of_element_located((By.CSS_SELECTOR, ".modal[data-modal='place-edit']")))
        time.sleep(0.5)  # окно первые 350 мс не принимает набор (Modal.GUARD_MS)
        title = field(driver, "Название", "//div[contains(@class,'modal')]//")
        title.send_keys(Keys.CONTROL, "a")
        title.send_keys("Новодеревенька Малая")
        click(driver, "//div[contains(@class,'modal')]//button[normalize-space()='Сохранить изменения']")
        try:
            wait.until(EC.invisibility_of_element_located((By.CSS_SELECTOR, ".modal")))
        except TimeoutException:
            pass
        errorbar = [e.text for e in driver.find_elements(By.CSS_SELECTOR, ".errorbar")]
        value = field(driver, "НП", god1).get_attribute("value")
        check("переименование НП прошло через Rust", value == "Новодеревенька Малая" and not errorbar,
              f"«{value}» | " + " | ".join(errorbar))
    fill(driver, "Счёт", "10")
    fill(driver, "Ребёнок", "Анна")
    child = "//div[contains(@class,'field')][./label[normalize-space()='Ребёнок']]"
    wait.until(EC.text_to_be_present_in_element((By.XPATH, child + "//*[contains(@class,'parsedline')]"), "Ж"))
    click(driver, "//button[starts-with(normalize-space(),'Сохранить и следующая')]")
    try:
        wait.until(EC.text_to_be_present_in_element((By.TAG_NAME, "body"), "Набрано: 3"))
    except TimeoutException:
        pass
    body = driver.find_element(By.TAG_NAME, "body").text
    errorbar = [e.text for e in driver.find_elements(By.CSS_SELECTOR, ".errorbar")]
    check("третья запись сохранена", "Набрано: 3" in body, " | ".join(errorbar))
    check("в списке виден отец", "отец Кесарь Иванов Сидоров" in body)

    print("\n10. Запись без отца (заказчик, приоритет № 3 от 23.09.2026)")
    mother = "//section[.//h2[normalize-space()='Мать']]//"
    rank = field(driver, "Звание", mother).get_attribute("value")
    check("у новой записи звание матери пусто", rank == "", f"«{rank}»")
    fill(driver, "Счёт", "11")
    fill(driver, "Ребёнок", "Мария")
    field(driver, "ИОФ", mother).send_keys("Анна Иванова")
    field(driver, "ИОФ", mother).send_keys(Keys.ESCAPE)
    # Ждём разбор матери (метка пола под её ИОФ), а не ребёнка — иначе
    # проверка звания пройдёт до разбора (ревьюер 24.09.2026).
    wait.until(EC.text_to_be_present_in_element(
        (By.XPATH, mother + "div[contains(@class,'field')][./label[normalize-space()='ИОФ']]"
                   "//*[contains(@class,'parsedline')]"), "Ж"))
    rank = field(driver, "Звание", mother).get_attribute("value")
    check("без отца у матери нет «законная жена его»", rank == "", f"«{rank}»")
    click(driver, "//button[starts-with(normalize-space(),'Сохранить и следующая')]")
    try:
        wait.until(EC.text_to_be_present_in_element((By.TAG_NAME, "body"), "Набрано: 4"))
    except TimeoutException:
        pass
    body = driver.find_element(By.TAG_NAME, "body").text
    errorbar = [e.text for e in driver.find_elements(By.CSS_SELECTOR, ".errorbar")]
    check("запись без отца сохранена", "Набрано: 4" in body, " | ".join(errorbar))
    father = "//section[.//h2[normalize-space()='Отец']]//"
    field(driver, "ИОФ", father).send_keys("Пётр Сидоров")
    field(driver, "ИОФ", father).send_keys(Keys.ESCAPE)
    time.sleep(0.5)
    rank = field(driver, "Звание", mother).get_attribute("value")
    check("набрали отца — у матери «законная жена его»", rank == "законная жена его", f"«{rank}»")

    print("\n11. Браки (заказчик 25.09.2026)")
    driver.find_element(By.XPATH, "//nav//button[normalize-space()='Браки']").click()
    m = "//div[contains(@class,'marriage')]//"
    wait.until(EC.visibility_of_element_located((By.XPATH, m + "section[.//h2[normalize-space()='Жених']]")))
    groom = m + "section[.//h2[normalize-space()='Жених']]//"
    bride = m + "section[.//h2[normalize-space()='Невеста']]//"
    order = field(driver, "Брак", groom).get_attribute("value")
    check("у жениха заготовка «Первым браком»", order == "Первым браком", f"«{order}»")
    fill(driver, "Год", "1886", m)
    fill(driver, "Счёт", "1", m)
    field(driver, "ИОФ", groom).send_keys("Михаил Дмитриев")
    field(driver, "ИОФ", groom).send_keys(Keys.ESCAPE)
    field(driver, "ИОФ", bride).send_keys("Евдокия Савельева")
    field(driver, "ИОФ", bride).send_keys(Keys.ESCAPE)
    time.sleep(0.8)  # разбор ИОФ — асинхронный
    click(driver, m + "button[starts-with(normalize-space(),'Сохранить и следующая')]")
    try:
        wait.until(EC.text_to_be_present_in_element((By.CSS_SELECTOR, ".marriage"), "Набрано браков: 1"))
    except TimeoutException:
        pass
    block = driver.find_element(By.CSS_SELECTOR, ".marriage").text
    errorbar = [e.text for e in driver.find_elements(By.CSS_SELECTOR, ".errorbar")]
    check("запись о браке сохранена", "Набрано браков: 1" in block, " | ".join(errorbar))
    check("в строке жених и невеста", "Михаил Дмитриев" in block and "Евдокия Савельева" in block)
    count = field(driver, "Счёт", m).get_attribute("value")
    check("счёт браков вырос до 2", count == "2", f"«{count}»")
    check("в списке номер перед датой", "№ 1 · " in block)
    # Поручители до шести (Роман 27.09.2026).
    click(driver, m + "button[normalize-space()='+ добавить поручителя']")
    w5 = shown(driver, m + "section[.//h2[starts-with(normalize-space(),'Поручитель 5')]]").text
    check("пятый поручитель добавлен, сторона «по жениху»", "по жениху" in w5, w5.replace("\n", " | ")[:80])
    # Причт общий: набран в рождениях — есть и в браках.
    check("причт из рождений виден в браках", "Александр Рождественский" in clergy_shown(driver, ".marriage"),
          clergy_shown(driver, ".marriage"))
    edit_and_save(driver, wait, m, ".marriage", 5, "№ 5 · ")

    print("\n12. Смерти (27.09.2026)")
    driver.find_element(By.XPATH, "//nav//button[normalize-space()='Смерти']").click()
    d = "//div[contains(@class,'death')]//"
    wait.until(EC.visibility_of_element_located((By.XPATH, d + "section[.//h2[normalize-space()='Умерший']]")))
    dead = d + "section[.//h2[normalize-space()='Умерший']]//"
    fill(driver, "Год", "1886", d)
    fill(driver, "Счёт", "3", d)
    field(driver, "ИОФ", dead).send_keys("Анна Иванова")
    field(driver, "ИОФ", dead).send_keys(Keys.ESCAPE)
    fill(driver, "Причина", "понос", dead)
    fill(driver, "Возраст", "1,5 мес", dead)
    time.sleep(0.8)  # разбор ИОФ — асинхронный
    click(driver, d + "button[starts-with(normalize-space(),'Сохранить и следующая')]")
    try:
        wait.until(EC.text_to_be_present_in_element((By.CSS_SELECTOR, ".death"), "Набрано смертей: 1"))
    except TimeoutException:
        pass
    block = driver.find_element(By.CSS_SELECTOR, ".death").text
    errorbar = [e.text for e in driver.find_elements(By.CSS_SELECTOR, ".errorbar")]
    check("запись о смерти сохранена", "Набрано смертей: 1" in block, " | ".join(errorbar))
    check("девочка — «№ ж. 3», умершая в строке", "№ ж. 3" in block and "Анна Иванова" in block)
    check("причт общий и здесь", "Александр Рождественский" in clergy_shown(driver, ".death"),
          clergy_shown(driver, ".death"))
    edit_and_save(driver, wait, d, ".death", 4, "№ ж. 4")

    print("\n13. Умерший без имени (Роман 30.09.2026)")
    # «Тело неизвестного человека мужеского пола» — флажок вместо ИОФ,
    # пол кнопкой, звание и причина как обычно.
    click(driver, dead + "label[contains(@class,'unknownbox')]/input")
    check("поле ИОФ умершего убрано", not driver.find_elements(
        By.XPATH, dead + "div[contains(@class,'field')][./label[normalize-space()='ИОФ']]"))
    fill(driver, "Счёт", "5", d)
    fill(driver, "Звание", "тело неизвестного человека мужеского пола", dead)
    fill(driver, "Причина", "утонул в Волге", dead)
    click(driver, dead + "button[normalize-space()='мужской']")
    click(driver, d + "button[starts-with(normalize-space(),'Сохранить и следующая')]")
    try:
        wait.until(EC.text_to_be_present_in_element((By.CSS_SELECTOR, ".death"), "Набрано смертей: 2"))
    except TimeoutException:
        pass
    block = driver.find_element(By.CSS_SELECTOR, ".death").text
    errorbar = [e.text for e in driver.find_elements(By.CSS_SELECTOR, ".errorbar")]
    check("запись без имени сохранена", "Набрано смертей: 2" in block, " | ".join(errorbar))
    check("в списке «№ м. 5 … личность не установлена»",
          "№ м. 5" in block and "личность не установлена" in block)
    check("после сохранения флажок снят, ИОФ снова на месте", bool(driver.find_elements(
        By.XPATH, dead + "div[contains(@class,'field')][./label[normalize-space()='ИОФ']]")))

    print("\n14. Выгрузка в Familio и в Excel (Роман 30.09.2026)")
    driver.find_element(By.XPATH, "//nav//button[normalize-space()='Дело']").click()
    click(driver, "//button[normalize-space()='Выгрузить в Familio…']")
    modal = "//div[@data-modal='familio']"
    wait.until(EC.visibility_of_element_located((By.XPATH, modal)))
    years = driver.find_element(By.XPATH, modal).text
    check("в окне годы 1886 и 1897", "1886" in years and "1897" in years, years.replace("\n", " | ")[:200])
    click(driver, modal + "//button[normalize-space()='Выгрузить']")
    path = exported_path(driver, wait, "Familio")
    if path:
        birth = xlsx_rows(path, "РОЖДЕНИЕ")
        data = {r: v for r, v in birth.items() if r >= 4}
        check("РОЖДЕНИЕ: шапка образца на месте", "№ п/п" in birth.get(2, {}).values())
        check("РОЖДЕНИЕ: девочка Мария", any("Мария" in v.values() for v in data.values()),
              str(list(data.values()))[:300])
        marriage = [v for r, v in xlsx_rows(path, "БРАК").items() if r >= 4]
        check("БРАК: жених Михаил", any("Михаил" in v.values() for v in marriage), str(marriage)[:300])
        death = [v for r, v in xlsx_rows(path, "СМЕРТЬ").items() if r >= 4]
        check("СМЕРТЬ: две записи, одна без имени",
              len(death) == 2 and any("утонул в Волге" in v.values() for v in death), str(death)[:300])
        about = xlsx_rows(path, "about")
        title = about.get(4, {}).get("B", "")
        check("about: название справочника по селу", "Борисоглебское" in title, f"«{title}»")
        validate_xlsx(path, "Familio", REPO / "db" / "export" / "familio_template.xlsx")
    click(driver, "//button[normalize-space()='Выгрузить в Excel']")
    # «Показать в папке»: ветка для Windows (explorer /select) на Mac не
    # компилируется — проверяем, что команда проходит без полосы ошибок
    # (ревьюер #39). С 03.10.2026 кнопка — в окне итога выгрузки.
    path = exported_path(driver, wait, "Excel", reveal=True)
    if path:
        for sheet, what in (("Рождения", "Мария"), ("Браки", "Михаил Дмитриев"), ("Смерти", "утонул в Волге")):
            rows = xlsx_rows(path, sheet)
            check(f"Excel, лист «{sheet}»: есть «{what}»",
                  any(what in " ".join(v.values()) for v in rows.values()), str(list(rows.values()))[-300:])
        mk = xlsx_rows(path, "МК")
        check("Excel, лист «МК»: строки персон есть", len(mk) > 4, f"строк {len(mk)}")
        validate_xlsx(path, "Excel")

    parishes(driver, wait)


def fresh_window(driver, wait):
    """Перечитать окно: формы пусты, несохранённого нет.

    Смена прихода с набранной, но не сохранённой записью запрещена (и это
    правильно), а сценарий к этому месту оставил в формах набранное (отец из
    шага 10). Поле через clear() React не чистит — надёжнее перечитать окно,
    как это делает сама программа при смене прихода.
    """
    driver.refresh()
    wait.until(EC.presence_of_element_located((By.CSS_SELECTOR, ".parishrow")))
    time.sleep(1.5)


def feed_file(driver, path):
    """Файл — в спрятанное поле окна «Приходы» (WebDriver кормит только видимое)."""
    file_input = driver.find_element(By.CSS_SELECTOR, "input[data-import-file]")
    driver.execute_script("arguments[0].hidden = false;", file_input)
    file_input.send_keys(str(path))


def open_parishes(driver, wait):
    """Окно «Приходы» с экрана «Дело»."""
    driver.find_element(By.XPATH, "//nav//button[normalize-space()='Дело']").click()
    # Строка прихода — с классом main: с 08.10.2026 на «Деле» есть и другая
    # строка того же вида, «Всегда в столбик».
    click(driver, "//div[contains(@class,'parishrow') and contains(@class,'main')]//button")
    wait.until(EC.visibility_of_element_located((By.XPATH, PARISH)))
    # Перечень приходит отдельным запросом чуть позже окна: сборка 05.10
    # упала на том, что список прочитали раньше, чем он появился.
    try:
        wait.until(EC.presence_of_element_located((By.XPATH, PARISH + "//table[contains(@class,'parishes')]//tr")))
    except TimeoutException:
        pass


def reloaded(driver, wait, name):
    """После смены прихода окно перечитывается целиком: ждём новое название."""
    # Текст читается одним вызовом в окне, а не по элементам: окно в этот
    # момент перечитывается, и элемент, найденный до перезагрузки, к чтению
    # текста уже не существует (StaleElementReference, сборка 08.10.2026 —
    # со второй строкой «Всегда в столбик» гонка стала попадать).
    def parish_row(d):
        return d.execute_script(
            "return [...document.querySelectorAll('.parishrow.main')].map((e) => e.innerText).join(' ');") or ""
    try:
        WebDriverWait(driver, 90, ignored_exceptions=(StaleElementReferenceException, WebDriverException)) \
            .until(lambda d: name in parish_row(d))
    except TimeoutException:
        pass
    row = parish_row(driver)
    check(f"открыт приход «{name}»", name in row, f"«{row}» | {error_details(driver)}")
    time.sleep(1.0)  # формы дочитывают списки и место работы


PARISH = "//div[@data-modal='parish']"


def parishes(driver, wait):
    """Приходы и импорт из Excel (спека 2026-10-02): каждый приход — свой файл."""
    print("\n15. Приходы: новый приход, записи не смешиваются")
    # С набранным в форме приход сменить нельзя — проверяем отказ, затем чистим.
    driver.find_element(By.XPATH, "//nav//button[normalize-space()='Рождения']").click()
    fill(driver, "Ребёнок", "Мария", "//div[contains(@class,'birth')]//")
    open_parishes(driver, wait)
    click(driver, PARISH + "//button[normalize-space()='Новый приход…']")
    time.sleep(0.6)
    shown(driver, PARISH + "//input[@data-field]").send_keys("Отказ")
    click(driver, PARISH + "//button[normalize-space()='Создать и открыть']")
    time.sleep(1.0)
    refused = " ".join(e.text for e in driver.find_elements(By.CSS_SELECTOR, ".refused"))
    check("с несохранённой записью приход не меняется — программа говорит почему",
          "Сначала сохраните или очистите набранное" in refused, refused or "отказа нет")
    fresh_window(driver, wait)
    open_parishes(driver, wait)
    listing = driver.find_element(By.XPATH, PARISH).text
    check("в перечне один приход — по селу дела, он открыт",
          "Борисоглебское" in listing and "открыт" in listing, listing.replace("\n", " | ")[:200])
    click(driver, PARISH + "//button[normalize-space()='Новый приход…']")
    time.sleep(0.6)  # окно первые мгновения набор не принимает
    name = shown(driver, PARISH + "//input[@data-field]")
    name.send_keys("Николо-Макарово")
    click(driver, PARISH + "//button[normalize-space()='Создать и открыть']")
    reloaded(driver, wait, "Николо-Макарово")
    body = driver.find_element(By.TAG_NAME, "body").text
    check("новый приход пуст: дело не заполнено", "Дело за" not in body, body[:200].replace("\n", " | "))
    for label, value in [("Архив", "ГА Костромской области"), ("Церковь", "Никольская"),
                         ("Село", "Николо-Макарово"), ("Уезд", "Макарьевский"), ("Губерния", "Костромская")]:
        fill(driver, label, value)
    driver.find_element(By.XPATH, "//button[normalize-space()='Сохранить дело']").click()
    wait.until(EC.text_to_be_present_in_element((By.TAG_NAME, "body"), "Сохранено"))
    village_card(driver, wait, "Николо-Макарово")
    driver.find_element(By.XPATH, "//nav//button[normalize-space()='Рождения']").click()
    b = "//div[contains(@class,'birth')]//"
    year = field(driver, "Год", b).get_attribute("value")
    check("в новом приходе форма пустая — год не подставлен", year == "", f"«{year}»")
    fill(driver, "Год", "1890", b)
    fill(driver, "Счёт", "1", b)
    fill(driver, "Ребёнок", "Ксения", b)
    father = b + "section[.//h2[normalize-space()='Отец']]//"
    field(driver, "ИОФ", father).send_keys("Никита Алексеев")
    field(driver, "ИОФ", father).send_keys(Keys.ESCAPE)
    time.sleep(1.0)  # разбор ИОФ — асинхронный
    hints = driver.find_element(By.CSS_SELECTOR, ".birth").text
    click(driver, b + "button[starts-with(normalize-space(),'Сохранить и следующая')]")
    try:
        wait.until(EC.text_to_be_present_in_element((By.CSS_SELECTOR, ".birth"), "Набрано: 1"))
    except TimeoutException:
        pass
    block = driver.find_element(By.CSS_SELECTOR, ".birth").text
    check("запись в новом приходе сохранена, список — за её год",
          "Набрано: 1 — за 1890 год" in block, error_details(driver) + " | " + hints[:120].replace("\n", " | "))
    # Свободный порядок на экране «Дело» (Роман 03.10.2026): поправил дело,
    # поставил новый год, сохранил — и набирает; прошлый год не тронут.
    driver.find_element(By.XPATH, "//nav//button[normalize-space()='Дело']").click()
    fill(driver, "Дело", "77")
    fill(driver, "Год книги", "1891")
    click(driver, "//button[normalize-space()='Сохранить дело']")
    try:
        wait.until(EC.text_to_be_present_in_element((By.TAG_NAME, "body"), "заведено своё дело"))
    except TimeoutException:
        pass
    try:  # список годов приходит отдельным запросом, чуть позже текста
        wait.until(EC.text_to_be_present_in_element((By.CSS_SELECTOR, "h2.caseyear"), "1890"))
    except TimeoutException:
        pass
    head = driver.find_element(By.CSS_SELECTOR, "h2.caseyear").text
    check("новому году заведено своё дело: «Дело за 1891 год», в списке и 1890",
          "Дело за 1891 год" in head and "1890" in head, head.replace("\n", " | ") + " | " + error_details(driver))
    driver.find_element(By.XPATH, "//nav//button[normalize-space()='Рождения']").click()
    year = field(driver, "Год", b).get_attribute("value")
    check("формы встали на новый год", year == "1891", f"«{year}»")
    driver.find_element(By.XPATH, "//nav//button[normalize-space()='Дело']").click()
    # Год без правки реквизитов и уход из поля — открывает дело этого года.
    fill(driver, "Год книги", "1890")
    field(driver, "Фонд").click()
    time.sleep(0.8)
    head = driver.find_element(By.CSS_SELECTOR, "h2.caseyear").text
    delo = field(driver, "Дело").get_attribute("value")
    check("у 1890 года реквизиты прежние — дело не «77»", "Дело за 1890 год" in head and delo != "77",
          f"{head.splitlines()[0]} | дело «{delo}»")
    # Общие справочники: пункт, заведённый в первом приходе (шаг 9), здесь известен.
    open_parishes(driver, wait)
    listing = driver.find_element(By.XPATH, PARISH).text
    check("в перечне два прихода", "Борисоглебское" in listing and "Николо-Макарово" in listing,
          listing.replace("\n", " | ")[:300])
    click(driver, PARISH + "//tr[.//b[normalize-space()='Борисоглебское']]//button[normalize-space()='Открыть']")
    reloaded(driver, wait, "Борисоглебское")
    driver.find_element(By.XPATH, "//nav//button[normalize-space()='Рождения']").click()
    try:
        wait.until(EC.text_to_be_present_in_element((By.CSS_SELECTOR, ".birth"), "Набрано: 4"))
    except TimeoutException:
        pass
    block = driver.find_element(By.CSS_SELECTOR, ".birth").text
    check("в первом приходе — свои четыре рождения 1897 года, «Ксении» из второго нет",
          "Набрано: 4 — за 1897 год" in block and "Ксения" not in block.split("Набрано")[-1],
          block[-300:].replace("\n", " | "))

    print("\n16. Импорт из Excel-индексатора — настоящим IPC, файл частями")
    fixture = REPO / "db" / "fixtures" / "indexer.xlsx"
    open_parishes(driver, wait)
    # Файл фикстуры — 6 КБ, а часть в программе — 512 КБ: одной частью склейку
    # частей в Rust проверка не видела (техдолг с 02.10.2026). Здесь часть —
    # 1 КБ: файл идёт шестью, и разбор ниже читает уже склеенное.
    driver.execute_script("window.__genmetricChunk = 1024;")
    feed_file(driver, fixture)
    try:
        WebDriverWait(driver, 60).until(EC.presence_of_element_located((By.CSS_SELECTOR, "[data-import-seen]")))
    except TimeoutException:
        pass
    sent = [e.get_attribute("data-parts") for e in driver.find_elements(By.CSS_SELECTOR, "[data-import-seen]")]
    expected_parts = -(-fixture.stat().st_size // 1024)
    check(f"файл ушёл в программу частями: {expected_parts}",
          bool(sent) and sent[0] == str(expected_parts) and expected_parts > 1,
          f"частей «{sent[0] if sent else 'нет'}», байт {fixture.stat().st_size} | " + error_details(driver))
    seen = [e.text for e in driver.find_elements(By.CSS_SELECTOR, "[data-import-seen]")]
    check("файл прочитан: в нём 5 рождений, 2 брака, 4 смерти, село Никольское",
          bool(seen) and "рождений 5, браков 2, смертей 4" in seen[0] and "Никольское" in seen[0],
          (seen[0] if seen else driver.find_element(By.XPATH, PARISH).text.replace("\n", " | ")[:300])
          + " | " + error_details(driver))
    if not seen:
        return
    click(driver, PARISH + "//button[normalize-space()='Импортировать']")
    try:
        WebDriverWait(driver, 180).until(EC.presence_of_element_located((By.CSS_SELECTOR, "[data-import-done]")))
    except TimeoutException:
        pass
    done = [e.text for e in driver.find_elements(By.CSS_SELECTOR, "[data-import-done]")]
    check("импорт прошёл: 4 рождения, 2 брака, 4 смерти",
          bool(done) and "рождений 4, браков 2, смертей 4" in done[0],
          (done[0] if done else "итога нет") + " | " + error_details(driver))
    if not done:
        return
    report = driver.find_element(By.XPATH, PARISH).text
    check("пропущенная строка и несверенное имя названы, не молча",
          "Не перенесено строк: 1" in report and "Жданко" in report, report.replace("\n", " | ")[:400])
    click(driver, PARISH + "//button[normalize-space()='Перейти в приход']")
    reloaded(driver, wait, "Никольское (из Excel)")
    # Список на сверку после импорта (Роман 03.10.2026).
    rows = driver.execute_script(
        "return [...document.querySelectorAll('.parishrow')].map((e) => e.innerText);") or []
    check("на экране «Дело» — «На сверку после импорта»", any("На сверку после импорта" in r for r in rows), " || ".join(rows))
    click(driver, "//div[contains(@class,'parishrow')][contains(.,'На сверку')]//button")
    review = "//div[@data-modal='review']"
    wait.until(EC.visibility_of_element_located((By.XPATH, review)))
    listing = driver.find_element(By.XPATH, review).text
    check("в списке — несверенное имя и пропущенная строка",
          "Жданко" in listing and "не перенесена" in listing, listing.replace("\n", " | ")[:400])
    click(driver, review + "//tr[contains(.,'Жданко')]//button[normalize-space()='Открыть запись']")
    try:
        wait.until(EC.presence_of_element_located((By.CSS_SELECTOR, ".birth .editbar")))
    except TimeoutException:
        pass
    opened = driver.find_elements(By.CSS_SELECTOR, ".birth .editbar")
    check("«Открыть запись» из списка — запись в форме рождений", bool(opened), error_details(driver))
    if opened:
        # Запись с именем вне словаря форма сразу предлагает сверить — окно
        # закрывает всю форму (ревьюер 03.10.2026). Закрываем его клавишей.
        time.sleep(1.0)
        for _ in range(3):
            modals = driver.find_elements(By.CSS_SELECTOR, ".modal")
            if not modals:
                break
            modals[0].send_keys(Keys.ESCAPE)
            time.sleep(0.5)
        check("окно сверки имени закрыто", not driver.find_elements(By.CSS_SELECTOR, ".modal"))
        click(driver, "//div[contains(@class,'birth')]//div[contains(@class,'editbar')]//button[normalize-space()='Отменить']")
        time.sleep(0.5)
    driver.find_element(By.XPATH, "//nav//button[normalize-space()='Дело']").click()
    head = driver.find_element(By.CSS_SELECTOR, "h2.caseyear").text
    check("дело — на год: открыт последний год, в списке оба",
          "Дело за 1890 год" in head and "1889" in head, head.replace("\n", " | "))
    delo = field(driver, "Дело").get_attribute("value")
    check("у 1890 года своё дело — 12", delo == "12", f"«{delo}»")
    driver.find_element(By.XPATH, "//nav//button[normalize-space()='Рождения']").click()
    try:
        wait.until(EC.text_to_be_present_in_element((By.CSS_SELECTOR, ".birth"), "Набрано: 3"))
    except TimeoutException:
        pass
    block = driver.find_element(By.CSS_SELECTOR, ".birth").text
    year = field(driver, "Год", b).get_attribute("value")
    check("форма продолжает с последней записи импорта: 1890 год, три рождения",
          year == "1890" and "Набрано: 3 — за 1890 год" in block and "Жданко" in block,
          f"год «{year}» | " + block[-300:].replace("\n", " | "))
    # Подсказки «прогреты»: отец из импорта подсказывается с местом и званием.
    field(driver, "ИОФ", father).send_keys("Иван Сем")
    time.sleep(1.2)
    hints = driver.find_element(By.CSS_SELECTOR, ".birth").text
    check("персона из импорта подсказывается", "Иван Семенов" in hints, hints[:300].replace("\n", " | "))
    field(driver, "ИОФ", father).send_keys(Keys.ESCAPE)
    fresh_window(driver, wait)  # набранное для подсказки — не запись, убрать
    click(driver, "//button[normalize-space()='Выгрузить в Excel']")
    path = exported_path(driver, wait, "Excel")
    if path:
        rows = xlsx_rows(path, "Рождения")
        check("выгрузка — весь импортированный приход: 4 рождения, «Татьяна» 1889 года с делом 11 фонда 1",
              sum(1 for r in rows if r >= 3) == 4
              and any("Татьяна" in v.values() and "Ф.1 Оп.2 Д.11" in v.values() for v in rows.values()),
              str(list(rows.values()))[-400:])
        validate_xlsx(path, "Excel (импортированный приход)")
    # Повторный импорт того же файла программа замечает.
    open_parishes(driver, wait)
    feed_file(driver, fixture)
    try:
        WebDriverWait(driver, 60).until(EC.presence_of_element_located((By.CSS_SELECTOR, "[data-import-seen]")))
    except TimeoutException:
        pass
    again = driver.find_element(By.XPATH, PARISH).text
    check("повторный импорт: программа спрашивает — заменить или создать ещё один",
          "уже импортирован" in again and "заменить приход" in again, again.replace("\n", " | ")[:400])
    click(driver, PARISH + "//button[normalize-space()='Назад']")
    click(driver, PARISH + "//tr[.//b[normalize-space()='Борисоглебское']]//button[normalize-space()='Открыть']")
    reloaded(driver, wait, "Борисоглебское")


REPO = Path(__file__).resolve().parents[2]


def validate_xlsx(path, kind, baseline=None):
    """Файл выгрузки — валидатором Open XML (scripts/xlsx-validate).

    Инцидент 01.10.2026: Excel у Романа открыл выгрузку в Familio с «Ошибка в
    части содержимого… восстановить?» — из [Content_Types].xml пропали все
    <Default>. Разбор XML (xlsx_rows выше) и openpyxl этого не видят, а
    валидатор проверяет файл по схеме формата, как Excel. С образцом
    (baseline) в счёт идут только ошибки, которых нет в самом образце.
    """
    cmd = ["dotnet", "run", "--project", str(REPO / "scripts" / "xlsx-validate"), "--", str(path)]
    if baseline:
        cmd.append(str(baseline))
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=600)
    except Exception as e:  # noqa: BLE001 — нет dotnet = провал, не пропуск
        check(f"файл {kind} проходит валидатор Open XML", False, f"валидатор не запустился: {e}")
        return
    tail = " | ".join((r.stdout + r.stderr).strip().splitlines()[-6:])
    check(f"файл {kind} проходит валидатор Open XML (новых ошибок 0)",
          r.returncode == 0, tail[:600])


def exported_path(driver, wait, kind, reveal=False):
    """Путь выгруженного файла — из окна итога выгрузки; окно закрывается.

    С 03.10.2026 итог — отдельным окном (Роман: строку под кнопками «легко
    пропустить»): в нём число записей, путь, «Показать в папке» и «ОК».
    """
    box = "//div[@data-modal='exported']"
    try:
        wait.until(EC.text_to_be_present_in_element((By.CSS_SELECTOR, ".exportdone"), f"Выгружено в {kind}"))
    except TimeoutException:
        pass
    done = driver.find_elements(By.CSS_SELECTOR, ".exportdone")
    errorbar = [e.text for e in driver.find_elements(By.CSS_SELECTOR, ".errorbar")]
    path = done[0].get_attribute("data-export-path") if done else None
    check(f"выгрузка в {kind} прошла", bool(path) and f"Выгружено в {kind}" in done[0].text,
          " | ".join(errorbar))
    if path:
        check(f"файл {kind} лежит на диске", Path(path).is_file(), path)
    if done:
        if reveal:
            click(driver, box + "//button[normalize-space()='Показать в папке']")
            time.sleep(1.5)
            bars = [e.text for e in driver.find_elements(By.CSS_SELECTOR, ".errorbar")]
            check("«Показать в папке» отработала без ошибки", not bars, " | ".join(bars))
        click(driver, box + "//button[normalize-space()='ОК']")
        time.sleep(0.4)
        check("окно итога выгрузки закрылось", not driver.find_elements(By.XPATH, box))
    return path if path and Path(path).is_file() else None


def xlsx_rows(path, sheet):
    """Лист выгруженного файла: номер строки → {буквы колонки: текст}.
    Без openpyxl: xlsx — zip с XML, этого хватает для проверки."""
    import re
    import zipfile
    import xml.etree.ElementTree as ET
    ns = {"m": "http://schemas.openxmlformats.org/spreadsheetml/2006/main"}
    rel = "{http://schemas.openxmlformats.org/officeDocument/2006/relationships}id"
    z = zipfile.ZipFile(path)
    shared = []
    if "xl/sharedStrings.xml" in z.namelist():
        for si in ET.fromstring(z.read("xl/sharedStrings.xml")).findall("m:si", ns):
            shared.append("".join(t.text or "" for t in si.iter(f"{{{ns['m']}}}t")))
    wb = ET.fromstring(z.read("xl/workbook.xml"))
    rels = {r.get("Id"): r.get("Target") for r in ET.fromstring(z.read("xl/_rels/workbook.xml.rels"))}
    target = next(rels[s.get(rel)] for s in wb.find("m:sheets", ns) if s.get("name") == sheet)
    target = target.lstrip("/")
    target = target if target.startswith("xl/") else "xl/" + target
    out = {}
    for row in ET.fromstring(z.read(target)).iter(f"{{{ns['m']}}}row"):
        cells = {}
        for c in row.findall("m:c", ns):
            col = re.match(r"[A-Z]+", c.get("r")).group(0)
            if c.get("t") == "inlineStr":
                cells[col] = "".join(t.text or "" for t in c.iter(f"{{{ns['m']}}}t"))
            elif c.get("t") == "s":
                cells[col] = shared[int(c.find("m:v", ns).text)]
            elif c.find("m:v", ns) is not None:
                cells[col] = c.find("m:v", ns).text or ""
        out[int(row.get("r"))] = cells
    return out


def main() -> int:
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8", errors="replace")
        except (AttributeError, ValueError):
            pass

    exe, archive = Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve()
    msedgedriver = Path(sys.argv[3]).resolve()
    for what, path in (("приложения", exe), ("архива", archive), ("msedgedriver", msedgedriver)):
        if not path.exists():
            print(f"нет {what}: {path}")
            return 1

    app, driver, wait = launch(exe, msedgedriver)
    if driver is None:
        return 1

    def snapshot():
        if fail_count:
            try:
                driver.get_screenshot_as_file("e2e-failure.png")
                print("снимок: e2e-failure.png")
            except Exception as e:  # noqa: BLE001 — снимок не должен прятать провал
                print(f"снимок не снялся: {e}")

    try:
        run(driver, wait, archive)
    except Exception as e:  # noqa: BLE001 — любое исключение = провал со снимком
        check("сценарий дошёл до конца", False, f"{type(e).__name__}: {e}")
    finally:
        snapshot()
        stop(app, driver)

    if not fail_count:
        # Второй запуск — та же база в папке данных, форма должна продолжить.
        app, driver, wait = launch(exe, msedgedriver)
        if driver is None:
            check("приложение запустилось второй раз", False)
        else:
            try:
                resumed(driver, wait)
            except Exception as e:  # noqa: BLE001
                check("сценарий перезапуска дошёл до конца", False, f"{type(e).__name__}: {e}")
            finally:
                snapshot()
                stop(app, driver)

    if fail_count:
        print_app_log()

    print(f"\nИтог: успешно {ok_count}, ошибок {fail_count}")
    return 1 if fail_count else 0


if __name__ == "__main__":
    raise SystemExit(main())
