#!/usr/bin/env python3
"""
Сквозная проверка собранного приложения на настоящем Windows.

Зачем. 13.09.2026 Роман получил сборку, в которой кнопка «Загрузить архив»
не работала: сырое тело IPC-запроса на WebView2 не доходило. В песочнице
это не воспроизвести — стенд не Tauri, cargo build невозможен. Единственное
место, где есть настоящий Windows с настоящим WebView2, — раннер конвейера.
Поэтому здесь: запускаем собранный genmetric.exe через tauri-driver
(WebDriver поверх WebView2) и проходим путь Романа руками робота.

Что проверяется:
  1. окно открылось, база справочников подключилась (нет экрана «База не открылась»);
  2. дело заполняется и сохраняется;
  3. архив загружается через <input type=file> и настоящий IPC: под кнопкой
     появляется «Добавлено: персон …»;
  4. подсказка отца видит персону из архива;
  5. запись девочки сохраняется, и в списке «Набрано» у неё «№ ж.».

Запуск (в конвейере, см. .github/workflows/build.yml):
    python scripts/e2e/windows.py путь\\к\\genmetric.exe путь\\к\\архив.sqlite

tauri-driver должен уже слушать 127.0.0.1:4444. При провале рядом остаётся
снимок e2e-failure.png — его конвейер выкладывает артефактом.
"""

import sys
from pathlib import Path

from selenium import webdriver
from selenium.common.exceptions import TimeoutException
from selenium.webdriver.common.by import By
from selenium.webdriver.common.keys import Keys
from selenium.webdriver.common.options import ArgOptions
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
                         ("Губерния", "Костромская"), ("Год", "1897")]:
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
    wait.until(EC.text_to_be_present_in_element(
        (By.XPATH, child + "/following-sibling::*[contains(@class,'parsedline')] | " + child + "//*[contains(@class,'parsedline')]"), "Ж"))
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


def main() -> int:
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8", errors="replace")
        except (AttributeError, ValueError):
            pass

    exe, archive = Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve()
    if not exe.exists():
        print(f"нет приложения: {exe}")
        return 1
    if not archive.exists():
        print(f"нет архива: {archive}")
        return 1

    opts = ArgOptions()
    opts.set_capability("browserName", "wry")
    opts.set_capability("tauri:options", {"application": str(exe)})
    driver = webdriver.Remote(command_executor="http://127.0.0.1:4444", options=opts)
    wait = WebDriverWait(driver, 30)

    try:
        run(driver, wait, archive)
    except Exception as e:  # noqa: BLE001 — любое исключение = провал со снимком
        check("сценарий дошёл до конца", False, f"{type(e).__name__}: {e}")
    finally:
        if fail_count:
            try:
                driver.get_screenshot_as_file("e2e-failure.png")
                print("снимок: e2e-failure.png")
            except Exception as e:  # noqa: BLE001 — снимок не должен прятать провал
                print(f"снимок не снялся: {e}")
        driver.quit()

    print(f"\nИтог: успешно {ok_count}, ошибок {fail_count}")
    return 1 if fail_count else 0


if __name__ == "__main__":
    raise SystemExit(main())
