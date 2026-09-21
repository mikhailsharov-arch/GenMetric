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
  6. после перезапуска приложения форма продолжает с места: счёт на месте.

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
from selenium.common.exceptions import TimeoutException
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


def field(driver, label, scope="//"):
    """Поле формы по подписи: <div class="field"><label>НП</label>…<input>."""
    return driver.find_element(
        By.XPATH, f"{scope}div[contains(@class,'field')][./label[normalize-space()='{label}']]//input")


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

    print("\n2. Дело")
    for label, value in [("Архив", "ГА Костромской области"), ("Церковь", "Христорождественская"),
                         ("Село", "Борисоглебское"), ("Уезд", "Макарьевский"),
                         ("Губерния", "Костромская"), ("Год начала", "1897")]:
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
    fill(driver, "Счёт", "7")
    fill(driver, "Ребёнок", "Мария")
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
    driver.find_element(By.XPATH, "//button[starts-with(normalize-space(),'Сохранить и следующая')]").click()
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
    check("список набранного на месте", "Набрано: 1" in driver.find_element(By.TAG_NAME, "body").text)


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
