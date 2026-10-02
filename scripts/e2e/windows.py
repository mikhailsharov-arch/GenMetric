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
  6. после перезапуска приложения форма продолжает с места: счёт на месте;
  7. сохранённая запись открывается в форму, правится и сохраняется без дублей;
  …
  13. умерший без имени («личность не установлена») сохраняется;
  14. выгрузка в Familio и в Excel пишет файлы, в них набранные записи;
  15. новый приход — отдельный файл: записи не смешиваются;
  16. импорт Excel-индексатора (db/fixtures/indexer.xlsx) через окно.

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

from selenium import webdriver
from selenium.common.exceptions import NoSuchElementException, TimeoutException
from selenium.webdriver.common.by import By
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
    el.send_keys(Keys.ESCAPE)  # закрыть подсказку, если открылась


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

    print("\n2. Дело")
    for label, value in [("Архив", "ГА Костромской области"), ("Церковь", "Христорождественская"),
                         ("Село", "Борисоглебское"), ("Уезд", "Макарьевский"),
                         ("Губерния", "Костромская")]:
        fill(driver, label, value)
    driver.find_element(By.XPATH, "//button[normalize-space()='Сохранить дело']").click()
    wait.until(EC.text_to_be_present_in_element((By.TAG_NAME, "body"), "Сохранено"))
    check("дело сохранено", True)

    print("\n3. Архив через настоящий IPC")
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
    driver.find_element(By.XPATH, "//nav//button[normalize-space()='Рождения']").click()
    wait.until(EC.presence_of_element_located((By.XPATH, "//section[.//h2[normalize-space()='Отец']]")))
    time.sleep(1)
    count = field(driver, "Счёт").get_attribute("value")
    check("счёт восстановлен: 7", count == "7", f"«{count}»")
    page = field(driver, "Стр.").get_attribute("value")
    check("страница восстановлена: 939об-940", page == "939об-940", f"«{page}»")
    year = field(driver, "Год").get_attribute("value")
    check("год восстановлен: 1897", year == "1897", f"«{year}»")
    body = driver.find_element(By.TAG_NAME, "body").text
    check("список набранного на месте", "Набрано: 1" in body)
    check("причт восстановлен (21.09.2026)", "Александр Рождественский" in body)
    # Записать ещё одну — с восстановленным причтом (ревьюер 21.09.2026: причт
    # приходит без разбора, и сохранение должно пройти без ошибок).
    fill(driver, "Счёт", "8")
    fill(driver, "Ребёнок", "Иван")
    child = "//div[contains(@class,'field')][./label[normalize-space()='Ребёнок']]"
    wait.until(EC.text_to_be_present_in_element((By.XPATH, child + "//*[contains(@class,'parsedline')]"), "М"))
    click(driver, "//button[starts-with(normalize-space(),'Сохранить и следующая')]")
    try:
        wait.until(EC.text_to_be_present_in_element((By.TAG_NAME, "body"), "Набрано: 2"))
    except TimeoutException:
        pass
    body = driver.find_element(By.TAG_NAME, "body").text
    errorbar = [e.text for e in driver.find_elements(By.CSS_SELECTOR, ".errorbar")]
    check("вторая запись после перезапуска сохранена", "Набрано: 2" in body, " | ".join(errorbar))
    check("у мальчика «№ м. 8»", "№ м. 8" in body)

    print("\n7. Правка сохранённой записи (заказчик 22.09.2026)")
    # Открываем последнюю (первую в списке — мальчик, счёт 8), меняем счёт на 9.
    click(driver, "(//table[contains(@class,'saved')]//button[starts-with(normalize-space(),'Открыть')])[1]")
    wait.until(EC.presence_of_element_located((By.CSS_SELECTOR, ".editbar")))
    lifted = field(driver, "Ребёнок").get_attribute("value")
    check("запись поднялась в форму: ребёнок «Иван»", lifted == "Иван", f"«{lifted}»")
    # Запись «Иван» сохранена после перезапуска с восстановленным причтом —
    # имя причта обязано быть в базе (инцидент 22.09.2026: терялось).
    body = driver.find_element(By.TAG_NAME, "body").text
    check("у записи после перезапуска причт с именем", "Александр Рождественский" in body)
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
    check("причт из рождений виден в браках", "Александр Рождественский" in block)
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
    check("причт общий и здесь", "Александр Рождественский" in block)
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
    path = exported_path(driver, wait, "Excel")
    if path:
        for sheet, what in (("Рождения", "Мария"), ("Браки", "Михаил Дмитриев"), ("Смерти", "утонул в Волге")):
            rows = xlsx_rows(path, sheet)
            check(f"Excel, лист «{sheet}»: есть «{what}»",
                  any(what in " ".join(v.values()) for v in rows.values()), str(list(rows.values()))[-300:])
        mk = xlsx_rows(path, "МК")
        check("Excel, лист «МК»: строки персон есть", len(mk) > 4, f"строк {len(mk)}")
        validate_xlsx(path, "Excel")
        # «Показать в папке»: ветка для Windows (explorer /select) на Mac не
        # компилируется и не выполнялась ни разу — проверяем, что команда
        # проходит без полосы ошибок (ревьюер #39).
        click(driver, "//div[contains(@class,'exportpanel')]//button[normalize-space()='Показать в папке']")
        time.sleep(1.5)
        errorbar = [e.text for e in driver.find_elements(By.CSS_SELECTOR, ".errorbar")]
        check("«Показать в папке» отработала без ошибки", not errorbar, " | ".join(errorbar))

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
    click(driver, "//div[contains(@class,'parishrow')]//button")
    wait.until(EC.visibility_of_element_located((By.XPATH, PARISH)))


def reloaded(driver, wait, name):
    """После смены прихода окно перечитывается целиком: ждём новое название."""
    try:
        WebDriverWait(driver, 90).until(lambda d: name in " ".join(
            e.text for e in d.find_elements(By.CSS_SELECTOR, ".parishrow")))
    except TimeoutException:
        pass
    row = " ".join(e.text for e in driver.find_elements(By.CSS_SELECTOR, ".parishrow"))
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
    feed_file(driver, fixture)
    try:
        WebDriverWait(driver, 60).until(EC.presence_of_element_located((By.CSS_SELECTOR, "[data-import-seen]")))
    except TimeoutException:
        pass
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


def exported_path(driver, wait, kind):
    """Путь выгруженного файла — из строки «Выгружено в …» под кнопками."""
    try:
        wait.until(EC.text_to_be_present_in_element((By.CSS_SELECTOR, ".exportpanel"), f"Выгружено в {kind}"))
    except TimeoutException:
        pass
    done = driver.find_elements(By.CSS_SELECTOR, ".exportdone")
    errorbar = [e.text for e in driver.find_elements(By.CSS_SELECTOR, ".errorbar")]
    path = done[0].get_attribute("data-export-path") if done else None
    check(f"выгрузка в {kind} прошла", bool(path) and f"Выгружено в {kind}" in done[0].text,
          " | ".join(errorbar))
    if path:
        check(f"файл {kind} лежит на диске", Path(path).is_file(), path)
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
