# Спецификация: архив через JSON-аргумент и сквозная проверка на Windows

По замыслу `intent/2026-09-14-arhiv-ne-zagruzilsya.md`. Инцидент.

## Диагноз

`import_archive` принимала `tauri::ipc::Request` и требовала `InvokeBody::Raw`.
Tauri шлёт сырое тело через `fetch` на `http://ipc.localhost`; если `fetch`
падает, молча переходит на `window.ipc.postMessage`, где всё уходит JSON-ом
(`Uint8Array` → массив чисел). У Романа на Windows тело пришло как JSON —
отсюда «пришло что-то другое». Сырое тело на Windows ненадёжно.

## 1. Байты — обычным аргументом

`invoke("import_archive", { bytes })`, в Rust `bytes: Vec<u8>`. Работает
через любой транспорт. Ошибка при неверном файле говорит, сколько байт
пришло и с чего начинаются.

Критерии: (а) в `main.rs` нет `tauri::ipc::Request` и `InvokeBody::Raw`;
(б) `CaseHeader.tsx` зовёт команду с объектом `{ bytes }`; (в) регресс
в `scripts/test_incidents.py`.

## 2. Сквозная проверка на Windows-раннере

В job сборки на `windows-latest`, после `tauri build` и до выкладки:
приложение запускается с `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port`,
`msedgedriver` (под версию WebView2 из реестра) подключается к нему —
подход «attach» из документации Microsoft. Первый вариант через `tauri-driver`
(«launch») упал 14.09 на «DevToolsActivePort file doesn't exist» без диагностики;
при явном запуске видно, живо ли приложение, и его журнал. Второй прогон
(attach через `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`): приложение живо, а порт
закрыт — переменная до WebView2 не дошла. Поэтому порт открывает само
приложение по `GENMETRIC_E2E_DEBUG_PORT` (`main.rs`, только Windows, только
с переменной; окно для этого создаётся в `setup`, в конфигурации `create: false`). Сценарий `scripts/e2e/windows.py`: окно и база
открылись; дело сохраняется; архив (синтетический, `scripts/e2e/make_archive.py`)
загружается через `<input type=file>` и настоящий IPC — под кнопкой
«Добавлено: персон 4»; отцу предлагается персона из архива; запись девочки
сохраняется с «№ ж.». Провал — снимок экрана артефактом и нет выпуска.

Критерии: шаг стоит в `build.yml` до `upload-artifact`; при зелёной сборке
в логе шага «Итог: успешно N, ошибок 0».

## Чего сознательно не делаем

Не выясняем причину падения `fetch` в WebView2. Не переписываем инструкцию
Роману — только письмо «скачайте заново, нажмите ту же кнопку».
